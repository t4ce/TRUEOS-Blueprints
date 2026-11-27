mod audio;
mod audit;
#[cfg(any(test, target_os = "trueos", target_os = "zkvm"))]
mod dance_graph;
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod dance_publisher;
mod drum;
mod examples;
mod highlight;
mod mcp;
mod midi;
mod piano_udp;
mod playhead;
mod ui;
mod visualizer;
#[cfg(any(test, target_os = "trueos", target_os = "zkvm"))]
mod visualizer_data;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => ui::run(midi::Input::default()),
        Some("--midi-piano") => {
            if args.next().is_some() {
                return Err("usage: lico --midi-piano".into());
            }
            ui::run(midi::Input::open(midi::Source::Wifi)?)
        }
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
                "lico [--self-check | --midi-piano]\nlico --render PATCH OUTPUT.wav [SECONDS]\nAlt+N opens MIDI input selection. Alt+E opens examples.\nAlt+R runs the patch and Alt+S stops it."
            );
            Ok(())
        }
        Some(arg) => Err(format!("unknown argument: {arg}").into()),
    }
}
