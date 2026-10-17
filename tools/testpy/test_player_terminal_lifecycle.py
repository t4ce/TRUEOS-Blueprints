#!/usr/bin/env python3
"""Exercise Player's production terminal lifecycle with a simulated TRUEOS lease."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / 'apps/Player/src/ui.rs').read_text()
run = source[source.index('pub fn run(config: UiConfig)'):source.index('/// Initial state and demo data')]
harness = r'''
extern crate self as trueos;
use std::{io, sync::Mutex};
type Result<T> = io::Result<T>;
#[derive(Debug, PartialEq, Clone, Copy)] pub enum UiExit { ReturnToShell, Terminate }
pub struct UiConfig;
static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static FAILURE: Mutex<&str> = Mutex::new("");
fn event(name: &'static str) -> Result<()> {
    EVENTS.lock().unwrap().push(name);
    if *FAILURE.lock().unwrap() == name { Err(io::Error::other(name)) } else { Ok(()) }
}
pub mod vshell {
    use super::*;
    pub struct Lease;
    pub struct Ticket;
    pub fn terminal_initial_lease() -> Result<Lease> { event("claim")?; Ok(Lease) }
    impl Lease {
        pub fn acknowledge_ready(&self) -> Result<()> { event("ack") }
        pub fn release_to_shell(self) -> Result<Ticket> { event("release")?; Ok(Ticket) }
    }
    impl Ticket { pub fn wait_for_reentry(self) -> Result<Lease> { event("reentry")?; Ok(Lease) } }
}
struct Terminal;
impl Terminal {
    fn draw(&mut self, draw: impl FnOnce(&mut ())) -> Result<()> { event("draw")?; draw(&mut ()); Ok(()) }
}
fn setup_terminal() -> Result<Terminal> { event("setup")?; Ok(Terminal) }
fn restore_terminal(_: &mut Terminal) -> Result<()> { event("restore") }
struct App { exit: Option<UiExit>, visits: usize }
impl App {
    fn new(_: UiConfig) -> Self { Self { exit: None, visits: 0 } }
    fn draw(&mut self, _: &mut ()) {}
    fn run(&mut self, _: &mut Terminal) -> Result<UiExit> {
        event("interact")?;
        assert_eq!(self.exit, None, "exit must be reset on reentry");
        self.visits += 1;
        let exit = if self.visits == 1 { UiExit::ReturnToShell } else { UiExit::Terminate };
        self.exit = Some(exit); Ok(exit)
    }
}
__RUN__
fn main() {
    assert_eq!(run(UiConfig).unwrap(), UiExit::Terminate);
    assert_eq!(*EVENTS.lock().unwrap(), ["claim", "setup", "draw", "ack", "interact", "restore", "release", "reentry", "setup", "draw", "ack", "interact", "restore", "release"]);
    for (failure, expected) in [
        ("claim", vec!["claim"]),
        ("setup", vec!["claim", "setup", "release"]),
        ("draw", vec!["claim", "setup", "draw", "restore", "release"]),
        ("ack", vec!["claim", "setup", "draw", "ack", "restore", "release"]),
        ("interact", vec!["claim", "setup", "draw", "ack", "interact", "restore", "release"]),
        ("restore", vec!["claim", "setup", "draw", "ack", "interact", "restore", "release"]),
        ("reentry", vec!["claim", "setup", "draw", "ack", "interact", "restore", "release", "reentry"]),
    ] {
        EVENTS.lock().unwrap().clear(); *FAILURE.lock().unwrap() = failure;
        assert!(run(UiConfig).is_err(), "{failure}");
        assert_eq!(*EVENTS.lock().unwrap(), expected, "{failure}");
    }
}
'''
with tempfile.TemporaryDirectory(prefix='player-terminal-') as tmp:
    path = Path(tmp)
    (path / 'test.rs').write_text(harness.replace('__RUN__', run))
    subprocess.run(['rustc', '--edition=2024', '--cfg', 'target_os="trueos"', '-Aexplicit_builtin_cfgs_in_flags', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test')], check=True)
print('Player terminal lifecycle: reentry, retained state, cleanup and failure cases passed')
