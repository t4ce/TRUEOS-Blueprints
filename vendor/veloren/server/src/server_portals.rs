//! Source-side admission holds. No character/inventory data crosses server
//! boundaries.
use crate::{Settings, client::Client};
use common::{
    comp::{
        self, Agent, CharacterState, Controller, ForceUpdate, Immovable, Object, Pos, Presence,
        PresenceKind, Vel,
    },
    resources::Time,
    terrain::TerrainGrid,
    vol::ReadVol,
};
use common_net::msg::ServerGeneral;
use specs::{Entity, Join, World, WorldExt};
use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};
use vek::Vec3;

const HOLD_TIMEOUT: f64 = 60.0;
const RETRY_DELAY: f64 = 10.0;
const ACTIVATION_RADIUS: f32 = 1.75;

struct Hold {
    id: u64,
    portal: Entity,
    origin: Vec3<f32>,
    hover: Vec3<f32>,
    expires: f64,
    cancelled: bool,
    had_immovable: bool,
}
#[derive(Default)]
struct Transfers {
    pending_portals: Vec<Vec3<f32>>,
    enabled: HashSet<Entity>,
    next_id: u64,
    holds: HashMap<Entity, Hold>,
    cooldowns: HashMap<Entity, f64>,
}
#[derive(Default)]
pub struct PortalTransfers(Mutex<Transfers>);
impl PortalTransfers {
    pub(crate) fn queue_portal(&self, position: Vec3<f32>) {
        let mut state = self.0.lock().unwrap();
        if !state.pending_portals.contains(&position) {
            state.pending_portals.push(position);
        }
    }

    pub(crate) fn enable(&self, entity: Entity) { self.0.lock().unwrap().enabled.insert(entity); }

    pub(crate) fn is_holding(&self, entity: Entity) -> bool {
        self.0.lock().unwrap().holds.contains_key(&entity)
    }

    pub(crate) fn is_portal_active(&self, portal: Entity) -> bool {
        self.0
            .lock()
            .unwrap()
            .holds
            .values()
            .any(|hold| hold.portal == portal)
    }

    pub(crate) fn cancel(&self, entity: Entity, id: u64) {
        if let Some(hold) = self
            .0
            .lock()
            .unwrap()
            .holds
            .get_mut(&entity)
            .filter(|h| h.id == id)
        {
            hold.cancelled = true;
        }
    }
}

/// Fail closed: every support and clearance block must be loaded and valid.
pub(crate) fn clear_landing(terrain: &TerrainGrid, base: Vec3<f32>) -> bool {
    let p = base.map(|v| v.floor() as i32);
    (-3..=3).all(|x| {
        (-3..=3).all(|y| {
            terrain
                .get_key_arc_real(TerrainGrid::chunk_key(p + Vec3::new(x, y, 0)))
                .is_some()
                && terrain
                    .get(p + Vec3::new(x, y, -1))
                    .is_ok_and(|b| b.is_filled())
                && (0..8).all(|z| {
                    terrain
                        .get(p + Vec3::new(x, y, z))
                        .is_ok_and(|b| b.solid_height() == 0.0 && !b.is_liquid())
                })
        })
    })
}

/// Chunk completion order must not decide whether a town gets its portal.
/// Wait until the whole footprint is loaded, then validate the final terrain.
pub(crate) fn spawn_pending(state: &mut common_state::State) {
    use crate::state_ext::StateExt;
    use specs::Builder;
    let ready = {
        let ecs = state.ecs();
        let transfers = ecs.read_resource::<PortalTransfers>();
        let mut transfers = transfers.0.lock().unwrap();
        if ecs
            .read_resource::<Settings>()
            .town_portal_destination
            .is_none()
        {
            transfers.pending_portals.clear();
            return;
        }
        let terrain = ecs.read_resource::<TerrainGrid>();
        let objects = ecs.read_storage::<Object>();
        let positions = ecs.read_storage::<Pos>();
        let mut ready = Vec::new();
        transfers.pending_portals.retain(|base| {
            let p = base.map(|v| v.floor() as i32);
            if ![-3, 3].into_iter().all(|x| {
                [-3, 3].into_iter().all(|y| {
                    terrain
                        .get_key_arc_real(TerrainGrid::chunk_key(p + Vec3::new(x, y, 0)))
                        .is_some()
                })
            }) {
                return true;
            }
            if clear_landing(&terrain, *base)
                && !(&objects, &positions).join().any(|(o, pos)| {
                    matches!(o, Object::ServerPortal) && pos.0.distance_squared(*base) < 0.01
                })
            {
                ready.push(*base);
            }
            false
        });
        ready
    };
    for position in ready {
        state
            .create_object(Pos(position), comp::object::Body::Portal)
            .with(Immovable)
            .with(Object::ServerPortal)
            .build();
    }
}

