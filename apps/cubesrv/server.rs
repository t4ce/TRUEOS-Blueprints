// trueos-blueprint: features=["lifecycle-net"]
//! Six-face gallery plus a revision-pinned, sparse 150 ms cube-frame asset.

extern crate alloc;

mod protocol;
use cubes_protocol as plateau;
mod profiles;

use alloc::{collections::BTreeMap, string::String, sync::Arc, vec, vec::Vec};
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
const VFX_CYCLE_MS: u64 = 3_000;
const VFX_DELAY_MS: u64 = 500;
const IDLE_VFX_FRAME: u8 = u8::MAX;
static PUBLISHED_HTTP_PORT: AtomicU16 = AtomicU16::new(0);

struct Blob {
    #[allow(dead_code)]
    name: &'static str,
    bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[derive(Clone)]
struct Player {
    username: String,
    id: u32,
    world_id: u8,
    telemetry: Option<Telemetry>,
    last_seen: time::Instant,
}

struct ServerState {
    next_player_id: u32,
    revision: u32,
    holy_frame: Option<u8>,
    vfx_anchor: [i16; 3],
    vfx_terrain: bool,
    vfx_event: u32,
    players: BTreeMap<SocketAddr, Player>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            next_player_id: 1,
            revision: GALLERY_REVISION,
            holy_frame: None,
            vfx_anchor: [0; 3],
            vfx_terrain: false,
            vfx_event: 0,
            players: BTreeMap::new(),
        }
    }

    fn join(&mut self, peer: SocketAddr, world_id: u8, username: &str) -> Option<u32> {
        self.players
            .retain(|_, player| player.last_seen.elapsed() < Duration::from_secs(30));
        if let Some(player) = self.players.get_mut(&peer) {
            if player.username != username { return None; }
            player.world_id = world_id;
            player.last_seen = time::Instant::now();
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
                username: username.into(),
                id,
                world_id,
                telemetry: None,
                last_seen: time::Instant::now(),
            },
        );
        Some(id)
    }

    fn telemetry(
        &mut self,
        peer: SocketAddr,
        telemetry: Telemetry,
    ) -> Option<(u32, bool, Vec<SocketAddr>)> {
        if !self.players.contains_key(&peer) {
            return None;
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
        player.last_seen = time::Instant::now();
        let id = player.id;
        let recipients = self
            .players
            .iter()
            .filter_map(|(address, other)| {
                (*address != peer && other.world_id == telemetry.world_id).then_some(*address)
            })
            .collect();
        Some((id, changed_world, recipients))
    }
}

/// Pick a deterministic cardinal c4-block position 5..=10 blocks from the landmark.
/// Coordinates are c1 lattice units, so each terrain block is eight units wide.
fn vfx_spawn_anchor(event: u32) -> [i16; 3] {
    let distance = (5 + event % 6) as i16 * 8;
    match event % 4 {
        0 => [distance, 0, 0],
        1 => [0, 0, distance],
        2 => [-distance, 0, 0],
        _ => [0, 0, -distance],
    }
}

async fn announce_vfx(socket: &UdpSocket, players: Vec<(SocketAddr, u32)>,
    frame: Option<u8>, anchor: [i16; 3], terrain: bool, event: u32)
{
    let sequence = cubes_protocol::holy::Sequence::parse(HOLY).unwrap();
    let frame = frame.unwrap_or(IDLE_VFX_FRAME);
    let encoded_len = sequence.frame(frame).map_or(0, |bytes| bytes.len());
    for (peer, id) in players {
        let _ = socket.send_to(&protocol::holy_info(id, GALLERY_REVISION, HOLY_REVISION,
            frame, encoded_len, anchor, terrain, event), peer).await;
    }
}

async fn hello() -> &'static str {
    "cubesrv hello\n"
}

fn router() -> Router {
    Router::new()
        .route("/", get(hello))
        .route("/healthz", get(hello))
        .merge(profiles::router(Arc::new(profiles::Store::new("common/cubesrv/cubeusers.db"))))
}

