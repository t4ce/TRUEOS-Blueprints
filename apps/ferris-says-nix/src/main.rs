use ferris_says::say;
use std::io::{BufWriter, Write, stdout};

const MESSAGE: &str = "Hello from a GitHub package built as a TRUEOS Blueprint!";

fn main() {
    let stdout = stdout();
    let mut writer = BufWriter::new(stdout.lock());

    if let Err(error) = say(MESSAGE, 48, &mut writer).and_then(|()| writer.flush()) {
        eprintln!("ferris-says-nix: output failed: {error}");
    }
}
