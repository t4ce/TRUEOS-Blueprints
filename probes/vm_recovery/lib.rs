//! Lifecycle probe: all CPU work yields; only /panic requests native termination.
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, AtomicU64, Ordering}};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(target_os = "trueos")]
mod hull_stream_transport;

const WORKERS_PER_RUNTIME: usize = 2;
#[cfg(target_os = "trueos")]
const DIRECTORY: &str = "/common/vm-recovery";
#[cfg(target_os = "trueos")]
const COMMITTED: &str = "/common/vm-recovery/committed.txt";

fn record(state: &str, detail: impl std::fmt::Display) {
    #[cfg(target_os = "trueos")]
    trueos::logl::log(trueos::logl::level::INFO,
        format_args!("vm_recovery: {state} {detail}"));
    #[cfg(not(target_os = "trueos"))]
    println!("vm_recovery: {state} {detail}");
}

#[cfg(target_os = "trueos")]
struct Stream(u32);

#[cfg(target_os = "trueos")]
impl Stream {
    fn begin(instance: &str) -> Result<Self, String> {
        let path = format!("{DIRECTORY}/abandoned-{instance}.bin");
        // Setup runs on the Hull. Use its existing VM transport: synchronous
        // filesystem CABI imports were retired from the public SDK.
        use hull_stream_transport as vm;
        let (status, result) = vm::call_with_payload(vm::OP_BP_FS_WRITE_BEGIN,
            4096, path.as_bytes());
        let handle = result as u32;
        if status != vm::STATUS_OK || (result as i64) <= 0 {
            return Err(format!("stream-begin status={status} result={result} handle={handle}"));
        }
        let stream = Self(handle);
        let data = b"deliberately incomplete stream; BSP must abort on retirement\n";
        let (status, result) = vm::call_with_payload(vm::OP_BP_FS_WRITE_CHUNK,
            u64::from(handle), data);
        if status != vm::STATUS_OK || result != 0 {
            return Err(format!("stream-chunk status={status} result={result}"));
        }
        Ok(stream)
    }
}

#[cfg(target_os = "trueos")]
impl Drop for Stream {
    fn drop(&mut self) {
        let _ = hull_stream_transport::call_with_payload(
            hull_stream_transport::OP_BP_FS_WRITE_ABORT, u64::from(self.0), &[]);
    }
}

#[cfg(target_os = "trueos")]
struct Graphics {
    _frame: trueos::ui4_scene::Frame,
    device: trueos::vgpu::Device,
    buffer: trueos::vgpu::Buffer,
}

#[cfg(target_os = "trueos")]
impl Graphics {
    fn open(instance: &str) -> Result<Self, String> {
        let mut frame = trueos::ui4_scene::Frame::open(40, 40, 320, 180)
            .map_err(|error| format!("ui4-open {error:?}"))?;
        frame.set_title(&format!("VM recovery {}", &instance[..8]))
            .map_err(|error| format!("ui4-title {error:?}"))?;
        frame.begin(trueos::ui4_scene::rgba(24, 92, 144, 255))
            .map_err(|error| format!("ui4-begin {error:?}"))?;
        frame.publish(trueos::ui4_scene::Damage { x: 0, y: 0, width: 320, height: 180 })
            .map_err(|error| format!("ui4-publish {error:?}"))?;
        let device = trueos::vgpu::Device::open(trueos::vgpu::Capabilities::BUFFER)
            .map_err(|error| format!("vgpu-open status={error}"))?;
        let buffer = device.create_buffer(65536, trueos::vgpu::BUFFER_USAGE_STORAGE)
            .map_err(|error| format!("vgpu-buffer status={error}"))?;
        record("UI4", format_args!("instance={instance} window={} device={} buffer={}",
            frame.window_id(), device.raw(), buffer.raw()));
        Ok(Self { _frame: frame, device, buffer })
    }
}

#[cfg(target_os = "trueos")]
impl Drop for Graphics {
    fn drop(&mut self) {
        let _ = self.device.destroy_buffer(self.buffer);
        let _ = self.device.close();
        // Frame::drop closes its own window after this method returns.
    }
}

struct State {
    instance: String,
    started: AtomicUsize,
    ticks: [AtomicU64; 2],
    parked: AtomicBool,
    native_control: std::sync::mpsc::Sender<bool>,
    done: AtomicBool,
}

fn runtime(state: &Arc<State>, index: usize) -> Result<tokio::runtime::Runtime, String> {
    let start = state.clone();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKERS_PER_RUNTIME)
        .thread_name(format!("vm-recovery-rt{index}"))
        .enable_all()
        .on_thread_start(move || { start.started.fetch_add(1, Ordering::AcqRel); })
        .build().map_err(|error| format!("runtime-{index} {error}"))?;
    let ticker = state.clone();
    runtime.spawn(async move {
        while !ticker.done.load(Ordering::Acquire) {
            ticker.ticks[index].fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    });
    Ok(runtime)
}

