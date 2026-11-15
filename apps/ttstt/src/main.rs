use std::io::{Seek, Write};
use std::sync::{Arc, Mutex};
use ttstt::ttstt_service::{Engine, TtsRequest};

fn run() -> Result<(), String> {
    let mut text = None;
    let mut wav: Option<std::path::PathBuf> = None;
    let mut voice = "af_heart".to_string();
    let mut speed = 1.0;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "serve" => {}
            "--text" => text = Some(args.next().ok_or("missing text")?),
            "--wav" => {
                wav = Some(std::path::PathBuf::from(
                    args.next().ok_or("missing WAV filename")?,
                ))
            }
            "--voice" => voice = args.next().ok_or("missing voice")?,
            "--speed" => {
                speed = args
                    .next()
                    .ok_or("missing speed")?
                    .parse::<f32>()
                    .map_err(|e| e.to_string())?
            }
            "--licenses" => {
                println!("{}", ttstt::bundle::LICENSES);
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "ttstt [serve]\nttstt --text TEXT --wav FILE [--speed 1.0]\nttstt --licenses\nEnglish + Heart TTS, all model assets embedded. No setup or downloads. STT is unavailable."
                );
                return Ok(());
            }
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let Some(text) = text else {
        if wav.is_some() {
            return Err("--wav requires --text".into());
        }
        return ttstt::http::serve();
    };
    let request = TtsRequest { text, voice, speed };
    request.validate()?;
    let mut engine = Engine::load()?;
    if let Some(path) = wav {
        let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        let state = Arc::new(Mutex::new((file, 0u32)));
        state
            .lock()
            .unwrap()
            .0
            .write_all(&ttstt::playback::wav_header(0)?)
            .map_err(|e| e.to_string())?;
        let output = state.clone();
        engine.synthesize(
            request,
            Box::new(move |samples| {
                let mut out = output.lock().unwrap();
                out.1 = out
                    .1
                    .checked_add((samples.len() / 2) as u32)
                    .ok_or("wav-too-large")?;
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                out.0.write_all(&bytes).map_err(|_| "wav-write-failed")
            }),
        )?;
        let mut out = state.lock().unwrap();
        let header = ttstt::playback::wav_header(out.1)?;
        out.0.rewind().map_err(|e| e.to_string())?;
        out.0.write_all(&header).map_err(|e| e.to_string())?;
        out.0.flush().map_err(|e| e.to_string())?;
    } else {
        let playback = ttstt::playback::Playback::open()?;
        engine.synthesize(request, playback.sink())?;
        playback.handoff()?;
    }
    Ok(())
}
fn main() {
    ttstt::log_info!(target: "ttstt"; "ttstt: optimizations retained_conv={} prepared_weights={} blocked_gemm={}", cfg!(feature = "retained-convolution") as u8, cfg!(feature = "prepared-weights") as u8, cfg!(feature = "blocked-gemm") as u8);
    ttstt::log_info!(target: "ttstt"; "ttstt: startup phase=entry bundle_bytes={} language=en voice=af_heart", ttstt::bundle::TOTAL_BYTES);
    if let Err(e) = run() {
        ttstt::log_warn!(target: "ttstt"; "ttstt: startup-or-request failed error={e}");
        std::process::exit(1);
    }
}
