//! Ubuntu-only full-model experiment. Args: workers coordinator cpus output_dir.
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
use trueos_kokoro_dispatch::async_conv::{self, Pool};
use ttstt::ttstt_service::{Engine, TtsRequest};

fn request(text: String) -> TtsRequest {
    TtsRequest {
        text,
        voice: "af_heart".into(),
        speed: 1.0,
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let workers: usize = args.get(1).ok_or("workers")?.parse()?;
    let coordinator: usize = args.get(2).ok_or("coordinator CPU")?.parse()?;
    let cpus: Vec<usize> = args
        .get(3)
        .ok_or("compute CPUs")?
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    let output = PathBuf::from(args.get(4).ok_or("output directory")?);
    fs::create_dir_all(&output)?;
    if !async_conv::pin_current(coordinator) {
        return Err("coordinator affinity failed".into());
    }
    if cpus.iter().any(|cpu| *cpu == coordinator) {
        return Err("compute CPU overlaps coordinator".into());
    }
    #[cfg(feature = "occupancy-trace")]
    trueos_kokoro_dispatch::occupancy::configure(coordinator, &cpus);
    let pool = if workers != 0 {
        if cpus.len() != workers {
            return Err("CPU count must equal workers".into());
        }
        let p = Pool::new(&cpus)?;
        async_conv::install(p.clone())?;
        Some(p)
    } else {
        None
    };
    let mut engine = Engine::load()?;
    // Warm every immutable convolution pack before the measured corpus.
    smol::block_on(
        engine.synthesize_async(request("Hello from TrueOS.".into()), Box::new(|_| Ok(()))),
    )?;
    if let Some(pool) = &pool {
        let c = &pool.counters;
        for value in [
            &c.operations,
            &c.tiles,
            &c.worker_us,
            &c.wait_us,
            &c.snapshot_bytes,
            &c.max_tile_us,
        ] {
            value.store(0, Ordering::Relaxed);
        }
    }
    let cases: Vec<(String, String)> =
        serde_json::from_str(include_str!("../tools/perf_texts.json"))?;
    let finished = Arc::new(AtomicBool::new(false));
    let flag = finished.clone();
    let heartbeat = async move {
        let mut ticks = 0u64;
        let mut max_gap_us = 0u128;
        let mut last = Instant::now();
        while !flag.load(Ordering::Acquire) {
            smol::Timer::after(std::time::Duration::from_millis(2)).await;
            max_gap_us = max_gap_us.max(last.elapsed().as_micros());
            last = Instant::now();
            ticks += 1;
        }
        (ticks, max_gap_us)
    };
    let synthesis = async {
        let result = async {
            let mut rows = Vec::new();
            for (name,text) in cases {
                let samples = Arc::new(Mutex::new(Vec::<i16>::new())); let sink = samples.clone();
                #[cfg(feature = "occupancy-trace")]
                let trace_start = trueos_kokoro_dispatch::occupancy::begin_case();
                let started = Instant::now();
                engine.synthesize_async(request(text), Box::new(move |pcm| {
                    sink.lock().unwrap().extend_from_slice(pcm); Ok(())
                })).await?;
                let wall_ms = started.elapsed().as_secs_f64() * 1000.0;
                #[cfg(feature = "occupancy-trace")]
                let trace = trueos_kokoro_dispatch::occupancy::finish_case(&name, trace_start);
                let pcm = samples.lock().unwrap(); let frames = pcm.len()/2;
                let wav = output.join(std::format!("{name}.wav"));
                let mut file = fs::File::create(&wav).map_err(|e|e.to_string())?;
                file.write_all(&ttstt::playback::wav_header(frames as u32)?).map_err(|e|e.to_string())?;
                let bytes: Vec<u8> = pcm.iter().flat_map(|s|s.to_le_bytes()).collect();
                file.write_all(&bytes).map_err(|e|e.to_string())?;
                #[cfg(feature = "occupancy-trace")]
                if let Some(trace) = trace {
                    let folder = output.join("trace"); fs::create_dir_all(&folder).map_err(|e|e.to_string())?;
                    fs::write(folder.join(std::format!("{name}.json")), serde_json::to_vec(&trace).map_err(|e|e.to_string())?)
                        .map_err(|e|e.to_string())?;
                }
                rows.push(serde_json::json!({"case":name,"synthesis_ms":wall_ms,"audio_ms":frames as f64/48.0,"wav":wav}));
            }
            Ok::<_,String>(rows)
        }.await;
        finished.store(true, Ordering::Release);
        result
    };
    let (rows, (ticks, gap)) = smol::block_on(smol::future::zip(synthesis, heartbeat));
    let rows = rows?;
    let counters = pool.as_ref().map(|p| &p.counters);
    let result = serde_json::json!({"workers":workers,"coordinator_cpu":coordinator,"compute_cpus":cpus,
        "cases":rows,"heartbeat_ticks":ticks,"coordinator_max_gap_us":gap,
        "operations":counters.map(|c|c.operations.load(Ordering::Relaxed)).unwrap_or(0),
        "tiles":counters.map(|c|c.tiles.load(Ordering::Relaxed)).unwrap_or(0),
        "worker_elapsed_sum_us":counters.map(|c|c.worker_us.load(Ordering::Relaxed)).unwrap_or(0),
        "max_worker_tile_us":counters.map(|c|c.max_tile_us.load(Ordering::Relaxed)).unwrap_or(0),
        "await_elapsed_us":counters.map(|c|c.wait_us.load(Ordering::Relaxed)).unwrap_or(0),
        "activation_snapshot_bytes":counters.map(|c|c.snapshot_bytes.load(Ordering::Relaxed)).unwrap_or(0)});
    fs::write(
        output.join("result.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
