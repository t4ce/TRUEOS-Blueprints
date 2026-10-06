use crate::{
    ai::Action,
    data::{Reports, Sentiments, quest::Quest},
    generate::name,
};
pub use common::rtsim::{ActorId, Profession};
use common::{
    character::CharacterId,
    comp::{self, agent::FlightMode, item::ItemDef},
    grid::Grid,
    map::Marker,
    resources::{Time, TimeOfDay},
    rtsim::{
        Dialogue, DialogueId, DialogueKind, FactionId, NpcAction, NpcActivity, NpcInput, NpcMsg,
        Personality, QuestId, ReportId, Response, Role, SiteId, TerrainResource,
    },
    store::Id,
    terrain::CoordinateConversions,
    time::DayPeriod,
    util::Dir,
};
use hashbrown::{HashMap, HashSet};
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use slotmap::DenseSlotMap;
use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};
use tracing::error;
use vek::*;
use world::{
    civ::{Track, airship_travel::AirshipFlightPhase},
    site::Site as WorldSite,
    util::{LOCALITY, RandomPerm},
};

#[derive(Copy, Clone, Debug, Default)]
pub enum SimulationMode {
    /// The NPC is unloaded and is being simulated via rtsim.
    #[default]
    Simulated,
    /// The NPC has been loaded into the game world as an ECS entity.
    Loaded,
}

#[derive(Clone)]
pub struct PathData<P, N> {
    pub end: N,
    pub path: VecDeque<P>,
    pub repoll: bool,
}

#[derive(Clone, Default)]
pub struct PathingMemory {
    pub intrasite_path: Option<(PathData<Vec2<i32>, Vec2<i32>>, Id<WorldSite>)>,
    pub intersite_path: Option<(PathData<(Id<Track>, bool), SiteId>, usize)>,
}

#[derive(Default)]
pub struct Controller {
    pub actions: Vec<NpcAction>,
    pub activity: Option<NpcActivity>,
    pub new_home: Option<Option<SiteId>>,
    pub look_dir: Option<Dir>,
    pub job: Option<Job>,
    pub quests_to_create: Vec<(QuestId, Quest)>,

    /// Each pilot gets assigned to a route, and as the server ticks onward, the
    /// current leg of each pilot's assigned route increments. This gets
    /// periodically updated to allow the current leg that a given NPC is on to
    /// be retrieved, which is necessary for players to be able to ask their
    /// captain where they're currently headed.
    ///
    /// NOTE: Do not put other arbitrary data into `Controller` without proper
    /// consideration/discussion regarding where it should go. This will soon be
    /// refactored into the Job data for the NPC.
    pub current_airship_pilot_leg: Option<(usize, AirshipFlightPhase)>,
}

impl Controller {
    /// Reset the controller to a neutral state before the start of the next
    /// brain tick.
    pub fn reset(&mut self, npc: &Npc) {
        self.activity = None;
        self.look_dir = None;
        self.job = npc.job.clone();
    }

    pub fn do_idle(&mut self) { self.activity = None; }

    pub fn do_talk(&mut self, tgt: ActorId) { self.activity = Some(NpcActivity::Talk(tgt)); }

    pub fn do_goto(&mut self, wpos: Vec3<f32>, speed_factor: f32) {
        self.activity = Some(NpcActivity::Goto(wpos, speed_factor));
    }

    /// go to with height above terrain and direction
    pub fn do_goto_with_height_and_dir(
        &mut self,
        wpos: Vec3<f32>,
        speed_factor: f32,
        height: Option<f32>,
        dir: Option<Dir>,
        flight_mode: FlightMode,
    ) {
        self.activity = Some(NpcActivity::GotoFlying(
            wpos,
            speed_factor,
            height,
            dir,
            flight_mode,
        ));
    }

    pub fn do_gather(&mut self, resources: &'static [TerrainResource]) {
        self.activity = Some(NpcActivity::Gather(resources));
    }

    pub fn do_hunt_animals(&mut self) { self.activity = Some(NpcActivity::HuntAnimals); }

    pub fn do_dance(&mut self, dir: Option<Dir>) { self.activity = Some(NpcActivity::Dance(dir)); }

