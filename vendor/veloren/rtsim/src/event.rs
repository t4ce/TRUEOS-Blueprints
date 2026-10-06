use crate::{RtState, Rule, ai::ActorSystemData};
use common::{
    mounting::VolumePos,
    resources::{Time, TimeOfDay},
    rtsim::{ActorId, SiteId},
    terrain::SpriteKind,
};
use vek::*;
use world::{IndexRef, World};

pub trait Event: Clone + 'static {
    type SystemData<'a>;
}

pub struct EventCtx<'a, 'd, R: Rule, E: Event> {
    pub state: &'a RtState,
    pub rule: &'a mut R,
    pub event: &'a E,
    pub world: &'a World,
    pub index: IndexRef<'a>,
    pub system_data: &'a mut E::SystemData<'d>,
}

#[derive(Clone)]
pub struct OnSetup;
impl Event for OnSetup {
    type SystemData<'a> = ();
}

#[derive(Clone)]
pub struct OnTick {
    pub time_of_day: TimeOfDay,
    pub time: Time,
    pub tick: u64,
    pub dt: f32,
}
impl Event for OnTick {
    type SystemData<'a> = ActorSystemData<'a>;
}

#[derive(Clone)]
pub struct OnDeath {
    pub actor: ActorId,
    pub wpos: Option<Vec3<f32>>,
    pub killer: Option<ActorId>,
}
impl Event for OnDeath {
    type SystemData<'a> = ();
}

#[derive(Clone)]
pub struct OnHelped {
    pub actor: ActorId,
    pub saver: Option<ActorId>,
}
impl Event for OnHelped {
    type SystemData<'a> = ();
}

#[derive(Clone)]
pub struct OnHealthChange {
    pub actor: ActorId,
    pub cause: Option<ActorId>,
    pub new_health_fraction: f32,
    pub change: f32,
}
impl Event for OnHealthChange {
    type SystemData<'a> = ();
}

#[derive(Clone)]
pub struct OnTheft {
    pub actor: ActorId,
    pub wpos: Vec3<i32>,
    pub sprite: SpriteKind,
    pub site: Option<SiteId>,
}

impl Event for OnTheft {
    type SystemData<'a> = ();
}

#[derive(Clone)]
pub struct OnMountVolume {
    pub actor: ActorId,
    pub pos: VolumePos<ActorId>,
}
impl Event for OnMountVolume {
    type SystemData<'a> = ();
}
