use crate::{Message, cli};
use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
use tracing::{debug, error, warn};

pub struct Console {
    pub msg_r: mpsc::Receiver<Message>,
    running: Arc<AtomicBool>,
}

impl Console {
    pub fn run() -> Self {
        let (msg_s, msg_r) = mpsc::channel();
        let running = Arc::new(AtomicBool::new(true));
        let running2 = Arc::clone(&running);

        std::thread::Builder::new()
            .name("server-console".to_owned())
            .spawn(|| Self::read_commands(running2, msg_s))
            .unwrap();

        Self { msg_r, running }
    }

    fn read_commands(running: Arc<AtomicBool>, mut msg_s: mpsc::Sender<Message>) {
        while running.load(Ordering::Relaxed) {
            let mut line = String::new();

            match io::stdin().read_line(&mut line) {
                Err(e) => {
                    error!(
                        ?e,
                        "couldn't read from stdin, console commands are disabled now!"
                    );
                    break;
                },
                Ok(0) => {
                    warn!("EOF received, console commands are disabled now!");
                    break;
                },
                Ok(_) => {
                    debug!(?line, "console command entered");
                    cli::parse_command(line.trim(), &mut msg_s);
                },
            }
        }
    }
}

impl Drop for Console {
    fn drop(&mut self) { self.running.store(false, Ordering::Relaxed); }
}
