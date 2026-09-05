use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};
use tracing::Instrument;
use trueos::{
    logl::{self, level},
    runtime, trace,
};

pub mod stack {
    tonic::include_proto!("trueos.stack");
}

use stack::stack_witness_client::StackWitnessClient;
use stack::stack_witness_server::{StackWitness, StackWitnessServer};
use stack::{StackReply, StackRequest};

const GENERATION: u32 = 0x5453_0001;
const LANES: usize = 2;
const CLIENTS: usize = 4;
const REQUESTS: usize = 8;
const DEADLINE: Duration = Duration::from_secs(20);
const REQUEST_PREFIX: &[u8] = b"trueos/";
const REQUEST_BODY: &[u8] = b"tokio-stack";
const RESPONSE_PREFIX: &[u8] = b"materialized:";
#[derive(Default)]
struct Witness;

#[tonic::async_trait]
impl StackWitness for Witness {
    async fn materialize(
        &self,
        request: Request<StackRequest>,
    ) -> Result<Response<StackReply>, Status> {
        let request = request.into_inner();
        tracing::info!(
            generation = request.generation,
            payload_len = request.payload.len(),
            "tonic server request"
        );

        if request.generation >> 16 != GENERATION >> 16 {
            return Err(Status::failed_precondition("generation mismatch"));
        }
        if request.payload.as_slice() != b"trueos/tokio-stack" {
            return Err(Status::invalid_argument("payload mismatch"));
        }

        let mut payload = BytesMut::with_capacity(RESPONSE_PREFIX.len() + request.payload.len());
        payload.put_slice(RESPONSE_PREFIX);
        payload.put_slice(&request.payload);

        Ok(Response::new(StackReply {
            payload: payload.freeze().to_vec(),
            generation: request.generation,
            runtime: "trueos-blueprint-hypervisor".to_string(),
        }))
    }
}

fn main() {
    logl::log(level::INFO, format_args!("tokio_stack: start"));

    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("tokio_stack: failed runtime.current_thread_net.build: {error}"),
            );
            return;
        }
    };

    let emitted_before = trace::emitted_events();
    let result = trace::with_default(|| {
        runtime.block_on(run_fleet().instrument(tracing::info_span!(
            "tokio_stack.materialization",
            generation = GENERATION
        )))
    });

    match result {
        Ok(()) if trace::emitted_events() > emitted_before => logl::log(
            level::INFO,
            format_args!(
                "tokio_stack: PASS runtime=mio,tokio bytes=direct hyper=tcp tower=service tracing=kernel-log tonic=grpc lanes=2 clients_per_lane=4 requests_per_client=8 trace_events={}",
                trace::emitted_events() - emitted_before
            ),
        ),
        Ok(()) => logl::log(
            level::ERROR,
            format_args!("tokio_stack: failed stage=tracing.subscriber.no_output"),
        ),
        Err(stage) => logl::log(
            level::ERROR,
            format_args!("tokio_stack: failed stage={stage}"),
        ),
    }
}

