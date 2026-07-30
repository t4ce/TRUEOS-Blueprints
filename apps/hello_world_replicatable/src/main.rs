#![no_std]

use core::net::Ipv4Addr;

use trueos::{
    env,
    logl::{self, level},
    replication, t,
};

const DEFAULT_PREFERRED_PORT: u16 = 48_080;
const MAX_INCREMENT_ATTEMPTS: u16 = 32;
const ACCEPT_RETRY_DELAY_MS: u64 = 100;
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u8 = 8;
const LIFECYCLE_POLL_MS: u64 = 100;
const CHECKPOINT_VERSION: u64 = 1;
const GREETING: &[u8] = b"hello from a replicatable TRUEOS Blueprint\n";

#[derive(Clone, Copy)]
enum PortPolicy {
    /// Ask TRUEOS to allocate an unused listener port.
    Ephemeral,
    /// Prefer a stable port, then step upward when another instance owns it.
    Increment { preferred: u16 },
}

#[derive(Clone, Copy)]
struct LogicalState {
    generation: u64,
    accepted_connections: u64,
}

fn main() {
    let policy = parse_policy();
    let runtime = match t::runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            logl::log(
                level::ERROR,
                format_args!("replicatable: runtime build failed: {}", err),
            );
            return;
        }
    };

    runtime.block_on(run(policy));
}

fn parse_policy() -> PortPolicy {
    let mut args = env::args();
    let _archive_name = args.next();

    match args.next().as_deref() {
        Some("auto") | Some("ephemeral") | Some("0") => PortPolicy::Ephemeral,
        Some("next") | Some("increment") => PortPolicy::Increment {
            preferred: args
                .next()
                .and_then(|value| value.parse::<u16>().ok())
                .unwrap_or(DEFAULT_PREFERRED_PORT),
        },
        Some(port) => PortPolicy::Increment {
            preferred: port.parse::<u16>().unwrap_or(DEFAULT_PREFERRED_PORT),
        },
        None => PortPolicy::Increment {
            preferred: DEFAULT_PREFERRED_PORT,
        },
    }
}

async fn run(policy: PortPolicy) {
    let mut state = LogicalState {
        generation: 0,
        accepted_connections: 0,
    };

    loop {
        if let Some(prepare) = replication::poll_prepare_pause() {
            resume_after_ready(prepare).await;
            continue;
        }
        let Some(listener) = acquire_listener(policy).await else {
            logl::log(
                level::ERROR,
                format_args!("replicatable: no listener port available"),
            );
            return;
        };
        let local = match listener.local_addr() {
            Ok(local) => local,
            Err(err) => {
                logl::log(
                    level::ERROR,
                    format_args!("replicatable: local_addr failed: {}", err),
                );
                return;
            }
        };

        state.generation = state.generation.saturating_add(1);
        logl::log(
            level::INFO,
            format_args!(
                "replicatable: ready generation={} listen={} accepted={}",
                state.generation, local, state.accepted_connections
            ),
        );

        match serve_until_boundary(listener, &mut state).await {
            ServeBoundary::PreparePause(prepare) => resume_after_ready(prepare).await,
            ServeBoundary::ListenerUnhealthy => logl::log(
                level::WARN,
                format_args!(
                    "replicatable: listener unhealthy; releasing and rebinding generation={}",
                    state.generation
                ),
            ),
        }
        t::time::sleep(t::time::Duration::from_millis(ACCEPT_RETRY_DELAY_MS)).await;
    }
}

async fn resume_after_ready(prepare: replication::PreparePause) {
    logl::log(
        level::INFO,
        format_args!(
            "replicatable: PreparePause operation={} reason={:?}; listener released, Ready",
            prepare.operation(),
            prepare.reason
        ),
    );
    match replication::ready(prepare, CHECKPOINT_VERSION) {
        Ok(resume) => logl::log(
            level::INFO,
            format_args!(
                "replicatable: Resume instance={} lineage={} generation={} clone={}",
                resume.instance_guid(),
                resume.lineage_guid(),
                resume.generation,
                resume.is_clone
            ),
        ),
        Err(error) => logl::log(
            level::WARN,
            format_args!("replicatable: Ready rejected: {:?}", error),
        ),
    }
}

async fn acquire_listener(policy: PortPolicy) -> Option<t::net::TcpListener> {
    match policy {
        PortPolicy::Ephemeral => {
            match t::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await {
                Ok(listener) => Some(listener),
                Err(err) => {
                    logl::log(
                        level::WARN,
                        format_args!("replicatable: ephemeral bind failed: {}", err),
                    );
                    None
                }
            }
        }
        PortPolicy::Increment { preferred } => {
            for offset in 0..MAX_INCREMENT_ATTEMPTS {
                let Some(port) = preferred.checked_add(offset) else {
                    break;
                };
                match t::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
                    Ok(listener) => {
                        if offset != 0 {
                            logl::log(
                                level::INFO,
                                format_args!(
                                    "replicatable: preferred port {} busy; rebound to {}",
                                    preferred, port
                                ),
                            );
                        }
                        return Some(listener);
                    }
                    Err(err) => logl::log(
                        level::WARN,
                        format_args!(
                            "replicatable: bind {} failed attempt={}/{} error={}",
                            port,
                            offset + 1,
                            MAX_INCREMENT_ATTEMPTS,
                            err
                        ),
                    ),
                }
            }
            None
        }
    }
}

enum ServeBoundary {
    PreparePause(replication::PreparePause),
    ListenerUnhealthy,
}

async fn serve_until_boundary(
    listener: t::net::TcpListener,
    state: &mut LogicalState,
) -> ServeBoundary {
    use t::io::AsyncWriteExt;

    let mut consecutive_errors = 0u8;
    loop {
        let accept = t::time::timeout(
            t::time::Duration::from_millis(LIFECYCLE_POLL_MS),
            listener.accept(),
        )
        .await;
        match accept {
            Ok(Ok((mut stream, peer))) => {
                consecutive_errors = 0;
                state.accepted_connections = state.accepted_connections.saturating_add(1);
                logl::log(
                    level::INFO,
                    format_args!(
                        "replicatable: accepted peer={} total={}",
                        peer, state.accepted_connections
                    ),
                );
                if let Err(err) = stream.write_all(GREETING).await {
                    logl::log(
                        level::WARN,
                        format_args!("replicatable: greeting write failed: {}", err),
                    );
                }
            }
            Ok(Err(err)) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                logl::log(
                    level::WARN,
                    format_args!(
                        "replicatable: accept failed consecutive={}/{} error={}",
                        consecutive_errors, MAX_CONSECUTIVE_ACCEPT_ERRORS, err
                    ),
                );
                if consecutive_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    return ServeBoundary::ListenerUnhealthy;
                }
                t::time::sleep(t::time::Duration::from_millis(ACCEPT_RETRY_DELAY_MS)).await;
            }
            Err(_) => {
                if let Some(prepare) = replication::poll_prepare_pause() {
                    // Returning drops the listener before Ready is acknowledged.
                    return ServeBoundary::PreparePause(prepare);
                }
            }
        }
    }
}
