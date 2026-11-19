mod audio;
mod mcp;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        None => ui::run(false),
        Some("--mcp") => ui::run(true),
        Some("--self-check") => audio::self_check(),
        Some("--help" | "-h") => {
            println!(
                "lico [--mcp | --self-check]\nLive-code audio in the terminal with Alt+R (Run) and Alt+S (Stop). Use --mcp for a session connection command."
            );
            Ok(())
        }
        Some(arg) => Err(format!("unknown argument: {arg}").into()),
    }
}
