use crate::{
    RtState, Rule, RuleError,
    data::{
        Sentiment,
        actor::{ActorKind, SimulationMode},
    },
    event::{EventCtx, OnHealthChange, OnHelped, OnMountVolume, OnTick},
};
use common::{
    comp::{self, Body, agent::FlightMode},
    mounting::{Volume, VolumePos},
    rtsim::{NpcAction, NpcActivity},
    terrain::{CoordinateConversions, TerrainChunkSize},
    vol::RectVolSize,
};
use slotmap::SecondaryMap;
use vek::{Clamp, Vec2};

pub struct SimulateNpcs;

impl Rule for SimulateNpcs {
    fn start(rtstate: &mut RtState) -> Result<Self, RuleError> {
        rtstate.bind(on_helped);
        rtstate.bind(on_health_changed);
        rtstate.bind(on_mount_volume);
        rtstate.bind(on_tick);

        Ok(Self)
    }
}

fn on_mount_volume(ctx: EventCtx<SimulateNpcs, OnMountVolume>) {
    let data = &mut *ctx.state.data_mut();

    // TODO: Add actor to riders.
    if let VolumePos {
        kind: Volume::Entity(vehicle),
        ..
    } = ctx.event.pos
        && let Some(link) = data.actors.mounts.get_steerer_link(vehicle)
        && let Some(driver) = data.actors.actors.get_mut(link.rider)
        && let Some(driver_npc) = driver.npc_mut()
    {
        driver_npc.controller.actions.push(NpcAction::Say(
            Some(ctx.event.actor),
            comp::Content::localized("npc-speech-welcome-aboard"),
        ))
    }
}

fn on_health_changed(ctx: EventCtx<SimulateNpcs, OnHealthChange>) {
    let data = &mut *ctx.state.data_mut();

    if let Some(cause) = ctx.event.cause
        && let Some(actor) = data.actors.get_mut(ctx.event.actor)
        && let Some(npc) = actor.npc_mut()
    {
        if ctx.event.change < 0.0 {
            npc.sentiments
                .toward_mut(cause)
                .change_by(-0.1, Sentiment::ENEMY);
        } else if ctx.event.change > 0.0 {
            npc.sentiments
                .toward_mut(cause)
                .change_by(0.05, Sentiment::POSITIVE);
        }
    }
}

fn on_helped(ctx: EventCtx<SimulateNpcs, OnHelped>) {
    let data = &mut *ctx.state.data_mut();

    if let Some(saver) = ctx.event.saver
        && let Some(actor) = data.actors.get_mut(ctx.event.actor)
        && let Some(npc) = actor.npc_mut()
    {
        npc.controller.actions.push(NpcAction::Say(
            Some(ctx.event.actor),
            comp::Content::localized("npc-speech-thank_you"),
        ));
        npc.sentiments
            .toward_mut(saver)
            .change_by(0.3, Sentiment::FRIEND);
    }
}

