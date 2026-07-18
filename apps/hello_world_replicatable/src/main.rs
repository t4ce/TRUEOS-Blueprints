#![no_std]

use core::net::Ipv4Addr;

use trueos::{
    env,
    logl::{self, level},
    t,
};

const DEFAULT_PREFERRED_PORT: u16 = 48_080;
const MAX_INCREMENT_ATTEMPTS: u16 = 32;
const ACCEPT_RETRY_DELAY_MS: u64 = 100;
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u8 = 8;
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

        // A future F2 PreparePause hook belongs at this ownership boundary:
        // stop accepting, drop the listener, checkpoint only LogicalState, then
        // acknowledge quiescence. Resume/replicate calls acquire_listener again.
        serve_until_rebind(listener, &mut state).await;
        logl::log(
            level::WARN,
            format_args!(
                "replicatable: listener unhealthy; releasing and rebinding generation={}",
                state.generation
            ),
        );
        t::time::sleep(t::time::Duration::from_millis(ACCEPT_RETRY_DELAY_MS)).await;
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

async fn serve_until_rebind(listener: t::net::TcpListener, state: &mut LogicalState) {
    use t::io::AsyncWriteExt;

    let mut consecutive_errors = 0u8;
    loop {
        match listener.accept().await {
            Ok((mut stream, peer)) => {
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
            Err(err) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                logl::log(
                    level::WARN,
                    format_args!(
                        "replicatable: accept failed consecutive={}/{} error={}",
                        consecutive_errors, MAX_CONSECUTIVE_ACCEPT_ERRORS, err
                    ),
                );
                if consecutive_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                    return;
                }
                t::time::sleep(t::time::Duration::from_millis(ACCEPT_RETRY_DELAY_MS)).await;
            }
        }
    }
}
