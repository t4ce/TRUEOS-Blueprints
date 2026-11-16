//! Explicit one-shot rig measurement, leaving the normal argument-free service.
use crate::ttstt_service::{Engine, TtsRequest};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, atomic::Ordering},
    time::Instant,
};
use trueos_kokoro_dispatch::native_conv;

const BASE: &str = "apps/ttstt";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub nonce: String,
    #[serde(default = "repeats")]
    pub repeats: u8,
    #[serde(default)]
    pub cases: Vec<String>,
}
fn repeats() -> u8 {
    2
}
fn validate(request: &Request) -> Result<(), String> {
    if request.nonce.is_empty()
        || request.nonce.len() > 48
        || !request
            .nonce
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid benchmark nonce".into());
    }
    if !(1..=2).contains(&request.repeats) {
        return Err("benchmark repetitions must be one or two".into());
    }
    if request
        .cases
        .iter()
        .any(|s| !matches!(s.as_str(), "short" | "medium" | "long"))
    {
        return Err("unknown benchmark passage".into());
    }
    Ok(())
}
pub fn consume_request(engine: &mut Engine) -> Result<(), String> {
    let file = Path::new(BASE).join("benchmark.request.json");
    let bytes = match fs::read(&file) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if bytes.len() > 4096 {
        return Err("benchmark request too large".into());
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&request)?;
    fs::rename(
        &file,
        Path::new(BASE).join(format!("benchmark.{}.running.json", request.nonce)),
    )
    .map_err(|e| e.to_string())?;
    run(
        engine,
        &request,
        Path::new(BASE).join("benchmarks").join(&request.nonce),
    )
}
fn save(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub fn run(engine: &mut Engine, request: &Request, directory: PathBuf) -> Result<(), String> {
    validate(request)?;
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let pool = native_conv::pool().ok_or("native pool not installed")?;
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(pool) = native_conv::pool() {
                let _ = pool.set_workers(3);
            }
        }
    }
    let _restore = Restore;
    let outcome = (|| {
        pool.set_workers(3).map_err(String::from)?;
        engine.synthesize(
            TtsRequest {
                text: "Hello from TrueOS.".into(),
                voice: "af_heart".into(),
                speed: 1.0,
            },
            Box::new(|_| Ok(())),
        )?;
        let cases: Vec<(String, String)> =
            serde_json::from_str(include_str!("../tools/perf_texts.json"))
                .map_err(|e| e.to_string())?;
        let mut rows = Vec::new();
        for repeat in 0..request.repeats {
            let order = if repeat % 2 == 0 { [0, 3] } else { [3, 0] };
            for count in order {
                pool.set_workers(count).map_err(String::from)?;
                for (name, text) in &cases {
                    if !request.cases.is_empty() && !request.cases.contains(name) {
                        continue;
                    }
                    pool.reset_observations().map_err(String::from)?;
                    crate::diagnostics::record(
                        false,
                        format_args!(
                            "ttstt: native benchmark pass={repeat} workers={count} case={name} begin"
                        ),
                    );
                    let pcm = Arc::new(Mutex::new(Vec::<i16>::new()));
                    let sink = pcm.clone();
                    let started = Instant::now();
                    engine.synthesize(
                        TtsRequest {
                            text: text.clone(),
                            voice: "af_heart".into(),
                            speed: 1.0,
                        },
                        Box::new(move |samples| {
                            sink.lock().unwrap().extend_from_slice(samples);
                            Ok(())
                        }),
                    )?;
                    let elapsed_us = started.elapsed().as_micros();
                    let pcm = pcm.lock().unwrap();
                    let frames = pcm.len() / 2;
                    let filename = format!("pass-{repeat}-workers-{count}-{name}.wav");
                    let mut file =
                        fs::File::create(directory.join(&filename)).map_err(|e| e.to_string())?;
                    file.write_all(&crate::playback::wav_header(frames as u32)?)
                        .map_err(|e| e.to_string())?;
                    for chunk in pcm.chunks(8192) {
                        let bytes: Vec<u8> = chunk.iter().flat_map(|s| s.to_le_bytes()).collect();
                        file.write_all(&bytes).map_err(|e| e.to_string())?;
                    }
                    file.flush().map_err(|e| e.to_string())?;
                    let c = &pool.counters;
                    let placements:Vec<_>=pool.placements().iter().map(|p|serde_json::json!({"cpu_slot":p.cpu_slot,"core_kind":p.core_kind,"lane":p.lane})).collect();
                    let row = serde_json::json!({"repeat":repeat,"workers":count,"case":name,"text_bytes":text.len(),"elapsed_us":elapsed_us,"audio_ms":frames as u64*1000/48000,"frames":frames,"wav":filename,
                        "operations":c.operations.load(Ordering::Relaxed),"remote_jobs":c.remote_jobs.load(Ordering::Relaxed),"local_fallbacks":c.local_fallbacks.load(Ordering::Relaxed),
                        "peak_remote_workers":c.peak_remote.load(Ordering::Relaxed),"worker_us_sum":c.worker_us.load(Ordering::Relaxed),"await_us":c.wait_us.load(Ordering::Relaxed),
                        "shared_prepare_us":c.shared_prepare_us.load(Ordering::Relaxed),"snapshot_bytes":c.snapshot_bytes.load(Ordering::Relaxed),"placements":placements});
                    rows.push(row);
                    save(
                        &directory.join("progress.json"),
                        &serde_json::json!({"protocol":"ttstt-native-bench-v1","nonce":request.nonce,"complete":false,"rows":rows}),
                    )?;
                    crate::diagnostics::record(
                        false,
                        format_args!(
                            "ttstt: native benchmark pass={repeat} workers={count} case={name} elapsed_ms={} peak={} fallbacks={}",
                            elapsed_us / 1000,
                            c.peak_remote.load(Ordering::Relaxed),
                            c.local_fallbacks.load(Ordering::Relaxed)
                        ),
                    );
                }
            }
        }
        save(
            &directory.join("result.json"),
            &serde_json::json!({"protocol":"ttstt-native-bench-v1","nonce":request.nonce,"complete":true,"target":if cfg!(target_os="trueos"){"trueos"}else{std::env::consts::OS},"coordinator":format!("{:?}",native_conv::current_placement()),"rows":rows}),
        )?;
        Ok::<_, String>(())
    })();
    if let Err(error) = &outcome {
        let _ = save(
            &directory.join("error.json"),
            &serde_json::json!({"error":error}),
        );
    }
    outcome
}