async fn run_fleet() -> Result<(), &'static str> {
    if trueos::worker::capacity() < LANES {
        return Err("insufficient-native-capacity");
    }
    let main_slot = trueos::worker::local_slot();
    let (ready_tx, mut ready_rx) = tokio::sync::mpsc::channel(LANES);
    let release = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut jobs = Vec::new();
    let mut error = None;
    for lane in 0..LANES {
        let ready_tx = ready_tx.clone();
        let release = release.clone();
        let cancel = cancel.clone();
        match trueos::worker::spawn(move || {
            // A scoped main-lane subscriber does not propagate to a worker.
            trace::with_default(|| {
                let runtime = runtime::current_thread_net()
                    .build()
                    .map_err(|_| "worker.runtime")?;
                let result = runtime.block_on(async {
                    let slot = trueos::worker::local_slot();
                    ready_tx
                        .send((lane, slot))
                        .await
                        .map_err(|_| "worker.ready.send")?;
                    tokio::time::timeout(DEADLINE, async {
                        while !release.load(Ordering::Acquire) {
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    })
                    .await
                    .map_err(|_| "worker.release.timeout")?;
                    if cancel.load(Ordering::Acquire) {
                        return Err("worker.cancelled");
                    }
                    tokio::time::timeout(
                        DEADLINE,
                        run_probe().instrument(tracing::info_span!("native.runtime", lane)),
                    )
                    .await
                    .map_err(|_| "worker.probe.timeout")??;
                    if trueos::worker::local_slot() != slot {
                        return Err("worker.slot.moved");
                    }
                    Ok(slot)
                });
                drop(runtime);
                result
            })
        }) {
            Ok(job) => jobs.push(job),
            Err(_) => {
                error = Some("worker.submit");
                break;
            }
        }
    }
    drop(ready_tx);
    let mut slots = [None; LANES];
    if error.is_none() {
        for _ in 0..LANES {
            match tokio::time::timeout(DEADLINE, ready_rx.recv()).await {
                Ok(Some((lane, slot))) if lane < LANES && slots[lane].is_none() => {
                    slots[lane] = Some(slot)
                }
                _ => {
                    error = Some("worker.ready.timeout-or-protocol");
                    break;
                }
            }
        }
    }
    cancel.store(error.is_some(), Ordering::Release);
    release.store(true, Ordering::Release);
    for (lane, mut job) in jobs.into_iter().enumerate() {
        let joined = match tokio::time::timeout(DEADLINE, &mut job).await {
            Ok(result) => result,
            Err(_) => {
                error.get_or_insert("worker.join.timeout");
                logl::log(
                    level::ERROR,
                    format_args!(
                        "tokio_stack: FAIL lane={lane} stage=worker.join.timeout action=draining"
                    ),
                );
                cancel.store(true, Ordering::Release);
                job.await
            }
        };
        match joined {
            Ok(Ok(slot)) if slots[lane] == Some(slot) => {}
            Ok(Err(stage)) => {
                error.get_or_insert(stage);
            }
            _ => {
                error.get_or_insert("worker.join.result");
            }
        }
    }
    if slots[0].is_none()
        || slots[1].is_none()
        || slots[0] == slots[1]
        || slots.contains(&Some(main_slot))
    {
        error.get_or_insert("distinct-worker-slots");
    }
    error.map_or(Ok(()), Err)
}

async fn run_probe() -> Result<(), &'static str> {
    tracing::info!("full stack witness entered");
    let request_payload = probe_bytes()?;
    probe_tower_service(request_payload.clone()).await?;
    probe_tonic_loopback(request_payload).await?;
    Ok(())
}

fn probe_bytes() -> Result<Bytes, &'static str> {
    logl::log(
        level::INFO,
        format_args!("tokio_stack: stage bytes.materialize"),
    );

    let mut mutable = BytesMut::with_capacity(REQUEST_PREFIX.len() + REQUEST_BODY.len() + 4);
    mutable.put_slice(REQUEST_PREFIX);
    mutable.put_slice(REQUEST_BODY);
    mutable.put_u32(GENERATION);

    let mut payload = mutable.freeze();
    let shared = payload.clone();
    let mut generation = payload.split_off(payload.len() - 4);

    if payload.as_ref() != b"trueos/tokio-stack"
        || generation.get_u32() != GENERATION
        || shared.len() != REQUEST_PREFIX.len() + REQUEST_BODY.len() + 4
    {
        return Err("bytes.materialize.value");
    }

    logl::log(
        level::INFO,
        format_args!(
            "tokio_stack: success bytes.materialize payload={} shared={}",
            payload.len(),
            shared.len()
        ),
    );
    Ok(payload)
}

