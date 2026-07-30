use bytes::{Buf, BufMut, Bytes, BytesMut};
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

        if request.generation != GENERATION {
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
        runtime.block_on(run_probe().instrument(tracing::info_span!(
            "tokio_stack.materialization",
            generation = GENERATION
        )))
    });

    match result {
        Ok(()) if trace::emitted_events() > emitted_before => logl::log(
            level::INFO,
            format_args!(
                "tokio_stack: done runtime=mio,tokio bytes=direct hyper=tcp tower=service tracing=kernel-log tonic=grpc trace_events={}",
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

    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
        .map_err(|_| "tonic.client.endpoint")?
        .connect_timeout(core::time::Duration::from_secs(2))
        .timeout(core::time::Duration::from_secs(2));
    let channel = endpoint
        .connect()
        .await
        .map_err(|_| "tonic.client.connect")?;
    let mut client = StackWitnessClient::new(channel);
    let reply = client
        .materialize(StackRequest {
            payload: payload.to_vec(),
            generation: GENERATION,
        })
        .instrument(tracing::info_span!("tonic.client.materialize"))
        .await
        .map_err(|_| "tonic.client.materialize")?
        .into_inner();

    let mut expected = BytesMut::with_capacity(RESPONSE_PREFIX.len() + payload.len());
    expected.put_slice(RESPONSE_PREFIX);
    expected.put_slice(&payload);
    if reply.payload.as_slice() != expected.as_ref()
        || reply.generation != GENERATION
        || reply.runtime != "trueos-blueprint-hypervisor"
    {
        return Err("tonic.client.reply");
    }

    let _ = shutdown_tx.send(());
    server
        .await
        .map_err(|_| "tonic.server.join")?
        .map_err(|_| "tonic.server.result")?;

    logl::log(
        level::INFO,
        format_args!(
            "tokio_stack: success tonic.grpc.loopback addr={} reply_bytes={}",
            address,
            reply.payload.len()
        ),
    );
    Ok(())
}
