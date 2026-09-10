// trueos-blueprint: features=["lifecycle-net"]
//! Minimal cubesrv: stock Axum health routes plus world-aware UDP telemetry.

extern crate alloc;

mod protocol;

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::net::SocketAddr;
use core::sync::atomic::{AtomicU16, Ordering};

use axum::{Router, routing::get, serve::ListenerExt};
use protocol::{BlobKind, ClientPacket, Telemetry};
use trueos::{
    logl,
    logl::level,
    platform, runtime,
    time::{self, Duration},
    tokio::{self, net::UdpSocket, sync::RwLock},
};

const HTTP_PORT: u16 = 18;
const UDP_PORT: u16 = 30_018;
const UDP_RETRY_MS: u64 = 1_000;
const MAX_PLAYERS: usize = 64;
static PUBLISHED_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

struct Blob {
    #[allow(dead_code)]
    name: &'static str,
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[derive(Clone, Copy)]
struct Player {
    id: u32,
    world_id: u8,
    telemetry: Option<Telemetry>,
}

struct ServerState {
    next_player_id: u32,
    players: BTreeMap<SocketAddr, Player>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            next_player_id: 1,
            players: BTreeMap::new(),
        }
    }

    fn join(&mut self, peer: SocketAddr, world_id: u8) -> Option<u32> {
        if let Some(player) = self.players.get_mut(&peer) {
            player.world_id = world_id;
            player.telemetry = None;
            return Some(player.id);
        }
        if self.players.len() >= MAX_PLAYERS {
            return None;
        }
        let id = self.next_player_id;
        self.next_player_id = self.next_player_id.wrapping_add(1).max(1);
        self.players.insert(
            peer,
            Player {
                id,
                world_id,
                telemetry: None,
            },
        );
        Some(id)
    }

    fn telemetry(
        &mut self,
        peer: SocketAddr,
        telemetry: Telemetry,
    ) -> Option<(u32, bool, Vec<SocketAddr>)> {
        let mut joined = false;
        if !self.players.contains_key(&peer) {
            self.join(peer, telemetry.world_id)?;
            joined = true;
        }
        let player = self.players.get_mut(&peer).unwrap();
        let changed_world = player.world_id != telemetry.world_id;
        let accept = player
            .telemetry
            .is_none_or(|previous| protocol::is_newer(telemetry.sequence, previous.sequence));
        if !accept {
            return None;
        }
        player.world_id = telemetry.world_id;
        player.telemetry = Some(telemetry);
        let id = player.id;
        let recipients = self
            .players
            .iter()
            .filter_map(|(address, other)| {
                (*address != peer && other.world_id == telemetry.world_id).then_some(*address)
            })
            .collect();
        Some((id, joined || changed_world, recipients))
    }
}

async fn hello() -> &'static str {
    "cubesrv hello\n"
}

fn router() -> Router {
    Router::new()
        .route("/", get(hello))
        .route("/healthz", get(hello))
}

