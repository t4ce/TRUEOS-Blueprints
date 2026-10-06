use crate::{Client, ClientType, ServerInfo};
use common_net::msg::{ClientHello, GameVersionAnswer};
use crossbeam_channel::{Receiver, Sender, unbounded};
use futures_util::future::FutureExt;
use network::{Network, Participant, Promises, Stream};
use std::time::Duration;
use tokio::{runtime::Runtime, select, sync::oneshot};
use tracing::{debug, error, info, trace, warn};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Admit the numeric game version before creating a client or accepting any
/// registration credentials. A legacy raw ClientType fails closed on decode.
async fn receive_client_hello(
    register_stream: &mut Stream,
    server_version: u32,
) -> Result<Option<ClientType>, Box<dyn std::error::Error>> {
    let hello = match select!(
        _ = tokio::time::sleep(TIMEOUT).fuse() => None,
        hello = register_stream.recv::<ClientHello>().fuse() => Some(hello),
    ) {
        None => {
            debug!("Timeout for incoming client version elapsed, aborting connection");
            return Ok(None);
        },
        Some(hello) => hello?,
    };
    let answer: GameVersionAnswer = hello.check_version(server_version);
    register_stream.send(&answer)?;
    match answer {
        Ok(()) => Ok(Some(hello.client_type)),
        Err(mismatch) => {
            warn!(
                client = mismatch.client,
                server = mismatch.server,
                "Game version mismatch; rejecting client before registration"
            );
            Ok(None)
        },
    }
}

pub(crate) struct ServerInfoPacket {
    pub info: ServerInfo,
    pub time: f64,
}

// The request queue is drained synchronously by the game loop. Connection
// setup must await its reply without parking a Tokio worker. The connection
// handler initializes one participant at a time, so at most one request is
// live.
async fn request_server_info(
    sender: &Sender<oneshot::Sender<ServerInfoPacket>>,
) -> Result<ServerInfoPacket, Box<dyn std::error::Error>> {
    let (reply, response) = oneshot::channel();
    sender.send(reply)?;
    Ok(response.await?)
}

pub(crate) type IncomingClient = Client;

pub(crate) struct ConnectionHandler {
    /// We never actually use this, but if it's dropped before the network has a
    /// chance to exit, it won't block the main thread, and if it is dropped
    /// after the network thread ends, it will drop the network here (rather
    /// than delaying the network thread).  So it emulates the effects of
    /// storing the network in an Arc, without us losing mutability in the
    /// network thread.
    _network_receiver: oneshot::Receiver<Network>,
    thread_handle: Option<tokio::task::JoinHandle<()>>,
    pub client_receiver: Receiver<IncomingClient>,
    pub info_requester_receiver: Receiver<oneshot::Sender<ServerInfoPacket>>,
    stop_sender: Option<oneshot::Sender<()>>,
}

/// Instead of waiting the main loop we are handling connections, especially
/// their slow network .await part on a different thread. We need to communicate
/// to the Server main thread sometimes though to get the current server_info
/// and time
impl ConnectionHandler {
    pub fn new(network: Network, runtime: &Runtime) -> Self {
        let (stop_sender, stop_receiver) = oneshot::channel();
        let (network_sender, _network_receiver) = oneshot::channel();

        let (client_sender, client_receiver) = unbounded::<IncomingClient>();
        let (info_requester_sender, info_requester_receiver) =
            unbounded::<oneshot::Sender<ServerInfoPacket>>();

        let thread_handle = Some(runtime.spawn(Self::work(
            network,
            client_sender,
            info_requester_sender,
            stop_receiver,
            network_sender,
        )));

        Self {
            thread_handle,
            client_receiver,
            info_requester_receiver,
            stop_sender: Some(stop_sender),
            _network_receiver,
        }
    }

    async fn work(
        network: Network,
        client_sender: Sender<IncomingClient>,
        info_requester_sender: Sender<oneshot::Sender<ServerInfoPacket>>,
        stop_receiver: oneshot::Receiver<()>,
        network_sender: oneshot::Sender<Network>,
    ) {
        // Emulate the effects of storing the network in an Arc, without losing
        // mutability.
        let mut network_sender = Some(network_sender);
        let mut network = drop_guard::guard(network, move |network| {
            // If the network receiver was already dropped, we just drop the network here,
            // just like Arc, so we don't care about the result.
            let _ = network_sender
                .take()
                .expect("Only used once in drop")
                .send(network);
        });
        let mut stop_receiver = stop_receiver.fuse();
        loop {
            let participant = match select!(
                _ = &mut stop_receiver => None,
                p = network.connected().fuse() => Some(p),
            ) {
                None => break,
                Some(Ok(p)) => p,
                Some(Err(e)) => {
                    error!(
                        ?e,
                        "Stopping Connection Handler, no new connections can be made to server \
                         now!"
                    );
                    break;
                },
            };

            let client_sender = client_sender.clone();
            let info_requester_sender = info_requester_sender.clone();

            match select!(
                _ = &mut stop_receiver => None,
                e = Self::init_participant(participant, client_sender, info_requester_sender).fuse() => Some(e),
            ) {
                None => break,
                Some(Ok(())) => (),
                Some(Err(e)) => warn!(?e, "drop new participant, because an error occurred"),
            }
        }
    }