    pub fn do_cheer(&mut self, dir: Option<Dir>) { self.activity = Some(NpcActivity::Cheer(dir)); }

    pub fn do_sit(&mut self, dir: Option<Dir>, pos: Option<Vec3<i32>>) {
        self.activity = Some(NpcActivity::Sit(dir, pos));
    }

    pub fn say(&mut self, target: impl Into<Option<ActorId>>, content: comp::Content) {
        self.actions.push(NpcAction::Say(target.into(), content));
    }

    pub fn attack(&mut self, target: ActorId) { self.actions.push(NpcAction::Attack(target)); }

    pub fn set_new_home(&mut self, new_home: impl Into<Option<SiteId>>) {
        self.new_home = Some(new_home.into());
    }

    pub fn set_newly_hired(&mut self, actor: ActorId, expires: Time) {
        self.job = Some(Job::Hired(actor, expires));
    }

    pub fn end_hiring(&mut self) {
        if matches!(self.job, Some(Job::Hired(..))) {
            self.job = None;
        }
    }

    pub fn end_quest(&mut self) {
        if matches!(self.job, Some(Job::Quest(..))) {
            self.job = None;
        }
    }

    pub fn send_msg(&mut self, to: ActorId, msg: NpcMsg) {
        self.actions.push(NpcAction::Msg { to, msg });
    }

    /// Start a new dialogue.
    pub fn dialogue_start(&mut self, target: ActorId) -> DialogueSession {
        let session = DialogueSession {
            target,
            id: DialogueId(rand::rng().random()),
        };

        self.actions.push(NpcAction::Dialogue(target, Dialogue {
            id: session.id,
            kind: DialogueKind::Start,
        }));

        session
    }

    /// End an existing dialogue.
    pub fn dialogue_end(&mut self, session: DialogueSession) {
        self.actions
            .push(NpcAction::Dialogue(session.target, Dialogue {
                id: session.id,
                kind: DialogueKind::End,
            }));
    }

    pub fn dialogue_response(
        &mut self,
        session: DialogueSession,
        tag: u32,
        response: &(u16, Response),
    ) {
        self.actions
            .push(NpcAction::Dialogue(session.target, Dialogue {
                id: session.id,
                kind: DialogueKind::Response {
                    tag,
                    response: response.1.clone(),
                    response_id: response.0,
                },
            }));
    }

    fn new_dialogue_tag(&self) -> u32 {
        static TAG_COUNTER: AtomicU32 = AtomicU32::new(0);
        TAG_COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Ask a question, with various possible answers. Returns the dialogue tag,
    /// used for identifying the answer.
    pub fn dialogue_question(
        &mut self,
        session: DialogueSession,
        msg: comp::Content,
        responses: impl IntoIterator<Item = (u16, Response)>,
    ) -> u32 {
        let tag = self.new_dialogue_tag();

        self.actions
            .push(NpcAction::Dialogue(session.target, Dialogue {
                id: session.id,
                kind: DialogueKind::Question {
                    tag,
                    msg,
                    responses: responses.into_iter().collect(),
                },
            }));

        tag
    }

    /// Provide a statement as part of a dialogue. Returns the dialogue tag,
    /// used for identifying acknowledgements.
    pub fn dialogue_statement(
        &mut self,
        session: DialogueSession,
        msg: comp::Content,
        given_item: Option<(Arc<ItemDef>, u32)>,
    ) -> u32 {
        let tag = self.new_dialogue_tag();

        self.actions
            .push(NpcAction::Dialogue(session.target, Dialogue {
                id: session.id,
                kind: DialogueKind::Statement {
                    msg,
                    given_item,
                    tag,
                },
            }));

        tag
    }

    /// Provide a location marker as part of a dialogue.
    pub fn dialogue_marker(&mut self, session: DialogueSession, marker: Marker) {
        self.actions
            .push(NpcAction::Dialogue(session.target, Dialogue {
                id: session.id,
                kind: DialogueKind::Marker(marker),
            }));
    }
}

// Represents an ongoing dialogue with another actor.
#[derive(Copy, Clone)]
pub struct DialogueSession {
    pub target: ActorId,
    pub id: DialogueId,
}

pub struct Brain {
    pub action: Box<dyn Action<(), !>>,
}

#[derive(Serialize, Deserialize)]
pub struct Npc {
    /// The [`crate::data::Report`]s that the NPC is aware of.
    pub known_reports: HashSet<ReportId>,

