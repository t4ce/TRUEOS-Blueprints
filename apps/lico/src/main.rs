mod audio;
mod examples;
mod highlight;
mod mcp;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => ui::run(),
        Some("--self-check") => audio::self_check(),
        Some("--render") => {
            let patch = args.next().ok_or("--render requires a patch file")?;
            let wav = args.next().ok_or("--render requires an output WAV file")?;
            let seconds: u32 = args.next().map_or(Ok(8), |value| value.parse())?;
            if seconds == 0 || args.next().is_some() {
                return Err("usage: lico --render PATCH OUTPUT.wav [SECONDS]".into());
            }
            audio::render_wav(&std::fs::read_to_string(patch)?, &wav, seconds)
        }
        Some("--help" | "-h") => {
            println!(
                "lico [--self-check]\nlico --render PATCH OUTPUT.wav [SECONDS]\nLive-code audio with Alt+R (Run), Alt+S (Stop), and Alt+E (Examples). Codex tools are available over the local TRUEOS connection."
            );
            Ok(())
        }
        Some(arg) => Err(format!("unknown argument: {arg}").into()),
    }
}