fn blob(catalog: &'static [Blob], id: u8) -> Option<&'static Blob> {
    id.checked_sub(1)
        .and_then(|index| catalog.get(index as usize))
}

async fn send_welcome(
    socket: &UdpSocket,
    state: &RwLock<ServerState>,
    peer: SocketAddr,
    player_id: u32,
) {
    let _ = socket.send_to(&protocol::welcome(player_id, 1, WORLD1.len(),
        WORLD1.len().div_ceil(protocol::BLOB_CHUNK_BYTES) as u16, ASSETS.len() as u8), peer).await;
    let (revision, holy_frame, vfx_anchor, vfx_terrain, vfx_event) = {
        let state = state.read().await;
        (state.revision, state.holy_frame, state.vfx_anchor, state.vfx_terrain, state.vfx_event)
    };
    let _ = socket
        .send_to(
            &protocol::slide_info(
                player_id,
                revision,
                GALLERY.len(),
            ),
            peer,
        )
        .await;
    announce_vfx(socket, vec![(peer, player_id)], holy_frame, vfx_anchor, vfx_terrain, vfx_event).await;
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
        ClientPacket::Hello { username, .. } => {
            let world_id = 1;
            let player_id = state.write().await.join(peer, world_id, username);
            match player_id {
                Some(player_id) => send_welcome(socket, state, peer, player_id).await,
                None => {
                    let _ = socket.send_to(&protocol::error(1), peer).await;
                }
            }
        }
        ClientPacket::Telemetry(mut telemetry) => {
            telemetry.world_id = 1;
            let accepted = state.write().await.telemetry(peer, telemetry);
            let Some((player_id, send_info, recipients)) = accepted else {
                return;
            };
            let _ = send_info;
            send_welcome(socket, state, peer, player_id).await;
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
        ClientPacket::SlideRequest { revision, chunk } => {
            {
                let mut state = state.write().await;
                let Some(player) = state.players.get_mut(&peer) else {
                    return;
                };
                player.last_seen = time::Instant::now();
            }
            if revision != GALLERY_REVISION { return; }
            if let Some(packet) = protocol::slide_chunk(revision, chunk, GALLERY) {
                let _ = socket.send_to(&packet, peer).await;
            }
        }
        ClientPacket::HolyRequest { revision, frame, chunk } => {
            {
                let mut state = state.write().await;
                let Some(player) = state.players.get_mut(&peer) else { return; };
                player.last_seen = time::Instant::now();
            }
            if revision != HOLY_REVISION { return; }
            let sequence = cubes_protocol::holy::Sequence::parse(HOLY).unwrap();
            let Some(bytes) = sequence.frame(frame) else { return; };
            if let Some(packet) = protocol::holy_chunk(revision, frame, chunk, bytes) {
                let _ = socket.send_to(&packet, peer).await;
            }
        }
        ClientPacket::WorldRequest { chunk } => {
            {
                let mut state = state.write().await;
                let Some(player) = state.players.get_mut(&peer) else { return; };
                player.last_seen = time::Instant::now();
            }
            if let Some(packet) = protocol::blob_chunk(BlobKind::World, 1, chunk, WORLD1) {
                let _ = socket.send_to(&packet, peer).await;
            }
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
            format_args!("cubesrv: udp listening on {addr} world=1 world_bytes={} world_chunks={} welcome=0x81",
                WORLD1.len(), WORLD1.len().div_ceil(protocol::BLOB_CHUNK_BYTES)),
        );

        let mut next_slide = time::Instant::now() + Duration::from_secs(10);
        let mut next_spawn = time::Instant::now();
        let mut vfx_start = None;
        let mut next_holy = None;
        let mut buffer = [0_u8; protocol::MAX_DATAGRAM];
        loop {
            let now = time::Instant::now();
            if now >= next_slide {
                let (revision, players) = {
                    let mut state = state.write().await;
                    state
                        .players
                        .retain(|_, player| player.last_seen.elapsed() < Duration::from_secs(30));
                    // Static gallery identity changes only when its encoded content changes.
                    (
                        state.revision,
                        state
                            .players
                            .iter()
                            .map(|(peer, p)| (*peer, p.id))
                            .collect::<Vec<_>>(),
                    )
                };
                next_slide += Duration::from_secs(10);
                for (peer, id) in players {
                    let _ = socket
                        .send_to(
                            &protocol::slide_info(
                                id,
                                revision,
                                GALLERY.len(),
                            ),
                            peer,
                        )
                        .await;
                }
            }
            if now >= next_spawn {
                let (anchor, event, players) = {
                    let mut state = state.write().await;
                    state.vfx_event = state.vfx_event.wrapping_add(1).max(1);
                    state.vfx_anchor = vfx_spawn_anchor(state.vfx_event);
                    state.vfx_terrain = true;
                    state.holy_frame = None;
                    (state.vfx_anchor, state.vfx_event, state.players.iter().map(|(peer, p)| (*peer, p.id)).collect::<Vec<_>>())
                };
                announce_vfx(&socket, players, None, anchor, true, event).await;
                next_spawn += Duration::from_millis(VFX_CYCLE_MS);
                vfx_start = Some(now + Duration::from_millis(VFX_DELAY_MS));
            }
            if vfx_start.is_some_and(|start| now >= start) {
                let (anchor, event, players) = {
                    let mut state = state.write().await;
                    state.holy_frame = Some(0);
                    (state.vfx_anchor, state.vfx_event, state.players.iter().map(|(peer, p)| (*peer, p.id)).collect::<Vec<_>>())
                };
                announce_vfx(&socket, players, Some(0), anchor, true, event).await;
                vfx_start = None;
                next_holy = Some(now + Duration::from_millis(cubes_protocol::holy::PERIOD_MS as u64));
            }
            if next_holy.is_some_and(|deadline| now >= deadline) {
                let sequence = cubes_protocol::holy::Sequence::parse(HOLY).unwrap();
                let (frame, anchor, terrain, event, players) = {
                    let mut state = state.write().await;
                    let frame = state.holy_frame.unwrap_or(0).saturating_add(1);
                    if frame == sequence.frame_count() {
                        state.holy_frame = None;
                        state.vfx_terrain = false;
                        (None, state.vfx_anchor, false, state.vfx_event,
                            state.players.iter().map(|(peer, p)| (*peer, p.id)).collect::<Vec<_>>())
                    } else {
                        state.holy_frame = Some(frame);
                        (Some(frame), state.vfx_anchor, true, state.vfx_event,
                            state.players.iter().map(|(peer, p)| (*peer, p.id)).collect::<Vec<_>>())
                    }
                };
                announce_vfx(&socket, players, frame, anchor, terrain, event).await;
                next_holy = frame.map(|_| now + Duration::from_millis(cubes_protocol::holy::PERIOD_MS as u64));
            }
            let mut next_event = next_slide.min(next_spawn);
            if let Some(deadline) = vfx_start { next_event = next_event.min(deadline); }
            if let Some(deadline) = next_holy { next_event = next_event.min(deadline); }
            match time::timeout(
                next_event.saturating_duration_since(time::Instant::now()),
                socket.recv_from(&mut buffer),
            )
            .await
            {
                Err(_) => continue,
                Ok(Ok((length, peer))) => {
                    handle_packet(&socket, &state, peer, &buffer[..length]).await;
                }
                Ok(Err(error)) => {
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