pub(crate) fn maintain(ecs: &World) {
    let time = ecs.read_resource::<Time>().0;
    let destination = ecs
        .read_resource::<Settings>()
        .town_portal_destination
        .clone();
    let transfers = ecs.read_resource::<PortalTransfers>();
    let mut state = transfers.0.lock().unwrap();
    let entities = ecs.entities();
    let objects = ecs.read_storage::<Object>();
    let clients = ecs.read_storage::<Client>();
    let presences = ecs.read_storage::<Presence>();
    let healths = ecs.read_storage::<comp::Health>();
    let agents = ecs.read_storage::<Agent>();
    let uids = ecs.read_storage::<common::uid::Uid>();
    let trades = ecs.read_resource::<common::trade::Trades>();
    let riders = ecs.read_storage::<common::link::Is<common::mounting::Rider>>();
    let volume_riders = ecs.read_storage::<common::link::Is<common::mounting::VolumeRider>>();
    let mut positions = ecs.write_storage::<Pos>();
    let mut velocities = ecs.write_storage::<Vel>();
    let orientations = ecs.read_storage::<comp::Ori>();
    let mut controllers = ecs.write_storage::<Controller>();
    let mut updates = ecs.write_storage::<ForceUpdate>();
    let mut immovable = ecs.write_storage::<Immovable>();
    let mut character_states = ecs.write_storage::<CharacterState>();
    let terrain = ecs.read_resource::<TerrainGrid>();
    let portals: Vec<_> = (&entities, &objects, &positions)
        .join()
        .filter(|(_, object, _)| matches!(object, Object::ServerPortal))
        .map(|(e, _, pos)| (e, pos.0))
        .collect();

    let release: Vec<_> = state
        .holds
        .iter()
        .filter(|(entity, hold)| {
            hold.cancelled
                || time >= hold.expires
                || destination.is_none()
                || !clients.contains(**entity)
                || !presences.contains(**entity)
                || !objects
                    .get(hold.portal)
                    .is_some_and(|o| matches!(o, Object::ServerPortal))
        })
        .map(|(e, _)| *e)
        .collect();
    for entity in release {
        let hold = state.holds.remove(&entity).unwrap();
        if let Some(pos) = positions.get_mut(entity) {
            pos.0 = hold.origin;
        }
        if let Some(vel) = velocities.get_mut(entity) {
            *vel = Vel::zero();
        }
        if let Some(update) = updates.get_mut(entity) {
            update.update();
        }
        if !hold.had_immovable {
            immovable.remove(entity);
        }
        if let Some(controller) = controllers.get_mut(entity) {
            *controller = Controller::default();
        }
        state.cooldowns.insert(entity, time + RETRY_DELAY);
        if let Some(client) = clients.get(entity) {
            let _ = client.send(ServerGeneral::ServerPortalReleased {
                transfer_id: hold.id,
            });
        }
    }
    state.enabled.retain(|e| entities.is_alive(*e));
    state
        .cooldowns
        .retain(|e, deadline| entities.is_alive(*e) && *deadline > time);
    if let Some(destination) = destination {
        for (portal, base) in &portals {
            if !clear_landing(&terrain, *base) {
                continue;
            }
            let candidates: Vec<_> = (&entities, &positions, &clients, &presences)
                .join()
                .filter(|(e, pos, _, presence)| {
                    matches!(presence.kind, PresenceKind::Character(_))
                        && pos.0.distance_squared(*base) < ACTIVATION_RADIUS.powi(2)
                        && state.enabled.contains(e)
                        && !state.holds.contains_key(e)
                        && !state.cooldowns.contains_key(e)
                        && uids
                            .get(*e)
                            .is_some_and(|uid| !trades.in_trade_with_property(uid, |_| true))
                        && !riders.contains(*e)
                        && !volume_riders.contains(*e)
                        && healths
                            .get(*e)
                            .is_some_and(|h| !h.is_dead && !h.has_consumed_death_protection())
                        && character_states.get(*e).is_some_and(|s| {
                            matches!(s, CharacterState::Idle(_) | CharacterState::Wielding(_))
                        })
                        && !(&agents, &positions).join().any(|(a, p)| {
                            p.0.distance_squared(pos.0) < 200.0_f32.powi(2)
                                && a.target.is_some_and(|t| t.target == *e && t.aggro_on)
                        })
                })
                .map(|(e, pos, _, _)| (e, pos.0))
                .collect();
            for (entity, origin) in candidates {
                state.next_id = state.next_id.wrapping_add(1).max(1);
                let id = state.next_id;
                // Admission must be offered before the source begins holding the avatar.
                if clients
                    .get(entity)
                    .unwrap()
                    .send(ServerGeneral::ServerPortalOffer {
                        transfer_id: id,
                        destination: destination.clone(),
                        hover_position: *base + Vec3::unit_z() * 1.25,
                    })
                    .is_err()
                {
                    continue;
                }
                let had_immovable = immovable.contains(entity);
                let _ = immovable.insert(entity, Immovable);
                state.holds.insert(entity, Hold {
                    id,
                    portal: *portal,
                    origin,
                    hover: *base + Vec3::unit_z() * 1.25,
                    expires: time + HOLD_TIMEOUT,
                    cancelled: false,
                    had_immovable,
                });
            }
        }
    }
    for (entity, hold) in &state.holds {
        if let Some(pos) = positions.get_mut(*entity) {
            pos.0 = hold.hover;
        }
        if let Some(vel) = velocities.get_mut(*entity) {
            *vel = Vel::zero();
        }
        if let Some(controller) = controllers.get_mut(*entity) {
            *controller = Controller::default();
            controller.inputs.look_dir = orientations
                .get(*entity)
                .map(|ori| ori.look_dir())
                .unwrap_or_default();
        }
        if let Some(mut character_state) = character_states.get_mut(*entity) {
            *character_state = CharacterState::Idle(Default::default());
        }
        if let Some(update) = updates.get_mut(*entity) {
            update.update();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{
        terrain::{Block, BlockKind, SpriteKind, TerrainChunk, TerrainChunkMeta, map::MapSizeLg},
        vol::WriteVol,
    };
    use specs::Builder;
    use std::sync::Arc;
    use vek::{Rgb, Vec2};
    fn landing() -> TerrainGrid {
        let air = Block::air(SpriteKind::Empty);
        let chunk = Arc::new(TerrainChunk::new(
            1,
            Block::new(BlockKind::Rock, Rgb::broadcast(80)),
            air,
            TerrainChunkMeta::void(),
        ));
        let mut grid = TerrainGrid::new(
            MapSizeLg::new(Vec2::broadcast(2)).unwrap(),
            Arc::clone(&chunk),
        )
        .unwrap();
        grid.insert(Vec2::zero(), chunk);
        grid
    }
    #[test]
    fn landing_rejects_holes_water_overhangs_and_missing_neighbor_chunks() {
        let pos = Vec3::new(16.0, 16.0, 1.0);
        let mut grid = landing();
        assert!(clear_landing(&grid, pos));
        grid.set(Vec3::new(16, 16, 0), Block::air(SpriteKind::Empty))
            .unwrap();
        assert!(!clear_landing(&grid, pos));
        let mut grid = landing();
        grid.set(
            Vec3::new(16, 16, 5),
            Block::new(BlockKind::Rock, Rgb::broadcast(80)),
        )
        .unwrap();
        assert!(!clear_landing(&grid, pos));
        let mut grid = landing();
        grid.set(
            Vec3::new(16, 16, 1),
            Block::unfilled(BlockKind::Water, SpriteKind::Empty),
        )
        .unwrap();
        assert!(!clear_landing(&grid, pos));
        assert!(!clear_landing(&landing(), Vec3::new(0.0, 0.0, 1.0)));
    }
    #[test]
    fn cancellation_is_bound_to_entity_and_transfer_id_and_keeps_protection_until_release() {
        let mut ecs = World::new();
        let player = ecs.create_entity().build();
        let other = ecs.create_entity().build();
        let transfers = PortalTransfers::default();
        transfers.0.lock().unwrap().holds.insert(player, Hold {
            id: 42,
            portal: other,
            origin: Vec3::zero(),
            hover: Vec3::unit_z(),
            expires: 60.0,
            cancelled: false,
            had_immovable: false,
        });
        transfers.cancel(other, 42);
        transfers.cancel(player, 41);
        assert!(!transfers.0.lock().unwrap().holds[&player].cancelled);
        transfers.cancel(player, 42);
        assert!(transfers.0.lock().unwrap().holds[&player].cancelled);
        assert!(transfers.is_holding(player));
    }
    #[test]
    fn teardown_restores_position_and_ten_second_cooldown_without_removing_existing_immovable() {
        let mut ecs = World::new();
        ecs.register::<Object>();
        ecs.register::<Client>();
        ecs.register::<Presence>();
        ecs.register::<comp::Health>();
        ecs.register::<Agent>();
        ecs.register::<common::uid::Uid>();
        ecs.register::<common::link::Is<common::mounting::Rider>>();
        ecs.register::<common::link::Is<common::mounting::VolumeRider>>();
        ecs.register::<comp::Ori>();
        ecs.register::<Pos>();
        ecs.register::<Vel>();
        ecs.register::<Controller>();
        ecs.register::<ForceUpdate>();
        ecs.register::<Immovable>();
        ecs.register::<CharacterState>();
        ecs.insert(Time(0.0));
        ecs.insert(Settings::default());
        ecs.insert(landing());
        ecs.insert(common::trade::Trades::default());
        let origin = Vec3::new(16.0, 16.0, 1.0);
        let player = ecs
            .create_entity()
            .with(Pos(origin + Vec3::unit_z() * 5.0))
            .with(Vel(Vec3::unit_z()))
            .with(Immovable)
            .with(Controller::default())
            .with(ForceUpdate::forced())
            .build();
        let transfers = PortalTransfers::default();
        transfers.0.lock().unwrap().holds.insert(player, Hold {
            id: 9,
            portal: player,
            origin,
            hover: origin + Vec3::unit_z(),
            expires: 60.0,
            cancelled: false,
            had_immovable: true,
        });
        ecs.insert(transfers);
        // No client remains: teardown must release immediately, before its deadline.
        maintain(&ecs);
        assert_eq!(ecs.read_storage::<Pos>().get(player).unwrap().0, origin);
        assert_eq!(*ecs.read_storage::<Vel>().get(player).unwrap(), Vel::zero());
        assert!(ecs.read_storage::<Immovable>().contains(player));
        let transfers = ecs.read_resource::<PortalTransfers>();
        assert!(!transfers.is_holding(player));
        assert_eq!(transfers.0.lock().unwrap().cooldowns[&player], 10.0);
        drop(transfers);
        ecs.write_resource::<Time>().0 = 10.0;
        maintain(&ecs);
        assert!(
            !ecs.read_resource::<PortalTransfers>()
                .0
                .lock()
                .unwrap()
                .cooldowns
                .contains_key(&player)
        );
    }
}
