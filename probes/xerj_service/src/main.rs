//! Persistent entry-thread owner for XERJ. See ../BUILDING.txt.

#[cfg(target_os = "trueos")]
mod native {
    use std::io::{self, Write};
    use std::time::Duration;
    use trueos::child_hull as child;

    // A fixed launch request allows Shell3's bare app name to select the
    // persistent data directory and disable feedback without shell arguments.
    const REQUEST: &[u8] = br#"{
        "program": "xerj",
        "args": [
            "--bind", "127.0.0.1",
            "--port", "9200",
            "--data-dir", "/apps/xerj/data",
            "--disable-feedback"
        ],
        "env": { "XERJ_DISABLE_FEEDBACK": "true" },
        "cwd": "/apps/xerj"
    }"#;

    struct OwnedChild(u64);

    impl OwnedChild {
        fn spawn() -> io::Result<Self> {
            child::spawn_program(REQUEST).map(Self).map_err(|code| {
                io::Error::other(format!("cannot launch xerj: child Hull API {code}"))
            })
        }

        fn drain(&self, output: &mut impl Write) -> io::Result<()> {
            let mut buffer = [0u8; 4096];
            // Bound each drain so even continuously logging children cannot
            // prevent lifecycle checks. Forward bytes without text conversion.
            for _ in 0..32 {
                let size = child::read(self.0, &mut buffer).map_err(|code| {
                    io::Error::other(format!("xerj stdout read failed: child Hull API {code}"))
                })?;
                if size == 0 {
                    break;
                }
                if size > buffer.len() {
                    return Err(io::Error::other(
                        "xerj returned an invalid stdout read size",
                    ));
                }
                output.write_all(&buffer[..size])?;
            }
            output.flush()
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            match child::terminate(self.0) {
                Ok(()) | Err(child::ERR_NOT_FOUND) => {}
                Err(code) => eprintln!("xerj-service could not stop its child Hull: {code}"),
            }
        }
    }

    pub(super) fn run() -> io::Result<()> {
        std::fs::create_dir_all("/apps/xerj")?;
        // Spawn, read, status and cleanup always execute on the original Hull
        // entry thread. This launcher creates no runtime or worker threads.
        let engine = OwnedChild::spawn()?;
        let mut output = io::stdout().lock();
        loop {
            engine.drain(&mut output)?;
            match child::status(engine.0) {
                child::STATUS_STARTING | child::STATUS_RUNNING | child::STATUS_STOPPING => {}
                child::STATUS_EXITED => {
                    // One further bounded read preserves queued final output.
                    engine.drain(&mut output)?;
                    return Err(io::Error::other("xerj child Hull exited unexpectedly"));
                }
                code => {
                    return Err(io::Error::other(format!(
                        "xerj child Hull lifecycle failed: {code}"
                    )));
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[cfg(not(target_os = "trueos"))]
fn run() -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "xerj-service requires TRUEOS child Hulls; run xerj-service.bp on TRUEOS",
    ))
}

#[cfg(target_os = "trueos")]
use native::run;

fn main() {
    if let Err(error) = run() {
        eprintln!("xerj-service: {error}");
        std::process::exit(1);
    }
}
