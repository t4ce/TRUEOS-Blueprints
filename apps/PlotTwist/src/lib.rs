extern crate alloc;

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::f64::consts::{E, PI};
use serde::{Deserialize, Serialize};

pub const WORLD_MIN: f32 = -500.0;
pub const WORLD_MAX: f32 = 500.0;
pub const PLOT_WIDTH: u16 = 1920;
pub const PLOT_HEIGHT: u16 = 1080;
pub const TURN_MS: u64 = 60_000;
pub const COUNTDOWN_MS: u64 = 10_000;
pub const TRACE_MS: u64 = 6_000;
const FIGURES_PER_PLAYER: usize = 4;
const HIT_RADIUS_PX: i32 = 16;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    pub nickname: String,
    pub avatar: Avatar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum Avatar {
    Preset(u8),
    Image(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub token: String,
    pub player_id: u64,
    pub nickname: String,
    pub avatar: Avatar,
}

#[derive(Debug, Clone)]
struct Profile {
    player_id: u64,
    nickname: String,
    avatar: Avatar,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbySummary {
    pub id: String,
    pub name: String,
    pub player_count: usize,
    pub capacity: usize,
    pub phase: &'static str,
}

#[derive(Debug, Clone)]
struct Lobby {
    id: String,
    name: String,
    players: Vec<Player>,
    phase: Phase,
    chat: Vec<ChatMessage>,
    seed: u64,
}

#[derive(Debug, Clone)]
enum Phase {
    Waiting,
    Countdown {
        ends_at_ms: u64,
    },
    Playing(Game),
    Finished {
        reason: String,
        winner_team: Option<u8>,
    },
}

#[derive(Debug, Clone)]
struct Player {
    token: String,
    player_id: u64,
    nickname: String,
    avatar: Avatar,
    team: u8,
    color: String,
    ready: bool,
    connected: bool,
    measurements: Vec<Point>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerView {
    pub player_id: u64,
    pub nickname: String,
    pub avatar: Avatar,
    pub team: u8,
    pub color: String,
    pub ready: bool,
    pub connected: bool,
    pub alive_figures: usize,
    pub measurements: Vec<Point>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LobbySnapshot {
    pub id: String,
    pub name: String,
    pub phase: PhaseView,
    pub players: Vec<PlayerView>,
    pub chat: Vec<ChatMessage>,
    pub game: Option<GameView>,
    pub server_now_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PhaseView {
    Waiting,
    Countdown {
        ends_at_ms: u64,
    },
    Playing,
    Finished {
        reason: String,
        winner_team: Option<u8>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameView {
    pub round: u32,
    pub current_player_id: u64,
    pub turn_ends_at_ms: u64,
    pub paused: bool,
    pub figures: Vec<Figure>,
    pub obstacles: Vec<Obstacle>,
    pub traces: Vec<PlotTrace>,
}

#[derive(Debug, Clone)]
struct Game {
    round: u32,
    turn_index: usize,
    turn_ends_at_ms: u64,
    paused_remaining_ms: Option<u64>,
    figures: Vec<Figure>,
    obstacles: Vec<Obstacle>,
    traces: Vec<PlotTrace>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Figure {
    pub id: u32,
    pub owner_id: u64,
    pub index: u8,
    pub team: u8,
    pub x: f32,
    pub y: f32,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Obstacle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub depth: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlotTrace {
    pub owner_id: u64,
    pub expression: String,
    pub points: Vec<Point>,
    pub expires_at_ms: u64,
    pub hit_figure_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub player_id: u64,
    pub nickname: String,
    pub text: String,
    pub sent_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Action {
    SetTeam { team: u8 },
    SetColor { color: String },
    SetReady { ready: bool },
    Leave,
    Chat { text: String },
    Pause { paused: bool },
    EndGame,
    Measure { x: f32, y: f32 },
    Plot { expression: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub snapshot: Option<LobbySnapshot>,
    pub left: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameError(pub String);

impl GameError {
    fn new(message: impl ToString) -> Self {
        Self(message.to_string())
    }
}

pub struct PlotTwist {
    profiles: BTreeMap<String, Profile>,
    lobbies: BTreeMap<String, Lobby>,
    next_id: u64,
    rng: SoftRng,
}

impl Default for PlotTwist {
    fn default() -> Self {
        Self::new(0x50_4c_4f_54_54_57_49_53)
    }
}

impl PlotTwist {
    pub fn new(seed: u64) -> Self {
        Self {
            profiles: BTreeMap::new(),
            lobbies: BTreeMap::new(),
            next_id: 1,
            rng: SoftRng::new(seed),
        }
    }

    pub fn connect(&mut self, request: ConnectRequest) -> Result<Session, GameError> {
        let nickname = request.nickname.trim();
        if nickname.is_empty() || nickname.chars().count() > 24 {
            return Err(GameError::new("nickname must contain 1-24 characters"));
        }
        validate_avatar(&request.avatar)?;
        let player_id = self.next_id;
        self.next_id += 1;
        let token = format!("{:016x}{:016x}", self.rng.next(), self.rng.next());
        let profile = Profile {
            player_id,
            nickname: nickname.to_string(),
            avatar: request.avatar.clone(),
        };
        self.profiles.insert(token.clone(), profile.clone());
        Ok(Session {
            token,
            player_id,
            nickname: profile.nickname,
            avatar: profile.avatar,
        })
    }

    pub fn lobbies(&mut self, now_ms: u64) -> Vec<LobbySummary> {
        self.advance(now_ms);
        self.lobbies
            .values()
            .filter(|lobby| matches!(lobby.phase, Phase::Waiting | Phase::Countdown { .. }))
            .map(|lobby| LobbySummary {
                id: lobby.id.clone(),
                name: lobby.name.clone(),
                player_count: lobby.players.len(),
                capacity: 4,
                phase: if matches!(lobby.phase, Phase::Countdown { .. }) {
                    "countdown"
                } else {
                    "waiting"
                },
            })
            .collect()
    }

    pub fn create_lobby(&mut self, token: &str, now_ms: u64) -> Result<LobbySnapshot, GameError> {
        self.ensure_not_in_lobby(token)?;
        let profile = self.profile(token)?.clone();
        let id = format!("{:06X}", self.rng.next() & 0x00ff_ffff);
        let lobby = Lobby {
            id: id.clone(),
            name: format!("{}'s plot", profile.nickname),
            players: vec![new_player(token, &profile, 1)],
            phase: Phase::Waiting,
            chat: Vec::new(),
            seed: self.rng.next(),
        };
        self.lobbies.insert(id.clone(), lobby);
        self.snapshot(&id, token, now_ms)
    }

    pub fn join_lobby(
        &mut self,
        lobby_id: &str,
        token: &str,
        now_ms: u64,
    ) -> Result<LobbySnapshot, GameError> {
        self.advance(now_ms);
        self.ensure_not_in_lobby(token)?;
        let profile = self.profile(token)?.clone();
        let lobby = self
            .lobbies
            .get_mut(lobby_id)
            .ok_or_else(|| GameError::new("lobby not found"))?;
        if lobby.players.len() >= 4 {
            return Err(GameError::new("lobby is full"));
        }
        if !matches!(lobby.phase, Phase::Waiting | Phase::Countdown { .. }) {
            return Err(GameError::new("game already started"));
        }
        lobby.phase = Phase::Waiting;
        for player in &mut lobby.players {
            player.ready = false;
        }
        let team = (1..=4)
            .find(|team| !lobby.players.iter().any(|player| player.team == *team))
            .unwrap_or(1);
        lobby.players.push(new_player(token, &profile, team));
        self.snapshot(lobby_id, token, now_ms)
    }

    pub fn snapshot(
        &mut self,
        lobby_id: &str,
        token: &str,
        now_ms: u64,
    ) -> Result<LobbySnapshot, GameError> {
        self.advance(now_ms);
        let lobby = self
            .lobbies
            .get(lobby_id)
            .ok_or_else(|| GameError::new("lobby not found"))?;
        if !lobby.players.iter().any(|player| player.token == token) {
            return Err(GameError::new("you are not in this lobby"));
        }
        Ok(lobby_view(lobby, now_ms))
    }

    pub fn act(
        &mut self,
        lobby_id: &str,
        token: &str,
        action: Action,
        now_ms: u64,
    ) -> Result<ActionResult, GameError> {
        self.advance(now_ms);
        let mut remove_lobby = false;
        let mut left = false;
        {
            let lobby = self
                .lobbies
                .get_mut(lobby_id)
                .ok_or_else(|| GameError::new("lobby not found"))?;
            let player_index = lobby
                .players
                .iter()
                .position(|player| player.token == token)
                .ok_or_else(|| GameError::new("you are not in this lobby"))?;
            match action {
                Action::SetTeam { team } => {
                    require_waiting(lobby)?;
                    if !(1..=4).contains(&team) {
                        return Err(GameError::new("team must be between 1 and 4"));
                    }
                    lobby.players[player_index].team = team;
                    if let Some(color) = lobby
                        .players
                        .iter()
                        .find(|player| player.team == team && player.token != token)
                        .map(|player| player.color.clone())
                    {
                        lobby.players[player_index].color = color;
                    }
                    cancel_ready(lobby);
                }
                Action::SetColor { color } => {
                    require_waiting(lobby)?;
                    if !valid_color(&color) {
                        return Err(GameError::new("color must be a #RRGGBB value"));
                    }
                    let team = lobby.players[player_index].team;
                    for player in &mut lobby.players {
                        if player.team == team {
                            player.color.clone_from(&color);
                        }
                    }
                    cancel_ready(lobby);
                }
                Action::SetReady { ready } => {
                    require_waiting(lobby)?;
                    lobby.players[player_index].ready = ready;
                    update_countdown(lobby, now_ms);
                }
                Action::Leave => {
                    let leaving_id = lobby.players[player_index].player_id;
                    let current_player_id = match &lobby.phase {
                        Phase::Playing(game) => lobby
                            .players
                            .get(game.turn_index)
                            .map(|player| player.player_id),
                        _ => None,
                    };
                    lobby.players.remove(player_index);
                    left = true;
                    if lobby.players.is_empty() {
                        remove_lobby = true;
                    } else {
                        match &mut lobby.phase {
                            Phase::Waiting | Phase::Countdown { .. } => cancel_ready(lobby),
                            Phase::Playing(game) => {
                                for figure in &mut game.figures {
                                    if figure.owner_id == leaving_id {
                                        figure.alive = false;
                                    }
                                }
                                game.turn_index = current_player_id
                                    .and_then(|id| {
                                        lobby
                                            .players
                                            .iter()
                                            .position(|player| player.player_id == id)
                                    })
                                    .unwrap_or_else(|| player_index.min(lobby.players.len() - 1));
                                normalize_turn(lobby, now_ms);
                            }
                            Phase::Finished { .. } => {}
                        }
                    }
                }
                Action::Chat { text } => {
                    let text = text.trim();
                    if text.is_empty() || text.chars().count() > 240 {
                        return Err(GameError::new("chat message must contain 1-240 characters"));
                    }
                    let player = &lobby.players[player_index];
                    if lobby.chat.len() >= 100 {
                        lobby.chat.remove(0);
                    }
                    lobby.chat.push(ChatMessage {
                        player_id: player.player_id,
                        nickname: player.nickname.clone(),
                        text: text.to_string(),
                        sent_at_ms: now_ms,
                    });
                }
                Action::Pause { paused } => set_paused(lobby, paused, now_ms)?,
                Action::EndGame => {
                    if !matches!(lobby.phase, Phase::Playing(_)) {
                        return Err(GameError::new("game is not running"));
                    }
                    lobby.phase = Phase::Finished {
                        reason: format!("ended by {}", lobby.players[player_index].nickname),
                        winner_team: None,
                    };
                }
                Action::Measure { x, y } => {
                    require_turn(lobby, player_index)?;
                    if !x.is_finite()
                        || !y.is_finite()
                        || !(WORLD_MIN..=WORLD_MAX).contains(&x)
                        || !(WORLD_MIN..=WORLD_MAX).contains(&y)
                    {
                        return Err(GameError::new("measurement is outside the plot"));
                    }
                    if lobby.players[player_index].measurements.len() >= 3 {
                        return Err(GameError::new(
                            "only three measurements are allowed per turn",
                        ));
                    }
                    lobby.players[player_index]
                        .measurements
                        .push(Point { x, y });
                }
                Action::Plot { expression } => plot_turn(lobby, player_index, &expression, now_ms)?,
            }
        }
        if remove_lobby {
            self.lobbies.remove(lobby_id);
        }
        let snapshot = if left || remove_lobby {
            None
        } else {
            Some(self.snapshot(lobby_id, token, now_ms)?)
        };
        Ok(ActionResult { snapshot, left })
    }

    fn profile(&self, token: &str) -> Result<&Profile, GameError> {
        self.profiles
            .get(token)
            .ok_or_else(|| GameError::new("invalid session"))
    }

    fn ensure_not_in_lobby(&self, token: &str) -> Result<(), GameError> {
        if self
            .lobbies
            .values()
            .any(|lobby| lobby.players.iter().any(|player| player.token == token))
        {
            Err(GameError::new("leave your current lobby first"))
        } else {
            Ok(())
        }
    }

    fn advance(&mut self, now_ms: u64) {
        let ids: Vec<String> = self.lobbies.keys().cloned().collect();
        for id in ids {
            let Some(lobby) = self.lobbies.get_mut(&id) else {
                continue;
            };
            let start =
                matches!(lobby.phase, Phase::Countdown { ends_at_ms } if now_ms >= ends_at_ms);
            if start {
                if lobby.players.len() >= 2 && lobby.players.iter().all(|player| player.ready) {
                    start_game(lobby, now_ms);
                } else {
                    cancel_ready(lobby);
                }
            }
            if let Phase::Playing(game) = &mut lobby.phase {
                game.traces.retain(|trace| trace.expires_at_ms > now_ms);
                if game.paused_remaining_ms.is_none() && now_ms >= game.turn_ends_at_ms {
                    advance_turn(lobby, now_ms);
                }
            }
        }
        self.lobbies.retain(|_, lobby| !lobby.players.is_empty());
    }
}

fn validate_avatar(avatar: &Avatar) -> Result<(), GameError> {
    match avatar {
        Avatar::Preset(index) if *index < 5 => Ok(()),
        Avatar::Preset(_) => Err(GameError::new("unknown avatar preset")),
        Avatar::Image(data) => {
            let valid_type = data.starts_with("data:image/png;base64,")
                || data.starts_with("data:image/jpeg;base64,")
                || data.starts_with("data:image/webp;base64,");
            if valid_type && data.len() <= 96 * 1024 {
                Ok(())
            } else {
                Err(GameError::new(
                    "avatar must be a small PNG, JPEG, or WebP data URL",
                ))
            }
        }
    }
}

fn new_player(token: &str, profile: &Profile, team: u8) -> Player {
    const COLORS: [&str; 4] = ["#ff4d6d", "#4cc9f0", "#ffd166", "#80ed99"];
    Player {
        token: token.to_string(),
        player_id: profile.player_id,
        nickname: profile.nickname.clone(),
        avatar: profile.avatar.clone(),
        team,
        color: COLORS[usize::from(team - 1)].to_string(),
        ready: false,
        connected: true,
        measurements: Vec::new(),
    }
}

fn valid_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
}

fn require_waiting(lobby: &Lobby) -> Result<(), GameError> {
    if matches!(lobby.phase, Phase::Waiting | Phase::Countdown { .. }) {
        Ok(())
    } else {
        Err(GameError::new(
            "this setting is locked after the game starts",
        ))
    }
}

fn require_turn(lobby: &Lobby, player_index: usize) -> Result<(), GameError> {
    let Phase::Playing(game) = &lobby.phase else {
        return Err(GameError::new("game is not running"));
    };
    if game.paused_remaining_ms.is_some() {
        return Err(GameError::new("game is paused"));
    }
    if game.turn_index != player_index {
        return Err(GameError::new("wait for your turn"));
    }
    Ok(())
}

fn cancel_ready(lobby: &mut Lobby) {
    lobby.phase = Phase::Waiting;
    for player in &mut lobby.players {
        player.ready = false;
    }
}

fn update_countdown(lobby: &mut Lobby, now_ms: u64) {
    if lobby.players.len() >= 2 && lobby.players.iter().all(|player| player.ready) {
        if !matches!(lobby.phase, Phase::Countdown { .. }) {
            lobby.phase = Phase::Countdown {
                ends_at_ms: now_ms + COUNTDOWN_MS,
            };
        }
    } else {
        lobby.phase = Phase::Waiting;
    }
}

fn start_game(lobby: &mut Lobby, now_ms: u64) {
    let mut rng = SoftRng::new(lobby.seed);
    let obstacles = generate_obstacles(&mut rng);
    let mut figures = Vec::new();
    for (player_index, player) in lobby.players.iter().enumerate() {
        for figure_index in 0..FIGURES_PER_PLAYER {
            let (x, y) = spawn_point(&mut rng, player.team, &obstacles, &figures);
            figures.push(Figure {
                id: (player_index * FIGURES_PER_PLAYER + figure_index + 1) as u32,
                owner_id: player.player_id,
                index: (figure_index + 1) as u8,
                team: player.team,
                x,
                y,
                alive: true,
            });
        }
    }
    for player in &mut lobby.players {
        player.measurements.clear();
    }
    lobby.phase = Phase::Playing(Game {
        round: 1,
        turn_index: 0,
        turn_ends_at_ms: now_ms + TURN_MS,
        paused_remaining_ms: None,
        figures,
        obstacles,
        traces: Vec::new(),
    });
}

fn generate_obstacles(rng: &mut SoftRng) -> Vec<Obstacle> {
    let mut result = Vec::new();
    for _ in 0..7 {
        result.push(Obstacle {
            x: rng.range(-310.0, 310.0),
            y: rng.range(-310.0, 310.0),
            width: rng.range(45.0, 115.0),
            depth: rng.range(35.0, 100.0),
            height: rng.range(35.0, 130.0),
        });
    }
    result
}

fn spawn_point(
    rng: &mut SoftRng,
    team: u8,
    obstacles: &[Obstacle],
    figures: &[Figure],
) -> (f32, f32) {
    let anchors = [
        (-350.0, -350.0),
        (350.0, 350.0),
        (-350.0, 350.0),
        (350.0, -350.0),
    ];
    let anchor = anchors[usize::from(team.saturating_sub(1).min(3))];
    for _ in 0..128 {
        let x = (anchor.0 + rng.range(-105.0, 105.0)).clamp(-465.0, 465.0);
        let y = (anchor.1 + rng.range(-105.0, 105.0)).clamp(-465.0, 465.0);
        let clear_obstacle = obstacles.iter().all(|obstacle| {
            (x - obstacle.x).abs() > obstacle.width * 0.5 + 24.0
                || (y - obstacle.y).abs() > obstacle.depth * 0.5 + 24.0
        });
        let clear_figure = figures
            .iter()
            .all(|figure| squared_distance(x, y, figure.x, figure.y) > 50.0 * 50.0);
        if clear_obstacle && clear_figure {
            return (x, y);
        }
    }
    anchor
}

fn set_paused(lobby: &mut Lobby, paused: bool, now_ms: u64) -> Result<(), GameError> {
    let Phase::Playing(game) = &mut lobby.phase else {
        return Err(GameError::new("game is not running"));
    };
    match (paused, game.paused_remaining_ms) {
        (true, None) => {
            game.paused_remaining_ms = Some(game.turn_ends_at_ms.saturating_sub(now_ms));
        }
        (false, Some(remaining)) => {
            game.paused_remaining_ms = None;
            game.turn_ends_at_ms = now_ms + remaining;
        }
        _ => {}
    }
    Ok(())
}

fn plot_turn(
    lobby: &mut Lobby,
    player_index: usize,
    expression: &str,
    now_ms: u64,
) -> Result<(), GameError> {
    require_turn(lobby, player_index)?;
    let expression = expression.trim();
    if expression.is_empty() || expression.len() > 160 {
        return Err(GameError::new(
            "plot expression must contain 1-160 characters",
        ));
    }
    let owner_id = lobby.players[player_index].player_id;
    let owner_team = lobby.players[player_index].team;
    let Phase::Playing(game) = &mut lobby.phase else {
        unreachable!();
    };
    let mut points = raster_plot(expression)?;
    points.retain(|point| !blocked(point, &game.obstacles));
    let pixel_points: Vec<(i32, i32)> = points.iter().map(world_to_pixel).collect();
    let mut hit_figure_ids = Vec::new();
    for figure in &mut game.figures {
        if !figure.alive || figure.team == owner_team {
            continue;
        }
        let target = world_to_pixel(&Point {
            x: figure.x,
            y: figure.y,
        });
        if pixel_points.iter().any(|point| {
            let dx = point.0 - target.0;
            let dy = point.1 - target.1;
            dx * dx + dy * dy <= HIT_RADIUS_PX * HIT_RADIUS_PX
        }) {
            figure.alive = false;
            hit_figure_ids.push(figure.id);
        }
    }
    game.traces.push(PlotTrace {
        owner_id,
        expression: expression.to_string(),
        points,
        expires_at_ms: now_ms + TRACE_MS,
        hit_figure_ids,
    });
    advance_turn(lobby, now_ms);
    Ok(())
}

fn blocked(point: &Point, obstacles: &[Obstacle]) -> bool {
    obstacles.iter().any(|obstacle| {
        (point.x - obstacle.x).abs() <= obstacle.width * 0.5
            && (point.y - obstacle.y).abs() <= obstacle.depth * 0.5
    })
}

fn normalize_turn(lobby: &mut Lobby, now_ms: u64) {
    let winner = winning_team(lobby);
    if let Some(team) = winner {
        lobby.phase = Phase::Finished {
            reason: format!("team {team} is the last team standing"),
            winner_team: Some(team),
        };
        return;
    }
    let alive: Vec<bool> = lobby
        .players
        .iter()
        .map(|player| player.connected && player_alive(lobby, player.player_id))
        .collect();
    let Phase::Playing(game) = &mut lobby.phase else {
        return;
    };
    game.turn_index %= alive.len();
    if !alive.iter().any(|alive| *alive) {
        lobby.phase = Phase::Finished {
            reason: "no players remain".to_string(),
            winner_team: None,
        };
        return;
    }
    for _ in 0..alive.len() {
        if alive[game.turn_index] {
            game.turn_ends_at_ms = now_ms + TURN_MS;
            return;
        }
        game.turn_index = (game.turn_index + 1) % alive.len();
    }
}

fn advance_turn(lobby: &mut Lobby, now_ms: u64) {
    let winner = winning_team(lobby);
    if let Some(team) = winner {
        lobby.phase = Phase::Finished {
            reason: format!("team {team} is the last team standing"),
            winner_team: Some(team),
        };
        return;
    }
    let alive: Vec<bool> = lobby
        .players
        .iter()
        .map(|player| player.connected && player_alive(lobby, player.player_id))
        .collect();
    let player_count = alive.len();
    let Phase::Playing(game) = &mut lobby.phase else {
        return;
    };
    let previous = game.turn_index;
    for step in 1..=player_count {
        let next = (previous + step) % player_count;
        if alive[next] {
            if next <= previous {
                game.round += 1;
            }
            game.turn_index = next;
            game.turn_ends_at_ms = now_ms + TURN_MS;
            game.paused_remaining_ms = None;
            lobby.players[next].measurements.clear();
            return;
        }
    }
}

fn player_alive(lobby: &Lobby, player_id: u64) -> bool {
    let Phase::Playing(game) = &lobby.phase else {
        return false;
    };
    game.figures
        .iter()
        .any(|figure| figure.owner_id == player_id && figure.alive)
}

fn winning_team(lobby: &Lobby) -> Option<u8> {
    let Phase::Playing(game) = &lobby.phase else {
        return None;
    };
    let mut teams = Vec::new();
    for figure in game.figures.iter().filter(|figure| figure.alive) {
        if !teams.contains(&figure.team) {
            teams.push(figure.team);
        }
    }
    (teams.len() == 1).then(|| teams[0])
}

fn lobby_view(lobby: &Lobby, now_ms: u64) -> LobbySnapshot {
    let game_ref = match &lobby.phase {
        Phase::Playing(game) => Some(game),
        _ => None,
    };
    let players = lobby
        .players
        .iter()
        .map(|player| PlayerView {
            player_id: player.player_id,
            nickname: player.nickname.clone(),
            avatar: player.avatar.clone(),
            team: player.team,
            color: player.color.clone(),
            ready: player.ready,
            connected: player.connected,
            alive_figures: game_ref.map_or(FIGURES_PER_PLAYER, |game| {
                game.figures
                    .iter()
                    .filter(|figure| figure.owner_id == player.player_id && figure.alive)
                    .count()
            }),
            measurements: player.measurements.clone(),
        })
        .collect();
    let (phase, game) = match &lobby.phase {
        Phase::Waiting => (PhaseView::Waiting, None),
        Phase::Countdown { ends_at_ms } => (
            PhaseView::Countdown {
                ends_at_ms: *ends_at_ms,
            },
            None,
        ),
        Phase::Playing(game) => (
            PhaseView::Playing,
            Some(GameView {
                round: game.round,
                current_player_id: lobby.players[game.turn_index].player_id,
                turn_ends_at_ms: game.turn_ends_at_ms,
                paused: game.paused_remaining_ms.is_some(),
                figures: game.figures.clone(),
                obstacles: game.obstacles.clone(),
                traces: game.traces.clone(),
            }),
        ),
        Phase::Finished {
            reason,
            winner_team,
        } => (
            PhaseView::Finished {
                reason: reason.clone(),
                winner_team: *winner_team,
            },
            None,
        ),
    };
    LobbySnapshot {
        id: lobby.id.clone(),
        name: lobby.name.clone(),
        phase,
        players,
        chat: lobby.chat.clone(),
        game,
        server_now_ms: now_ms,
    }
}

fn world_to_pixel(point: &Point) -> (i32, i32) {
    let x = ((point.x - WORLD_MIN) / (WORLD_MAX - WORLD_MIN) * f32::from(PLOT_WIDTH - 1)).round();
    let y = ((WORLD_MAX - point.y) / (WORLD_MAX - WORLD_MIN) * f32::from(PLOT_HEIGHT - 1)).round();
    (x as i32, y as i32)
}

fn squared_distance(ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let dx = ax - bx;
    let dy = ay - by;
    dx * dx + dy * dy
}

#[derive(Clone)]
enum Expr {
    Number(f64),
    X,
    Y,
    UnaryMinus(Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    Call(String, Box<Expr>),
}

impl Expr {
    fn eval(&self, x: f64, y: f64) -> f64 {
        match self {
            Self::Number(value) => *value,
            Self::X => x,
            Self::Y => y,
            Self::UnaryMinus(value) => -value.eval(x, y),
            Self::Binary(op, left, right) => {
                let a = left.eval(x, y);
                let b = right.eval(x, y);
                match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    '^' => a.powf(b),
                    _ => f64::NAN,
                }
            }
            Self::Call(name, value) => {
                let value = value.eval(x, y);
                match name.as_str() {
                    "sin" => value.sin(),
                    "cos" => value.cos(),
                    "tan" => value.tan(),
                    "abs" => value.abs(),
                    "sqrt" => value.sqrt(),
                    "ln" => value.ln(),
                    "log" => value.log10(),
                    "exp" => value.exp(),
                    "floor" => value.floor(),
                    "ceil" => value.ceil(),
                    _ => f64::NAN,
                }
            }
        }
    }
}

struct Parser<'a> {
    input: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn parse(input: &'a str) -> Result<Expr, GameError> {
        let mut parser = Self {
            input: input.as_bytes(),
            at: 0,
        };
        let expr = parser.add_sub()?;
        parser.space();
        if parser.at != parser.input.len() {
            return Err(GameError::new("unexpected text in expression"));
        }
        Ok(expr)
    }

    fn add_sub(&mut self) -> Result<Expr, GameError> {
        let mut left = self.mul_div()?;
        loop {
            self.space();
            let Some(op @ (b'+' | b'-')) = self.peek() else {
                break;
            };
            self.at += 1;
            left = Expr::Binary(op as char, Box::new(left), Box::new(self.mul_div()?));
        }
        Ok(left)
    }

    fn mul_div(&mut self) -> Result<Expr, GameError> {
        let mut left = self.power()?;
        loop {
            self.space();
            let Some(op @ (b'*' | b'/')) = self.peek() else {
                break;
            };
            self.at += 1;
            left = Expr::Binary(op as char, Box::new(left), Box::new(self.power()?));
        }
        Ok(left)
    }

    fn power(&mut self) -> Result<Expr, GameError> {
        let left = self.unary()?;
        self.space();
        if self.peek() == Some(b'^') {
            self.at += 1;
            Ok(Expr::Binary('^', Box::new(left), Box::new(self.power()?)))
        } else {
            Ok(left)
        }
    }

    fn unary(&mut self) -> Result<Expr, GameError> {
        self.space();
        if self.peek() == Some(b'-') {
            self.at += 1;
            return Ok(Expr::UnaryMinus(Box::new(self.unary()?)));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, GameError> {
        self.space();
        if self.peek() == Some(b'(') {
            self.at += 1;
            let expr = self.add_sub()?;
            self.space();
            if self.peek() != Some(b')') {
                return Err(GameError::new("missing closing parenthesis"));
            }
            self.at += 1;
            return Ok(expr);
        }
        if self
            .peek()
            .is_some_and(|byte| byte.is_ascii_digit() || byte == b'.')
        {
            return self.number();
        }
        if self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            return self.identifier();
        }
        Err(GameError::new("expected a number, variable, or function"))
    }

    fn number(&mut self) -> Result<Expr, GameError> {
        let start = self.at;
        while self.peek().is_some_and(|byte| {
            byte.is_ascii_digit() || matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-')
        }) {
            if matches!(self.peek(), Some(b'+' | b'-'))
                && self.at > start
                && !matches!(self.input[self.at - 1], b'e' | b'E')
            {
                break;
            }
            self.at += 1;
        }
        let text = core::str::from_utf8(&self.input[start..self.at])
            .map_err(|_| GameError::new("bad number"))?;
        text.parse::<f64>()
            .map(Expr::Number)
            .map_err(|_| GameError::new("bad number"))
    }

    fn identifier(&mut self) -> Result<Expr, GameError> {
        let start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let name = core::str::from_utf8(&self.input[start..self.at])
            .unwrap_or("")
            .to_ascii_lowercase();
        match name.as_str() {
            "x" => Ok(Expr::X),
            "y" => Ok(Expr::Y),
            "pi" => Ok(Expr::Number(PI)),
            "e" => Ok(Expr::Number(E)),
            "sin" | "cos" | "tan" | "abs" | "sqrt" | "ln" | "log" | "exp" | "floor" | "ceil" => {
                self.space();
                if self.peek() != Some(b'(') {
                    return Err(GameError::new("function argument must be in parentheses"));
                }
                self.at += 1;
                let value = self.add_sub()?;
                self.space();
                if self.peek() != Some(b')') {
                    return Err(GameError::new("missing closing parenthesis"));
                }
                self.at += 1;
                Ok(Expr::Call(name, Box::new(value)))
            }
            _ => Err(GameError::new(format!("unknown name '{name}'"))),
        }
    }

    fn space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.at).copied()
    }
}

fn raster_plot(input: &str) -> Result<Vec<Point>, GameError> {
    let trimmed = input.trim();
    if let Some(args) = call_args(trimmed, "circle") {
        let values = numeric_args(args, 3)?;
        let mut points = Vec::with_capacity(1440);
        for step in 0..1440 {
            let angle = f64::from(step) * PI * 2.0 / 1440.0;
            points.push(Point {
                x: (values[0] + values[2] * angle.cos()) as f32,
                y: (values[1] + values[2] * angle.sin()) as f32,
            });
        }
        return Ok(points);
    }
    if let Some(args) = call_args(trimmed, "segment") {
        let values = numeric_args(args, 4)?;
        return Ok(raster_segment(
            Point {
                x: values[0] as f32,
                y: values[1] as f32,
            },
            Point {
                x: values[2] as f32,
                y: values[3] as f32,
            },
        ));
    }
    let Some((left_text, right_text)) = trimmed.split_once('=') else {
        let expr = Parser::parse(trimmed)?;
        return Ok(sample_function(&expr, true));
    };
    let left_text = left_text.trim();
    let right_text = right_text.trim();
    if left_text.eq_ignore_ascii_case("y") {
        return Ok(sample_function(&Parser::parse(right_text)?, true));
    }
    if left_text.eq_ignore_ascii_case("x") {
        return Ok(sample_function(&Parser::parse(right_text)?, false));
    }
    let left = Parser::parse(left_text)?;
    let right = Parser::parse(right_text)?;
    Ok(sample_implicit(&left, &right))
}

fn call_args<'a>(input: &'a str, name: &str) -> Option<&'a str> {
    input
        .strip_prefix(name)
        .and_then(|tail| tail.strip_prefix('('))
        .and_then(|tail| tail.strip_suffix(')'))
}

fn numeric_args(input: &str, expected: usize) -> Result<Vec<f64>, GameError> {
    let values: Result<Vec<f64>, GameError> = input
        .split(',')
        .map(|part| {
            let expr = Parser::parse(part)?;
            let value = expr.eval(0.0, 0.0);
            value
                .is_finite()
                .then_some(value)
                .ok_or_else(|| GameError::new("shape arguments must be finite constants"))
        })
        .collect();
    let values = values?;
    if values.len() != expected {
        return Err(GameError::new(format!(
            "expected {expected} shape arguments"
        )));
    }
    Ok(values)
}

fn sample_function(expr: &Expr, y_of_x: bool) -> Vec<Point> {
    let samples = if y_of_x { PLOT_WIDTH } else { PLOT_HEIGHT };
    let mut result = Vec::with_capacity(usize::from(samples));
    for index in 0..samples {
        let independent = f64::from(WORLD_MIN)
            + f64::from(index) / f64::from(samples - 1) * f64::from(WORLD_MAX - WORLD_MIN);
        let dependent = if y_of_x {
            expr.eval(independent, 0.0)
        } else {
            expr.eval(0.0, independent)
        };
        if dependent.is_finite()
            && (f64::from(WORLD_MIN)..=f64::from(WORLD_MAX)).contains(&dependent)
        {
            result.push(if y_of_x {
                Point {
                    x: independent as f32,
                    y: dependent as f32,
                }
            } else {
                Point {
                    x: dependent as f32,
                    y: independent as f32,
                }
            });
        }
    }
    result
}

fn sample_implicit(left: &Expr, right: &Expr) -> Vec<Point> {
    const COLS: usize = 480;
    const ROWS: usize = 270;
    let mut result = Vec::new();
    let value = |x: f64, y: f64| left.eval(x, y) - right.eval(x, y);
    for row in 0..ROWS {
        let y = f64::from(WORLD_MIN)
            + row as f64 / (ROWS - 1) as f64 * f64::from(WORLD_MAX - WORLD_MIN);
        let mut previous_x = f64::from(WORLD_MIN);
        let mut previous = value(previous_x, y);
        for col in 1..COLS {
            let x = f64::from(WORLD_MIN)
                + col as f64 / (COLS - 1) as f64 * f64::from(WORLD_MAX - WORLD_MIN);
            let current = value(x, y);
            if previous.is_finite()
                && current.is_finite()
                && (previous == 0.0 || current == 0.0 || previous.signum() != current.signum())
            {
                let denominator = previous.abs() + current.abs();
                let mix = if denominator == 0.0 {
                    0.5
                } else {
                    previous.abs() / denominator
                };
                result.push(Point {
                    x: (previous_x + (x - previous_x) * mix) as f32,
                    y: y as f32,
                });
            }
            previous_x = x;
            previous = current;
        }
    }
    result
}

fn raster_segment(start: Point, end: Point) -> Vec<Point> {
    let a = world_to_pixel(&start);
    let b = world_to_pixel(&end);
    let steps = (a.0 - b.0).abs().max((a.1 - b.1).abs()).max(1);
    (0..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            Point {
                x: start.x + (end.x - start.x) * t,
                y: start.y + (end.y - start.y) * t,
            }
        })
        .collect()
}

struct SoftRng(u64);

impl SoftRng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut value = self.0;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.0 = value;
        value
    }

    fn range(&mut self, min: f32, max: f32) -> f32 {
        let unit = (self.next() >> 40) as f32 / ((1u32 << 24) - 1) as f32;
        min + unit * (max - min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connect(game: &mut PlotTwist, name: &str) -> Session {
        game.connect(ConnectRequest {
            nickname: name.to_string(),
            avatar: Avatar::Preset(0),
        })
        .unwrap()
    }

    #[test]
    fn countdown_cancels_and_restarts_from_ten_seconds() {
        let mut game = PlotTwist::new(7);
        let a = connect(&mut game, "Ada");
        let b = connect(&mut game, "Bob");
        let lobby = game.create_lobby(&a.token, 0).unwrap();
        game.join_lobby(&lobby.id, &b.token, 0).unwrap();
        game.act(&lobby.id, &a.token, Action::SetReady { ready: true }, 10)
            .unwrap();
        let started = game
            .act(&lobby.id, &b.token, Action::SetReady { ready: true }, 20)
            .unwrap()
            .snapshot
            .unwrap();
        assert!(matches!(
            started.phase,
            PhaseView::Countdown { ends_at_ms: 10_020 }
        ));
        game.act(&lobby.id, &a.token, Action::SetReady { ready: false }, 500)
            .unwrap();
        let reset = game
            .act(&lobby.id, &a.token, Action::SetReady { ready: true }, 1_000)
            .unwrap()
            .snapshot
            .unwrap();
        assert!(matches!(
            reset.phase,
            PhaseView::Countdown { ends_at_ms: 11_000 }
        ));
    }

    #[test]
    fn lobby_is_freed_when_last_player_leaves() {
        let mut game = PlotTwist::new(7);
        let player = connect(&mut game, "Ada");
        let lobby = game.create_lobby(&player.token, 0).unwrap();
        game.act(&lobby.id, &player.token, Action::Leave, 0)
            .unwrap();
        assert!(game.lobbies(0).is_empty());
    }

    #[test]
    fn parser_supports_functions_vertical_and_implicit_plots() {
        assert!(raster_plot("y = sin(x / 30) * 100").unwrap().len() > 1_000);
        assert!(raster_plot("x = y^2 / 100").unwrap().len() > 400);
        assert!(!raster_plot("x^2 + y^2 = 10000").unwrap().is_empty());
        assert!(raster_plot("circle(0, 0, 100)").unwrap().len() > 1_000);
    }

    #[test]
    fn team_color_changes_every_teammate() {
        let mut game = PlotTwist::new(7);
        let a = connect(&mut game, "Ada");
        let b = connect(&mut game, "Bob");
        let lobby = game.create_lobby(&a.token, 0).unwrap();
        game.join_lobby(&lobby.id, &b.token, 0).unwrap();
        game.act(&lobby.id, &b.token, Action::SetTeam { team: 1 }, 0)
            .unwrap();
        let view = game
            .act(
                &lobby.id,
                &a.token,
                Action::SetColor {
                    color: "#123ABC".to_string(),
                },
                0,
            )
            .unwrap()
            .snapshot
            .unwrap();
        assert!(view.players.iter().all(|player| player.color == "#123ABC"));
    }

    #[test]
    fn two_players_start_with_four_figures_and_a_plot_hits_an_enemy() {
        let mut game = PlotTwist::new(41);
        let a = connect(&mut game, "Ada");
        let b = connect(&mut game, "Bob");
        let lobby = game.create_lobby(&a.token, 0).unwrap();
        game.join_lobby(&lobby.id, &b.token, 0).unwrap();
        game.act(&lobby.id, &a.token, Action::SetReady { ready: true }, 0)
            .unwrap();
        game.act(&lobby.id, &b.token, Action::SetReady { ready: true }, 0)
            .unwrap();

        let started = game.snapshot(&lobby.id, &a.token, COUNTDOWN_MS).unwrap();
        let started_game = started.game.unwrap();
        assert_eq!(started_game.figures.len(), 8);
        assert_eq!(started_game.current_player_id, a.player_id);
        assert_eq!(started_game.turn_ends_at_ms, COUNTDOWN_MS + TURN_MS);

        let target = started_game
            .figures
            .iter()
            .find(|figure| figure.owner_id == b.player_id)
            .unwrap();
        let expression = format!(
            "segment({},{},{},{})",
            target.x, target.y, target.x, target.y
        );
        let after = game
            .act(
                &lobby.id,
                &a.token,
                Action::Plot { expression },
                COUNTDOWN_MS + 1,
            )
            .unwrap()
            .snapshot
            .unwrap();
        assert_eq!(
            after
                .players
                .iter()
                .find(|player| player.player_id == b.player_id)
                .unwrap()
                .alive_figures,
            3
        );
    }
}