    #[serde(default)]
    pub personality: Personality,
    #[serde(default)]
    pub sentiments: Sentiments,

    #[serde(default)]
    pub job: Option<Job>,

    #[serde(skip)]
    pub controller: Controller,
    #[serde(skip)]
    pub inbox: VecDeque<NpcInput>,

    #[serde(skip)]
    pub brain: Option<Brain>,
}

#[derive(Serialize, Deserialize)]
pub struct Character {
    pub id: CharacterId,
    // The tick on which the character was last present. If this value falls behind the global
    // rtsim tick, we assume the character has logged off and remove its presence
    pub last_present_at: Option<u64>,
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize, Deserialize)]
pub enum ActorKind {
    Npc(Npc),
    Character(Character),
}

#[derive(Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,

    // Persisted state
    pub seed: u32,
    /// Represents the location of the NPC.
    pub wpos: Vec3<f32>,
    pub dir: Vec2<f32>,

    pub body: comp::Body,
    pub role: Role,
    pub home: Option<SiteId>,
    pub faction: Option<FactionId>,
    pub presence: Option<Presence>,

    // Unpersisted state
    #[serde(skip)]
    pub chunk_pos: Option<Vec2<i32>>,
    #[serde(skip)]
    pub current_site: Option<SiteId>,

    /// Whether the actor is in simulated or loaded mode (when rtsim is run on
    /// the server, loaded corresponds to being within a loaded chunk). When
    /// in loaded mode, the interactions of the actor should not be
    /// simulated but should instead be derived from the game.
    #[serde(skip)]
    pub mode: SimulationMode,
}