fn blob(catalog: &'static [Blob], id: u8) -> Option<&'static Blob> {
    id.checked_sub(1)
        .and_then(|index| catalog.get(index as usize))
}

fn chunk_count(bytes: &[u8]) -> u16 {
    bytes
        .len()
        .div_ceil(protocol::BLOB_CHUNK_BYTES)
        .try_into()
        .unwrap()
}

async fn send_welcome(socket: &UdpSocket, peer: SocketAddr, player_id: u32, world_id: u8) {
    let world = &WORLDS[world_id as usize - 1];
    let packet = protocol::welcome(
        player_id,
        world_id,
        world.bytes.len(),
        chunk_count(world.bytes),
        ASSETS.len() as u8,
    );
    if let Err(error) = socket.send_to(&packet, peer).await {
        logl::log(
            level::WARN,
            format_args!("cubesrv: welcome to {peer} failed: {error}"),
        );
    }
}

async fn handle_packet(
    socket: &UdpSocket,
    state: &RwLock<ServerState>,
    peer: SocketAddr,
    bytes: &[u8],
) {
    let packet = match protocol::decode(bytes) {
        Ok(packet) => packet,
        Err(_) => return,
    };
    match packet {
        ClientPacket::Hello { world_id } => {
            let player_id = state.write().await.join(peer, world_id);
            match player_id {
                Some(player_id) => send_welcome(socket, peer, player_id, world_id).await,
                None => {
                    let _ = socket.send_to(&protocol::error(1), peer).await;
                }
            }
        }
        ClientPacket::Telemetry(telemetry) => {
            let accepted = state.write().await.telemetry(peer, telemetry);
            let Some((player_id, send_info, recipients)) = accepted else {
                return;
            };
            if send_info {
                send_welcome(socket, peer, player_id, telemetry.world_id).await;
            }
            let packet = protocol::player_state(player_id, telemetry);
            for recipient in recipients {
                if let Err(error) = socket.send_to(&packet, recipient).await {
                    logl::log(
                        level::DEBUG,
                        format_args!("cubesrv: telemetry to {recipient} failed: {error}"),
                    );
                }
            }
        }
        ClientPacket::WorldRequest { chunk } => {
            let Some(world_id) = state
                .read()
                .await
                .players
                .get(&peer)
                .map(|player| player.world_id)
            else {
                let _ = socket.send_to(&protocol::error(4), peer).await;
                return;
            };
            let Some(world) = blob(WORLDS, world_id) else {
                return;
            };
            let Some(packet) = protocol::blob_chunk(BlobKind::World, world_id, chunk, world.bytes)
            else {
                let _ = socket.send_to(&protocol::error(2), peer).await;
                return;
            };
            let _ = socket.send_to(&packet, peer).await;
        }
        ClientPacket::AssetRequest { asset_id, chunk } => {
            let Some(asset) = blob(ASSETS, asset_id) else {
                let _ = socket.send_to(&protocol::error(3), peer).await;
                return;
            };
            let Some(packet) = protocol::blob_chunk(BlobKind::Asset, asset_id, chunk, asset.bytes)
            else {
                let _ = socket.send_to(&protocol::error(2), peer).await;
                return;
            };
            let _ = socket.send_to(&packet, peer).await;
        }
    }
}

async fn udp_loop(state: Arc<RwLock<ServerState>>) {
    let addr = SocketAddr::from(([0, 0, 0, 0], UDP_PORT));
    loop {
        let socket = match UdpSocket::bind(addr).await {
            Ok(socket) => socket,
            Err(error) => {
                logl::log(
                    level::WARN,
                    format_args!("cubesrv: udp bind failed: {error}"),
                );
                time::sleep(Duration::from_millis(UDP_RETRY_MS)).await;
                continue;
            }
        };
        logl::log(
            level::INFO,
            format_args!("cubesrv: udp listening on {addr}"),
        );

        let mut buffer = [0_u8; protocol::MAX_DATAGRAM];
        loop {
            match socket.recv_from(&mut buffer).await {
                Ok((length, peer)) => {
                    handle_packet(&socket, &state, peer, &buffer[..length]).await;
                }
                Err(error) => {
                    logl::log(
                        level::WARN,
                        format_args!("cubesrv: udp receive failed: {error}; rebinding"),
                    );
                    break;
                }
            }
        }
        time::sleep(Duration::from_millis(UDP_RETRY_MS)).await;
    }
}

async fn serve() -> Result<(), trueos::platform::io::Error> {
    let state = Arc::new(RwLock::new(ServerState::new()));
    tokio::task::spawn_local(udp_loop(state));
    let addr = SocketAddr::from(([0, 0, 0, 0], HTTP_PORT));
    let listener = trueos::lifecycle_axum_listener!("cubesrv", addr, &PUBLISHED_HTTP_PORT).await;
    let listener = listener.tap_io(|_| logl::log(level::INFO, "cubesrv: tcp accepted"));
    logl::log(
        level::INFO,
        format_args!(
            "cubesrv: http listening on {}",
            PUBLISHED_HTTP_PORT.load(Ordering::Acquire)
        ),
    );
    axum::serve(listener, router()).await
}

fn main() {
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("cubesrv: runtime build failed: {error}"),
            );
            return;
        }
    };
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, async {
        if let Err(error) = serve().await {
            logl::log(
                level::ERROR,
                format_args!("cubesrv: server failed: {error:?}"),
            );
        }
    });
    platform::poll_once();
}
