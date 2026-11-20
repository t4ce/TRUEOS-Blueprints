mod audio;
mod examples;
mod mcp;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        None => ui::run(),
        Some("--self-check") => audio::self_check(),
        Some("--help" | "-h") => {
            println!(
                "lico [--self-check]\nLive-code audio with Alt+R (Run), Alt+S (Stop), and Alt+E (Examples). Codex tools are available over the local TRUEOS connection."
            );
            Ok(())
        }
        Some(arg) => Err(format!("unknown argument: {arg}").into()),
    }
}