/// A job is a long-running, persistent, non-stackable occupation that an NPC
/// must persistently attend to, but may be temporarily interrupted from. NPCs
/// will recurrently attempt to perform tasks that relate to their job.
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub enum Job {
    /// An NPC can temporarily become a hired hand (`(hiring_actor,
    /// termination_time)`).
    Hired(ActorId, Time),
    /// NPC is helping to perform a quest
    Quest(QuestId),
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Presence {
    /// The current health of the NPC, < 0.0 is dead and 1.0 is max.
    pub health_fraction: f32,
}

impl Clone for Actor {
    fn clone(&self) -> Self {
        Self {
            kind: match &self.kind {
                ActorKind::Npc(npc) => ActorKind::Npc(Npc {
                    known_reports: npc.known_reports.clone(),
                    personality: npc.personality,
                    sentiments: npc.sentiments.clone(),
                    job: npc.job.clone(),
                    controller: Default::default(),
                    inbox: Default::default(),
                    brain: Default::default(),
                }),
                ActorKind::Character(c) => ActorKind::Character(Character {
                    id: c.id,
                    last_present_at: c.last_present_at,
                }),
            },
            seed: self.seed,
            wpos: self.wpos,
            dir: self.dir,
            role: self.role.clone(),
            home: self.home,
            faction: self.faction,
            presence: self.presence.clone(),
            body: self.body,
            // Not persisted
            chunk_pos: None,
            current_site: Default::default(),
            mode: Default::default(),
        }
    }
}

impl Actor {
    pub const PERM_ENTITY_CONFIG: u32 = 1;
    const PERM_NAME: u32 = 0;
    const PERM_TIME: u32 = 2;

    pub fn new_npc(seed: u32, wpos: Vec3<f32>, body: comp::Body, role: Role) -> Self {
        Self {
            kind: ActorKind::Npc(Npc {
                personality: Default::default(),
                sentiments: Default::default(),
                job: None,
                known_reports: Default::default(),
                controller: Default::default(),
                inbox: Default::default(),
                brain: None,
            }),
            seed,
            wpos,
            dir: Vec2::unit_x(),
            body,
            role,
            home: None,
            faction: None,
            presence: Some(Presence {
                health_fraction: 1.0,
            }),
            chunk_pos: None,
            current_site: None,
            mode: SimulationMode::Simulated,
        }
    }

    pub fn npc(&self) -> Option<&Npc> {
        match &self.kind {
            ActorKind::Npc(npc) => Some(npc),
            _ => None,
        }
    }

    pub fn npc_mut(&mut self) -> Option<&mut Npc> {
        match &mut self.kind {
            ActorKind::Npc(npc) => Some(npc),
            _ => None,
        }
    }

    pub fn new_character(
        id: CharacterId,
        seed: u32,
        wpos: Vec3<f32>,
        body: comp::Body,
        mode: SimulationMode,
    ) -> Self {
        Self {
            kind: ActorKind::Character(Character {
                id,
                last_present_at: None,
            }),
            seed,
            wpos,
            dir: Vec2::unit_x(),
            body,
            role: Role::Civilised(None),
            home: None,
            faction: None,
            // The server will give the actor a presence by itself
            presence: None,
            chunk_pos: None,
            current_site: None,
            mode,
        }
    }

    pub fn character(&self) -> Option<&Character> {
        match &self.kind {
            ActorKind::Character(character) => Some(character),
            _ => None,
        }
    }

    pub fn character_mut(&mut self) -> Option<&mut Character> {
        match &mut self.kind {
            ActorKind::Character(character) => Some(character),
            _ => None,
        }
    }

    pub fn is_present_and_alive(&self) -> bool {
        self.presence
            .as_ref()
            .map_or(false, |p| p.health_fraction > 0.0)
    }

    pub fn is_present_and_dead(&self) -> bool {
        self.presence
            .as_ref()
            .map_or(false, |p| p.health_fraction <= 0.0)
    }

    // TODO: have a dedicated `NpcBuilder` type for this.
    pub fn with_personality(mut self, personality: Personality) -> Self {
        if let ActorKind::Npc(npc) = &mut self.kind {
            npc.personality = personality;
        } else {
            panic!("Cannot set personality for non-NPC");
        }
        self
    }

    // // TODO: have a dedicated `NpcBuilder` type for this.
    // pub fn with_profession(mut self, profession: impl Into<Option<Profession>>)
    // -> Self {     if let Role::Humanoid(p) = &mut self.role {
    //         *p = profession.into();
    //     } else {
    //         panic!("Tried to assign profession {:?} to NPC, but has role {:?},
    // which cannot have a profession", profession.into(), self.role);     }
    //     self
    // }

    // TODO: have a dedicated `NpcBuilder` type for this.
    pub fn with_home(mut self, home: impl Into<Option<SiteId>>) -> Self {
        self.home = home.into();
        self
    }

    // TODO: have a dedicated `NpcBuilder` type for this.
    pub fn with_faction(mut self, faction: impl Into<Option<FactionId>>) -> Self {
        self.faction = faction.into();
        self
    }

    pub fn rng(&self, perm: u32) -> impl Rng + use<> {
        RandomPerm::new(self.seed.wrapping_add(perm))
    }

    // TODO: Don't make this depend on deterministic RNG, actually persist names
    // once we've decided that we want to
    pub fn get_name(&self) -> Option<String> {
        if let comp::Body::Humanoid(_) = &self.body {
            Some(name::generate_npc(&mut self.rng(Self::PERM_NAME)))
        } else {
            None
        }
    }

    pub fn get_day_period(&self, time_of_day: TimeOfDay) -> DayPeriod {
        let offset = self.rng(Self::PERM_TIME).random_range(-3600.0..3600.0);
        DayPeriod::from(time_of_day.0 + offset)
    }

    pub fn profession(&self) -> Option<Profession> {
        match &self.role {
            Role::Civilised(profession) => *profession,
            Role::Monster | Role::Wild | Role::Vehicle => None,
        }
    }

    pub fn hired(&self) -> Option<(ActorId, Time)> {
        if let Some(Job::Hired(actor, time)) = self.npc()?.job {
            Some((actor, time))
        } else {
            None
        }
    }

    pub fn cleanup(&mut self, reports: &Reports) {
        if let ActorKind::Npc(npc) = &mut self.kind {
            // Clear old or superfluous sentiments
            // TODO: It might be worth giving more important NPCs a higher sentiment
            // 'budget' than less important ones.
            npc.sentiments
                .cleanup(crate::data::sentiment::NPC_MAX_SENTIMENTS);
            // Clear reports that have been forgotten
            npc.known_reports
                .retain(|report| reports.contains_key(*report));
            // TODO: Limit number of reports
            // TODO: Clear old inbox items
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
pub struct GridCell {
    pub actors: Vec<ActorId>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ActorLink {
    pub mount: ActorId,
    pub rider: ActorId,
    pub is_steering: bool,
}

#[derive(Clone, Default, Serialize, Deserialize)]
struct Riders {
    steerer: Option<MountId>,
    riders: Vec<MountId>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(
    from = "DenseSlotMap<MountId, ActorLink>",
    into = "DenseSlotMap<MountId, ActorLink>"
)]
pub struct ActorLinks {
    links: DenseSlotMap<MountId, ActorLink>,
    mount_map: slotmap::SecondaryMap<ActorId, Riders>,
    rider_map: HashMap<ActorId, MountId>,
}

impl ActorLinks {
    pub fn remove_mount(&mut self, mount: ActorId) {
        if let Some(riders) = self.mount_map.remove(mount) {
            for link in riders
                .riders
                .into_iter()
                .chain(riders.steerer)
                .filter_map(|link_id| self.links.get(link_id))
            {
                self.rider_map.remove(&link.rider);
            }
        }
    }

    /// Internal function, only removes from `mount_map`.
    fn remove_rider(&mut self, id: MountId, link: &ActorLink) {
        if let Some(riders) = self.mount_map.get_mut(link.mount) {
            if link.is_steering && riders.steerer == Some(id) {
                riders.steerer = None;
            } else if let Some((i, _)) = riders.riders.iter().enumerate().find(|(_, i)| **i == id) {
                riders.riders.remove(i);
            }

            if riders.steerer.is_none() && riders.riders.is_empty() {
                self.mount_map.remove(link.mount);
            }
        }
    }

    pub fn remove_link(&mut self, link_id: MountId) {
        if let Some(link) = self.links.remove(link_id) {
            self.rider_map.remove(&link.rider);
            self.remove_rider(link_id, &link);
        }
    }

    pub fn dismount(&mut self, rider: ActorId) {
        if let Some(id) = self.rider_map.remove(&rider)
            && let Some(link) = self.links.remove(id)
        {
            self.remove_rider(id, &link);
        }
    }

    // This is the only function to actually add a mount link.
    // And it ensures that there isn't link chaining
    pub fn add_mounting(
        &mut self,
        mount: ActorId,
        rider: ActorId,
        steering: bool,
    ) -> Result<MountId, MountingError> {
        if mount == rider {
            return Err(MountingError::MountSelf);
        }
        if self.mount_map.contains_key(rider) {
            return Err(MountingError::RiderIsMounted);
        }
        if self.rider_map.contains_key(&mount) {
            return Err(MountingError::MountIsRiding);
        }
        if let Some(mount_entry) = self.mount_map.entry(mount) {
            if let hashbrown::hash_map::Entry::Vacant(rider_entry) = self.rider_map.entry(rider) {
                let riders = mount_entry.or_insert(Riders::default());

                if steering {
                    if riders.steerer.is_none() {
                        let id = self.links.insert(ActorLink {
                            mount,
                            rider,
                            is_steering: true,
                        });
                        riders.steerer = Some(id);
                        rider_entry.insert(id);
                        Ok(id)
                    } else {
                        Err(MountingError::HasSteerer)
                    }
                } else {
                    // TODO: Maybe have some limit on the number of riders depending on the mount?
                    let id = self.links.insert(ActorLink {
                        mount,
                        rider,
                        is_steering: false,
                    });
                    riders.riders.push(id);
                    rider_entry.insert(id);
                    Ok(id)
                }
            } else {
                Err(MountingError::AlreadyRiding)
            }
        } else {
            Err(MountingError::MountDead)
        }
    }

    pub fn steer(&mut self, mount: ActorId, rider: ActorId) -> Result<MountId, MountingError> {
        self.add_mounting(mount, rider, true)
    }

    pub fn ride(&mut self, mount: ActorId, rider: ActorId) -> Result<MountId, MountingError> {
        self.add_mounting(mount, rider, false)
    }

    pub fn get_mount_link(&self, rider: ActorId) -> Option<&ActorLink> {
        self.rider_map
            .get(&rider)
            .and_then(|link| self.links.get(*link))
    }

    pub fn get_steerer_link(&self, mount: ActorId) -> Option<&ActorLink> {
        self.mount_map
            .get(mount)
            .and_then(|mount| self.links.get(mount.steerer?))
    }

    pub fn get(&self, id: MountId) -> Option<&ActorLink> { self.links.get(id) }

    pub fn ids(&self) -> impl Iterator<Item = MountId> + '_ { self.links.keys() }

    pub fn iter(&self) -> impl Iterator<Item = &ActorLink> + '_ { self.links.values() }

    pub fn iter_mounts(&self) -> impl Iterator<Item = ActorId> + '_ { self.mount_map.keys() }
}

impl From<DenseSlotMap<MountId, ActorLink>> for ActorLinks {
    fn from(mut value: DenseSlotMap<MountId, ActorLink>) -> Self {
        let mut from_map = slotmap::SecondaryMap::new();
        let mut to_map = HashMap::with_capacity(value.len());
        let mut delete = Vec::new();
        for (id, link) in value.iter() {
            if let Some(entry) = from_map.entry(link.mount) {
                let riders = entry.or_insert(Riders::default());
                if link.is_steering {
                    if let Some(old) = riders.steerer.replace(id) {
                        error!("Replaced steerer {old:?} with {id:?}");
                    }
                } else {
                    riders.riders.push(id);
                }
            } else {
                delete.push(id);
            }
            to_map.insert(link.rider, id);
        }
        for id in delete {
            value.remove(id);
        }
        Self {
            links: value,
            mount_map: from_map,
            rider_map: to_map,
        }
    }
}

impl From<ActorLinks> for DenseSlotMap<MountId, ActorLink> {
    fn from(other: ActorLinks) -> Self { other.links }
}
slotmap::new_key_type! {
    pub struct MountId;
}

#[derive(Clone, Serialize, Deserialize)]
pub struct MountData {
    is_steering: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Actors {
    pub actors: DenseSlotMap<ActorId, Actor>,
    pub mounts: ActorLinks,
    // TODO: This feels like it should be its own rtsim resource
    // TODO: Consider switching to `common::util::SpatialGrid` instead
    #[serde(skip, default = "construct_actor_grid")]
    pub actor_grid: Grid<GridCell>,
}

impl Default for Actors {
    fn default() -> Self {
        Self {
            actors: Default::default(),
            mounts: Default::default(),
            actor_grid: construct_actor_grid(),
        }
    }
}

fn construct_actor_grid() -> Grid<GridCell> { Grid::new(Vec2::zero(), Default::default()) }

#[derive(Debug)]
pub enum MountingError {
    MountDead,
    RiderDead,
    HasSteerer,
    AlreadyRiding,
    MountIsRiding,
    RiderIsMounted,
    MountSelf,
}

impl Actors {
    pub fn create_actor(&mut self, actor: Actor) -> ActorId { self.actors.insert(actor) }

    /// Queries nearby npcs, not garantueed to work if radius > 32.0
    // TODO: Find a more efficient way to implement this, it's currently
    // (theoretically) O(n^2).
    pub fn nearby(
        &self,
        this_actor: Option<ActorId>,
        wpos: Vec3<f32>,
        radius: f32,
    ) -> impl Iterator<Item = ActorId> + '_ {
        let chunk_pos = wpos.xy().as_().wpos_to_cpos();
        let r_sqr = radius * radius;
        LOCALITY
            .into_iter()
            .flat_map(move |neighbor| {
                self.actor_grid.get(chunk_pos + neighbor).map(move |cell| {
                    cell.actors.iter().copied().filter(move |actor_id| {
                        self.actors.get(*actor_id).is_some_and(|actor| {
                            actor.presence.is_some() && actor.wpos.distance_squared(wpos) < r_sqr
                        }) && Some(*actor_id) != this_actor
                    })
                })
            })
            .flatten()
    }
}

impl Deref for Actors {
    type Target = DenseSlotMap<ActorId, Actor>;

    fn deref(&self) -> &Self::Target { &self.actors }
}

impl DerefMut for Actors {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.actors }
}
