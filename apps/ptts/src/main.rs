mod bundle;
mod engine;
mod frontend;
mod http;
mod playback;
mod tone;

use anyhow::{Result, bail};
use std::time::Instant;

fn notice(message: impl AsRef<str>) {
    let text = message.as_ref();
    #[cfg(target_os = "trueos")]
    {
        trueos::logl::log(trueos::logl::level::INFO, text);
        let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "blueprint/ptts", text);
    }
    #[cfg(not(target_os = "trueos"))]
    eprintln!("{text}");
}

fn run() -> Result<()> {
    let mut label = None;
    let mut wav = None;
    let mut text = None;
    let mut serve = false;
    let mut sample = false;
    let mut port = http::PORT;
    let mut wav_dir = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "serve" | "--serve" => serve = true,
            "--sample" => sample = true,
            "--text" => {
                text = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--text needs text"))?,
                )
            }
            "--port" => {
                port = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--port needs a number"))?
                    .parse::<u16>()?
            }
            "--wav-dir" => {
                wav_dir =
                    Some(std::path::PathBuf::from(args.next().ok_or_else(|| {
                        anyhow::anyhow!("--wav-dir needs a directory")
                    })?))
            }
            "--label" => {
                label = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--label needs a Lily label"))?,
                )
            }
            "--wav" => {
                wav = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("--wav needs a path"))?,
                )
            }
            "--rules" => {
                println!("{}", tone::RULES_JSON);
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "ptts [serve] [--port 9773]\nptts --text TEXT --wav FILE [--label LABEL] (Ubuntu)\nptts --serve --wav-dir DIR (Ubuntu)\nptts --sample [--label LABEL] (TRUEOS demo)\nptts --rules\nEnglish/Azelma CPU speech sidekick; runtime tokenization and synthesis."
                );
                return Ok(());
            }
            _ => bail!("unknown argument {arg:?}"),
        }
    }
    // This entry point has not started inference or workers yet.
    unsafe {
        std::env::set_var("XN_THREADPOOL", "1");
    }
    let direct = sample || text.is_some() || wav.is_some() || label.is_some();
    if serve || !direct {
        if direct {
            bail!("serve mode cannot be combined with --text, --sample, --wav or --label")
        }
        if port == 0 {
            bail!("service port must be nonzero")
        }
        #[cfg(target_os = "trueos")]
        if wav_dir.is_some() {
            bail!("--wav-dir is host-only")
        }
        return http::serve(port, wav_dir);
    }
    #[cfg(target_os = "trueos")]
    if wav.is_some() {
        bail!("native ptts submits its generated clip to the audio sink; --wav is host-only")
    }
    #[cfg(not(target_os = "trueos"))]
    if wav.is_none() {
        bail!("host demo needs --wav FILE")
    }

    let rules = tone::ToneRules::embedded().map_err(anyhow::Error::msg)?;
    let temperature = rules.temperature_for(label.as_deref());
    let started = Instant::now();
    notice(format!(
        "ptts: loading model=english_2026-10 voice=azelma backend=xn-cpu threads=1 assets_bytes={}",
        bundle::model().len() + bundle::voice().len()
    ));
    let engine = engine::Engine::load()?;
    notice(format!(
        "ptts: ready load_ms={} temperature={temperature} label={} tokens={}",
        started.elapsed().as_millis(),
        label.as_deref().unwrap_or("Static"),
        bundle::SAMPLE_TOKENS.len()
    ));
    let text = text.as_deref().unwrap_or(bundle::SAMPLE_TEXT);
    let frontend = frontend::Frontend::from_bytes(bundle::tokenizer())?;
    let chunks = frontend.prepare(text)?;
    let mut pcm = Vec::new();
    let mut first = true;
    let mut frames = 0;
    let synthesis_started = Instant::now();
    let deadline = synthesis_started + std::time::Duration::from_secs(180);
    for chunk in chunks {
        let stats = engine.generate_with_limits(
            &chunk.tokens,
            temperature,
            chunk.frames_after_eos,
            deadline,
            &mut |samples| {
                if pcm.len() + samples.len() > 24_000 * 120 {
                    bail!("generated clip exceeds 120 seconds")
                }
                if first {
                    notice(format!(
                        "ptts: first_pcm elapsed_ms={}",
                        started.elapsed().as_millis()
                    ));
                    first = false;
                }
                pcm.extend_from_slice(samples);
                Ok(())
            },
        )?;
        frames += stats.frames;
    }
    notice(format!(
        "ptts: generated samples={} rate=24000 frames={frames} generation_ms={} total_ms={}",
        pcm.len(),
        synthesis_started.elapsed().as_millis(),
        started.elapsed().as_millis()
    ));
    if pcm.is_empty() {
        notice("ptts: silent emoji-only text");
        return Ok(());
    }
    #[cfg(not(target_os = "trueos"))]
    playback::write_wav(wav.as_deref().unwrap(), &pcm)?;
    #[cfg(target_os = "trueos")]
    playback::submit(&pcm)?;
    notice(format!(
        "ptts: done elapsed_ms={}",
        started.elapsed().as_millis()
    ));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        notice(format!("ptts: failed {error:#}"));
        std::process::exit(1);
    }
}