async fn serve(listener: tokio::net::TcpListener, state: Arc<State>) {
    loop {
        let (mut socket, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => { record("FAIL", format_args!("tcp-accept error={error}")); return; }
        };
        let mut request = [0u8; 1024];
        let count = match tokio::time::timeout(Duration::from_secs(3), socket.read(&mut request)).await {
            Ok(Ok(count)) if count != 0 => count,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&request[..count]);
        let route = request.split_whitespace().nth(1).unwrap_or("");
        let command = match route { "/panic" => 1, "/exit" => 2, _ => 0 };
        let body = format!("instance={} workers={} parked={} ticks0={} ticks1={} command={}\n",
            state.instance, state.started.load(Ordering::Acquire),
            state.parked.load(Ordering::Acquire),
            state.ticks[0].load(Ordering::Relaxed), state.ticks[1].load(Ordering::Relaxed),
            match command { 1 => "panic", 2 => "exit", _ => "status" });
        let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        if socket.write_all(response.as_bytes()).await.is_err() { continue; }
        let _ = socket.shutdown().await;
        if command == 1 { let _ = state.native_control.send(true); }
        if command == 2 {
            let _ = state.native_control.send(false);
            state.done.store(true, Ordering::Release);
            return;
        }
    }
}

fn run(peer: bool) -> Result<(), String> {
    let port = if peer { 32380 } else { 32379 };
    #[cfg(target_os = "trueos")]
    let instance = trueos::replication::current_identity()
        .ok_or("instance-identity")?.instance_guid();
    #[cfg(not(target_os = "trueos"))]
    let instance = format!("host-{}", std::process::id());
    record("BEGIN", format_args!("instance={instance} port={port} peer={}", u8::from(peer)));
    // Do every ordinary filesystem operation before retaining the root write lease.
    #[cfg(target_os = "trueos")]
    if !peer {
        std::fs::create_dir_all(DIRECTORY).map_err(|error| format!("mkdir {error}"))?;
        let committed = format!("vm_recovery committed instance={instance}\n");
        std::fs::write(COMMITTED, committed.as_bytes()).map_err(|error| format!("write {error}"))?;
        if std::fs::read(COMMITTED).map_err(|error| format!("read {error}"))? != committed.as_bytes() {
            return Err("committed-readback".into());
        }
    }
    #[cfg(target_os = "trueos")]
    let graphics = Graphics::open(&instance)?;
    let (native_control, native_commands) = std::sync::mpsc::channel();
    let state = Arc::new(State {
        instance, started: AtomicUsize::new(0),
        ticks: [const { AtomicU64::new(0) }; 2], parked: AtomicBool::new(false),
        native_control, done: AtomicBool::new(false),
    });
    let first = runtime(&state, 0)?;
    let second = runtime(&state, 1)?;
    let listener = first.block_on(tokio::net::TcpListener::bind(("0.0.0.0", port)))
        .map_err(|error| format!("tcp-bind port={port} error={error}"))?;
    first.spawn(serve(listener, state.clone()));
    let worker_state = state.clone();
    let native = std::thread::Builder::new().name("vm-recovery-native".into()).spawn(move || {
        worker_state.parked.store(true, Ordering::Release);
        if matches!(native_commands.recv(), Ok(true)) {
            // The channel and log calls finish before panic; no host lock crosses this boundary.
            record("NATIVE-PANIC", format_args!("instance={} locks=none", worker_state.instance));
            std::thread::sleep(Duration::from_millis(100));
            panic!("vm_recovery explicit native terminal instance={}", worker_state.instance);
        }
    }).map_err(|error| format!("native-spawn {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while state.started.load(Ordering::Acquire) != 4
        || !state.parked.load(Ordering::Acquire)
        || state.ticks.iter().any(|ticks| ticks.load(Ordering::Relaxed) == 0) {
        if Instant::now() >= deadline { return Err("worker-ready-timeout".into()); }
        std::thread::sleep(Duration::from_millis(1));
    }
    #[cfg(target_os = "trueos")]
    let stream = if peer { None } else { Some(Stream::begin(&state.instance)?) };
    #[cfg(target_os = "trueos")]
    if let Some(stream) = stream.as_ref() {
        record("STREAM", format_args!("instance={} handle={} declared=4096 incomplete=1", state.instance, stream.0));
    }
    record("READY", format_args!("instance={} port={port} peer={} runtimes=2 workers=4 native_parked=1 fs_stream={} ui4={}",
        state.instance, u8::from(peer), u8::from(cfg!(target_os = "trueos") && !peer),
        u8::from(cfg!(target_os = "trueos"))));
    // Intentionally ignore host stop requests: kill must wake and retire these frames.
    while !state.done.load(Ordering::Acquire) { std::thread::sleep(Duration::from_secs(1)); }
    #[cfg(target_os = "trueos")]
    drop(stream);
    native.join().map_err(|_| "native-join")?;
    first.shutdown_timeout(Duration::from_secs(1));
    second.shutdown_timeout(Duration::from_secs(1));
    #[cfg(target_os = "trueos")]
    drop(graphics);
    record("DONE", format_args!("instance={}", state.instance));
    Ok(())
}

pub fn run_probe(peer: bool) {
    if let Err(error) = run(peer) { record("FAIL", error); }
}