    async fn init_participant(
        mut participant: Participant,
        client_sender: Sender<IncomingClient>,
        info_requester_sender: Sender<oneshot::Sender<ServerInfoPacket>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!("New Participant connected to the server");
        info!(pid = ?participant.remote_pid(), stage = "open-streams", "Client setup");

        let reliable = Promises::ORDERED | Promises::CONSISTENCY;
        let reliablec = reliable | Promises::COMPRESSED;

        let general_stream = participant.open(3, reliablec, 500).await?;
        let ping_stream = participant.open(2, reliable, 500).await?;
        let mut register_stream = participant.open(3, reliablec, 500).await?;
        let character_screen_stream = participant.open(3, reliablec, 500).await?;
        let in_game_stream = participant.open(3, reliablec, 100_000).await?;
        let terrain_stream = participant.open(4, reliable, 20_000).await?;

        info!(pid = ?participant.remote_pid(), stage = "await-server-info", "Client setup");
        let server_data = request_server_info(&info_requester_sender).await?;

        register_stream.send(server_data.info)?;
        info!(pid = ?participant.remote_pid(), stage = "server-info-sent", "Client setup");

        let Some(client_type) =
            receive_client_hello(&mut register_stream, common::util::GAME_VERSION).await?
        else {
            return Ok(());
        };

        use network::ParticipantEvent;
        let connected_from = match select!(
            _ = tokio::time::sleep(TIMEOUT).fuse() => None,
            connected_from = participant.fetch_event().fuse() => Some(connected_from),
        ) {
            None => {
                error!("Did not receive initial channel created event. This is a bug!");
                return Ok(());
            },
            Some(Err(err)) => {
                debug!("Participant error when trying to receive event: {err:?}");
                return Ok(());
            },
            Some(Ok(ParticipantEvent::ChannelDeleted(_))) => {
                error!(
                    "Received channel deleted event instead of the initial channel created event. \
                     This is a bug!"
                );
                return Ok(());
            },
            Some(Ok(ParticipantEvent::ChannelCreated(connected_from))) => connected_from,
        };

        let client = Client::new(
            client_type,
            participant,
            connected_from,
            server_data.time,
            None,
            general_stream,
            ping_stream,
            register_stream,
            character_screen_stream,
            in_game_stream,
            terrain_stream,
        );

        client_sender.send(client)?;
        Ok(())
    }
}

impl Drop for ConnectionHandler {
    fn drop(&mut self) {
        let _ = self
            .stop_sender
            .take()
            .expect("`stop_sender` is private, initialized as `Some`, and only updated in Drop")
            .send(());
        trace!("aborting ConnectionHandler");
        self.thread_handle
            .take()
            .expect("`thread_handle` is private, initialized as `Some`, and only updated in Drop")
            .abort();
        trace!("aborted ConnectionHandler!");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common_net::msg::GameVersionMismatch;
    use network::{ConnectAddr, ListenAddr, Pid};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PORT: AtomicU64 = AtomicU64::new(30_000);

    fn gate_over_register_stream(
        client_version: Option<u32>,
        server_version: u32,
    ) -> (
        Result<Option<ClientType>, String>,
        Option<GameVersionAnswer>,
    ) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let port = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
            let mut server = Network::new(Pid::new(), &runtime);
            let client = Network::new(Pid::new(), &runtime);
            server.listen(ListenAddr::Mpsc(port)).await.unwrap();
            let mut client_participant = client.connect(ConnectAddr::Mpsc(port)).await.unwrap();
            let server_participant = server.connected().await.unwrap();
            let mut server_stream = server_participant
                .open(
                    3,
                    Promises::ORDERED | Promises::CONSISTENCY | Promises::COMPRESSED,
                    500,
                )
                .await
                .unwrap();
            let mut client_stream = client_participant.opened().await.unwrap();

            // Exercise the real compressed register-stream message order:
            // server info, client hello, then the server admission answer.
            server_stream
                .send(ServerInfo {
                    name: "version-gate-test".to_owned(),
                    git_hash: 0,
                    git_timestamp: 0,
                    auth_provider: None,
                    game_version: server_version,
                })
                .unwrap();
            let info: ServerInfo = client_stream.recv().await.unwrap();
            assert_eq!(info.game_version, server_version);
            let result = if let Some(game_version) = client_version {
                client_stream
                    .send(ClientHello {
                        client_type: ClientType::Game,
                        game_version,
                    })
                    .unwrap();
                let (accepted, answer) = tokio::join!(
                    receive_client_hello(&mut server_stream, server_version),
                    client_stream.recv::<GameVersionAnswer>(),
                );
                (
                    accepted.map_err(|error| error.to_string()),
                    Some(answer.unwrap()),
                )
            } else {
                client_stream.send(ClientType::Game).unwrap();
                (
                    receive_client_hello(&mut server_stream, server_version)
                        .await
                        .map_err(|error| error.to_string()),
                    None,
                )
            };
            drop(server_stream);
            drop(client_stream);
            let _ = tokio::join!(
                server_participant.disconnect(),
                client_participant.disconnect()
            );
            result
        })
    }

    #[test]
    fn matching_numeric_version_is_admitted_before_registration() {
        let version = common::util::GAME_VERSION;
        let (accepted, answer) = gate_over_register_stream(Some(version), version);
        assert_eq!(accepted.unwrap(), Some(ClientType::Game));
        assert_eq!(answer, Some(Ok(())));
    }

    #[test]
    fn mismatched_versions_return_both_numbers_without_admitting_a_client() {
        for (client, server) in [(0, 1), (2, 1), (1, 2), (u32::MAX, 1)] {
            let (accepted, answer) = gate_over_register_stream(Some(client), server);
            assert_eq!(accepted.unwrap(), None);
            assert_eq!(answer, Some(Err(GameVersionMismatch { client, server })));
        }
    }

    #[test]
    fn legacy_client_type_without_numeric_version_fails_closed() {
        let (accepted, answer) = gate_over_register_stream(None, common::util::GAME_VERSION);
        assert!(accepted.is_err());
        assert!(answer.is_none());
    }
}