async fn probe_tower_service(payload: Bytes) -> Result<(), &'static str> {
    use tower::{ServiceExt, service_fn};

    logl::log(
        level::INFO,
        format_args!("tokio_stack: stage tower.service.oneshot"),
    );
    let service = service_fn(|request: Bytes| async move {
        tracing::debug!(payload_len = request.len(), "tower request");
        Ok::<usize, core::convert::Infallible>(request.len())
    });
    let len = service
        .oneshot(payload)
        .await
        .map_err(|_| "tower.service.oneshot")?;
    if len != REQUEST_PREFIX.len() + REQUEST_BODY.len() {
        return Err("tower.service.value");
    }
    logl::log(
        level::INFO,
        format_args!("tokio_stack: success tower.service.oneshot"),
    );
    Ok(())
}

async fn probe_tonic_loopback(payload: Bytes) -> Result<(), &'static str> {
    logl::log(
        level::INFO,
        format_args!("tokio_stack: stage tonic.grpc.loopback"),
    );

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| "tonic.server.bind")?;
    let address = listener
        .local_addr()
        .map_err(|_| "tonic.server.local_addr")?;
    let incoming = TcpListenerStream::new(listener);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(
        async move {
            tonic::transport::Server::builder()
                .add_service(StackWitnessServer::new(Witness))
                .serve_with_incoming_shutdown(incoming, async {
                    let _ = shutdown_rx.await;
                })
                .await
                .map_err(|_| "tonic.server.serve")
        }
        .instrument(tracing::info_span!("tonic.server")),
    );

    tokio::task::yield_now().await;

    let mut clients = tokio::task::JoinSet::new();
    for client_index in 0..CLIENTS {
        let payload = payload.clone();
        clients.spawn(async move {
            let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
                .map_err(|_| "tonic.client.endpoint")?
                .connect_timeout(Duration::from_secs(2))
                .timeout(Duration::from_secs(2));
            let channel = endpoint
                .connect()
                .await
                .map_err(|_| "tonic.client.connect")?;
            let mut client = StackWitnessClient::new(channel);
            let mut expected = BytesMut::with_capacity(RESPONSE_PREFIX.len() + payload.len());
            expected.put_slice(RESPONSE_PREFIX);
            expected.put_slice(&payload);
            for request_index in 0..REQUESTS {
                let generation = GENERATION + (client_index * REQUESTS + request_index) as u32;
                let reply = client
                    .materialize(StackRequest {
                        payload: payload.to_vec(),
                        generation,
                    })
                    .instrument(tracing::info_span!(
                        "tonic.client.materialize",
                        client_index,
                        request_index
                    ))
                    .await
                    .map_err(|_| "tonic.client.materialize")?
                    .into_inner();
                if reply.payload.as_slice() != expected.as_ref()
                    || reply.generation != generation
                    || reply.runtime != "trueos-blueprint-hypervisor"
                {
                    return Err("tonic.client.reply");
                }
                tokio::task::yield_now().await;
            }
            Ok::<_, &'static str>(REQUESTS)
        });
    }
    let mut error = None;
    let mut replies = 0;
    while let Some(joined) = clients.join_next().await {
        match joined {
            Ok(Ok(count)) => replies += count,
            Ok(Err(stage)) => {
                error.get_or_insert(stage);
            }
            Err(_) => {
                error.get_or_insert("tonic.client.join");
            }
        }
    }
    let _ = shutdown_tx.send(()); // success or failure, request server shutdown
    let mut server = server;
    match tokio::time::timeout(Duration::from_secs(3), &mut server).await {
        Ok(Ok(Ok(()))) => {}
        Ok(_) => {
            error.get_or_insert("tonic.server.result");
        }
        Err(_) => {
            error.get_or_insert("tonic.server.shutdown.timeout");
            server.abort();
            let _ = server.await;
        }
    }
    if replies != CLIENTS * REQUESTS {
        error.get_or_insert("tonic.client.reply.count");
    }
    if let Some(stage) = error {
        return Err(stage);
    }
    logl::log(
        level::INFO,
        format_args!(
            "tokio_stack: success tonic.grpc.loopback addr={address} clients={CLIENTS} requests={REQUESTS} replies={replies} shutdown=joined"
        ),
    );

    Ok(())
}