fn on_tick(ctx: EventCtx<SimulateNpcs, OnTick>) {
    let data = &mut *ctx.state.data_mut();

    // Maintain links
    let ids = data.actors.mounts.ids().collect::<Vec<_>>();
    let mut mount_activity = SecondaryMap::new();
    for link_id in ids {
        if let Some(link) = data.actors.mounts.get(link_id) {
            if let Some(mount) = data
                .actors
                .get(link.mount)
                .filter(|mount| mount.is_present_and_alive())
            {
                let wpos = mount.wpos;
                if let Some(rider) = data
                    .actors
                    .actors
                    .get_mut(link.rider)
                    .filter(|rider| rider.is_present_and_alive())
                {
                    rider.wpos = wpos;
                    if let Some(rider_npc) = rider.npc_mut() {
                        mount_activity.insert(link.mount, rider_npc.controller.activity);
                    }
                } else {
                    data.actors.mounts.dismount(link.rider)
                }
            } else {
                data.actors.mounts.remove_mount(link.mount)
            }
        }
    }

    let actor_inputs = Vec::new();

    for (actor_id, actor) in data
        .actors
        .actors
        .iter_mut()
        .filter(|(_, actor)| actor.is_present_and_alive())
    {
        let ActorKind::Npc(npc) = &mut actor.kind else {
            continue;
        };

        // TODO: simulate important NPC actions (like attacking)
        npc.controller
            .actions
            .retain(|_| matches!(actor.mode, SimulationMode::Loaded));

        if matches!(actor.mode, SimulationMode::Simulated) {
            let activity = if data.actors.mounts.get_mount_link(actor_id).is_some() {
                // We are riding, nothing to do.
                continue;
            } else if let Some(activity) = mount_activity.get(actor_id) {
                *activity
            } else {
                npc.controller.activity
            };

            match activity {
                // Move NPCs if they have a target destination
                Some(NpcActivity::Goto(target, speed_factor)) => {
                    let diff = target - actor.wpos;
                    let dist2 = diff.magnitude_squared();

                    if dist2 > 0.5f32.powi(2) {
                        let offset = diff
                            * (actor.body.max_speed_approx() * speed_factor * ctx.event.dt
                                / dist2.sqrt())
                            .min(1.0);
                        let new_wpos = actor.wpos + offset;

                        let is_valid = match actor.body {
                            // Don't move water bound bodies outside of water.
                            Body::Ship(comp::ship::Body::SailBoat | comp::ship::Body::Galleon)
                            | Body::FishMedium(_)
                            | Body::FishSmall(_) => {
                                let chunk_pos = new_wpos.xy().as_().wpos_to_cpos();
                                ctx.world
                                    .sim()
                                    .get(chunk_pos)
                                    .is_none_or(|f| f.river.river_kind.is_some())
                            },
                            Body::Ship(comp::ship::Body::DefaultAirship) => false,
                            _ => true,
                        };

                        if is_valid {
                            actor.wpos = new_wpos;
                        }

                        actor.dir = (target.xy() - actor.wpos.xy())
                            .try_normalized()
                            .unwrap_or(actor.dir);
                    }
                },
                // Move Flying NPCs like airships if they have a target destination
                Some(NpcActivity::GotoFlying(target, speed_factor, height, dir, mode)) => {
                    let diff = target - actor.wpos;
                    let dist2 = diff.magnitude_squared();

                    if dist2 > 0.5f32.powi(2) {
                        match actor.body {
                            Body::Ship(comp::ship::Body::DefaultAirship) => {
                                // RTSim NPCs don't interract with terrain, and their position is
                                // independent of ground level.
                                // While movement is simulated, airships will happily stay at ground
                                // level or fly through mountains.
                                // The code at the end of this block "Make sure NPCs remain in a
                                // valid location" just forces
                                // airships to be at least above ground (on the ground actually).
                                // The reason is that when docking, airships need to descend much
                                // closer to the terrain
                                // than when cruising between sites, so airships cannot be forced to
                                // stay at a fixed height above
                                // terrain (i.e. flying_height()). Instead, when mode is
                                // FlightMode::FlyThrough, set the airship altitude directly to
                                // terrain height + height (if Some)
                                // or terrain height + default height (npc.body.flying_height()).
                                // When mode is FlightMode::Braking, the airship is allowed to
                                // descend below flying height
                                // because it is near or at the dock. In this mode, if height is
                                // Some, set the airship altitude to
                                // the maximum of target.z or terrain height + height. If height is
                                // None, set the airship altitude to
                                // target.z. By forcing the airship altitude to be at a specific
                                // value, when the airship is
                                // suddenly in a loaded chunk it will not be below or at the ground
                                // and will not get stuck.

                                // Move in x,y
                                let diffxy = target.xy() - actor.wpos.xy();
                                let distxy2 = diffxy.magnitude_squared();
                                if distxy2 > 0.5f32.powi(2) {
                                    let offsetxy = diffxy
                                        * (actor.body.max_speed_approx()
                                            * speed_factor
                                            * ctx.event.dt
                                            / distxy2.sqrt());
                                    actor.wpos.x += offsetxy.x;
                                    actor.wpos.y += offsetxy.y;
                                }
                                // The diff is not computed for z like x,y. Rather, the altitude is
                                // set directly so that when the
                                // simulated ship is suddenly in a loaded chunk it will not be below
                                // or at the ground level and risk getting stuck.
                                let base_height =
                                    if mode == FlightMode::FlyThrough || height.is_some() {
                                        ctx.world
                                            .sim()
                                            .get_surface_alt_approx(actor.wpos.xy().as_())
                                    } else {
                                        0.0
                                    };
                                let ship_z = match mode {
                                    FlightMode::FlyThrough => {
                                        base_height + height.unwrap_or(actor.body.flying_height())
                                    },
                                    FlightMode::Braking(_) => {
                                        (base_height + height.unwrap_or(0.0)).max(target.z)
                                    },
                                };
                                actor.wpos.z = ship_z;
                            },
                            _ => {
                                let offset = diff
                                    * (actor.body.max_speed_approx() * speed_factor * ctx.event.dt
                                        / dist2.sqrt())
                                    .min(1.0);
                                let new_wpos = actor.wpos + offset;

                                let is_valid = match actor.body {
                                    // Don't move water bound bodies outside of water.
                                    Body::Ship(
                                        comp::ship::Body::SailBoat | comp::ship::Body::Galleon,
                                    )
                                    | Body::FishMedium(_)
                                    | Body::FishSmall(_) => {
                                        let chunk_pos = new_wpos.xy().as_().wpos_to_cpos();
                                        ctx.world
                                            .sim()
                                            .get(chunk_pos)
                                            .is_none_or(|f| f.river.river_kind.is_some())
                                    },
                                    _ => true,
                                };

                                if is_valid {
                                    actor.wpos = new_wpos;
                                }
                            },
                        }

                        if let Some(dir_override) = dir {
                            actor.dir = dir_override.xy().try_normalized().unwrap_or(actor.dir);
                        } else {
                            actor.dir = (target.xy() - actor.wpos.xy())
                                .try_normalized()
                                .unwrap_or(actor.dir);
                        }
                    }
                },
                Some(
                    NpcActivity::Gather(_)
                    | NpcActivity::HuntAnimals
                    | NpcActivity::Dance(_)
                    | NpcActivity::Cheer(_)
                    | NpcActivity::Sit(..)
                    | NpcActivity::Talk(..),
                ) => {
                    // TODO: Maybe they should walk around randomly
                    // when gathering resources?
                },
                None => {},
            }

            // Make sure NPCs remain in a valid location
            let clamped_wpos = actor.wpos.xy().clamped(
                Vec2::zero(),
                (ctx.world.sim().get_size() * TerrainChunkSize::RECT_SIZE).as_(),
            );
            match actor.body {
                // Don't force air ships to be at flying_height, else they can't land at docks.
                Body::Ship(comp::ship::Body::DefaultAirship | comp::ship::Body::AirBalloon) => {
                    actor.wpos = clamped_wpos.with_z(
                        ctx.world
                            .sim()
                            .get_surface_alt_approx(clamped_wpos.as_())
                            .max(actor.wpos.z),
                    );
                },
                _ => {
                    actor.wpos = clamped_wpos.with_z(
                        ctx.world.sim().get_surface_alt_approx(clamped_wpos.as_())
                            + actor.body.flying_height(),
                    );
                },
            }
        }

        // Move home if required
        if let Some(new_home) = npc.controller.new_home.take() {
            // Remove the NPC from their old home population
            if let Some(old_home) = actor.home
                && let Some(old_home) = data.sites.get_mut(old_home)
            {
                old_home.population.remove(&actor_id);
            }
            // Add the NPC to their new home population
            if let Some(new_home) = new_home
                && let Some(new_home) = data.sites.get_mut(new_home)
            {
                new_home.population.insert(actor_id);
            }
            actor.home = new_home;
        }

        // Create registered quests
        for (id, quest) in core::mem::take(&mut npc.controller.quests_to_create) {
            data.quests.create(id, quest);
        }
        // Set job status
        npc.job = npc.controller.job.clone();
    }

    for (actor_id, input) in actor_inputs {
        if let Some(actor) = data.actors.get_mut(actor_id)
            && let Some(npc) = actor.npc_mut()
        {
            npc.inbox.push_back(input);
        }
    }
}
