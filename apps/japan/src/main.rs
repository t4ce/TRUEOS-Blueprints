#![no_std]

extern crate alloc;

use trueos::{
    clock, env,
    logl::{self, level},
    rng, vshell,
};

const CADENCE: tokio::time::Duration = tokio::time::Duration::from_secs(1);
const HEALTH_CADENCE: tokio::time::Duration = tokio::time::Duration::from_secs(10);

#[derive(Clone, Copy)]
enum TokioSpawnedTaskKind {
    Rng,
    Time,
    Env,
}

#[derive(Default)]
struct Health {
    rng: u64,
    time: u64,
    env: u64,
}

impl Health {
    fn record(&mut self, tokio_spawned_task_kind: TokioSpawnedTaskKind) {
        let counter = match tokio_spawned_task_kind {
            TokioSpawnedTaskKind::Rng => &mut self.rng,
            TokioSpawnedTaskKind::Time => &mut self.time,
            TokioSpawnedTaskKind::Env => &mut self.env,
        };
        *counter = counter.saturating_add(1);
    }

    const fn all_passed(&self) -> bool {
        self.rng > 0 && self.time > 0 && self.env > 0
    }

    fn clear(&mut self) {
        *self = Self::default();
    }
}

fn main() {
    // The current-thread Tokio scheduler drives all three Tokio spawned tasks
    // from the existing TRUEOS application fore. It does not launch Tokio
    // workers through `pthread_create` and the TRUEOS service lanes.
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_time();
    let runtime = match builder.build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("japan: current-thread Tokio scheduler failed: {error}"),
            );
            return;
        }
    };

    logl::log(
        level::INFO,
        format_args!("japan: Tokio scheduler=current-thread Tokio spawned tasks=3"),
    );
    runtime.block_on(run());
}

async fn run() {
    let (health_tx, mut health_rx) = tokio::sync::mpsc::channel(12);

    tokio::spawn(rng_tokio_spawned_task(health_tx.clone()));
    tokio::spawn(time_tokio_spawned_task(health_tx.clone()));
    tokio::spawn(env_tokio_spawned_task(health_tx));

    let mut health = Health::default();
    let mut health_tick = tokio::time::interval(HEALTH_CADENCE);
    health_tick.tick().await;

    loop {
        tokio::select! {
            Some(tokio_spawned_task_kind) = health_rx.recv() => {
                health.record(tokio_spawned_task_kind);
            }
            _ = health_tick.tick() => {
                if health.all_passed() {
                    logl::log(
                        level::INFO,
                        format_args!(
                            "japan: Tokio spawned tasks passed rng={} time={} env={}",
                            health.rng, health.time, health.env,
                        ),
                    );
                } else {
                    logl::log(
                        level::WARN,
                        format_args!(
                            "japan: Tokio spawned task heartbeat missing rng={} time={} env={}",
                            health.rng, health.time, health.env,
                        ),
                    );
                }
                health.clear();
            }
        }
    }
}

async fn rng_tokio_spawned_task(health: tokio::sync::mpsc::Sender<TokioSpawnedTaskKind>) {
    loop {
        tokio::time::sleep(CADENCE).await;
        shell_line(
            format_args!("rng  | {}", rng::u64()),
            vshell::Rgb::new(244, 114, 182),
        );
        if health.send(TokioSpawnedTaskKind::Rng).await.is_err() {
            return;
        }
    }
}

async fn time_tokio_spawned_task(health: tokio::sync::mpsc::Sender<TokioSpawnedTaskKind>) {
    loop {
        tokio::time::sleep(CADENCE).await;
        match clock::utc_date_time() {
            Some(now) => shell_line(format_args!("time | {now}"), vshell::Rgb::new(96, 165, 250)),
            None => shell_line(
                format_args!("time | monotonic={} ms", clock::monotonic_millis()),
                vshell::Rgb::new(96, 165, 250),
            ),
        };
        if health.send(TokioSpawnedTaskKind::Time).await.is_err() {
            return;
        }
    }
}

async fn env_tokio_spawned_task(health: tokio::sync::mpsc::Sender<TokioSpawnedTaskKind>) {
    loop {
        tokio::time::sleep(CADENCE).await;
        match env::var("TRUEOS_APP_ARCHIVE") {
            Ok(archive) => shell_line(
                format_args!("env  | TRUEOS_APP_ARCHIVE={archive}"),
                vshell::Rgb::new(52, 211, 153),
            ),
            Err(_) => shell_line(
                format_args!("env  | TRUEOS_APP_ARCHIVE unavailable"),
                vshell::Rgb::new(245, 158, 11),
            ),
        };
        if health.send(TokioSpawnedTaskKind::Env).await.is_err() {
            return;
        }
    }
}

fn shell_line(args: core::fmt::Arguments<'_>, color: vshell::Rgb) {
    let text = alloc::format!("{args}");
    let line = alloc::format!("{}\r\n", vshell::style(text.as_str()).fg(color));
    vshell::attached_write(line.as_bytes());
}
