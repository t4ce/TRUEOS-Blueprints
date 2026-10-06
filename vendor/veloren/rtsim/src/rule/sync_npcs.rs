use crate::{
    RtState, Rule, RuleError,
    event::{EventCtx, OnDeath, OnHealthChange, OnSetup, OnTick},
};
use common::{grid::Grid, rtsim::NpcInput, terrain::CoordinateConversions};

pub struct SyncNpcs;

impl Rule for SyncNpcs {
    fn start(rtstate: &mut RtState) -> Result<Self, RuleError> {
        rtstate.bind::<Self, OnSetup>(on_setup);
        rtstate.bind::<Self, OnDeath>(on_death);
        rtstate.bind::<Self, OnHealthChange>(on_health_change);
        rtstate.bind::<Self, OnTick>(on_tick);

        Ok(Self)
    }
}

fn on_setup(ctx: EventCtx<SyncNpcs, OnSetup>) {
    let data = &mut *ctx.state.data_mut();

    // Create actor grid
    data.actors.actor_grid = Grid::new(ctx.world.sim().get_size().as_(), Default::default());

    // Add actors to home population
    for (actor_id, actor) in data.actors.iter() {
        if let Some(home) = actor.home.and_then(|home| data.sites.get_mut(home)) {
            home.population.insert(actor_id);
        }
    }

    // Update the list of nearest sites by size for each site
    let sites_iter = data.sites.iter().filter_map(|(site_id, site)| {
        let world_site = site.world_site.map(|ws| ctx.index.sites.get(ws))?;
        Some((site_id, site, world_site))
    });
    let nearest_by_size = sites_iter.clone()
        .map(|(site_id, site, world_site)| {
            let mut other_sites = sites_iter.clone()
                // Only include sites in the list if they're not the current one and they're more populus
                .filter(|(other_id, _, other_site)| *other_id != site_id && other_site.plots().len() > world_site.plots().len())
                .collect::<Vec<_>>();
            other_sites.sort_by_key(|(_, other, _)| other.wpos.as_::<i64>().distance_squared(site.wpos.as_::<i64>()));
            let mut max_size = 0;
            // Remove sites that aren't in increasing order of size (Stalin sort?!)
            other_sites.retain(|(_, _, other_site)| {
                if other_site.plots().len() > max_size {
                    max_size = other_site.plots().len();
                    true
                } else {
                    false
                }
            });
            let nearest_by_size = other_sites
                .into_iter()
                .map(|(site_id, _, _)| site_id)
                .collect::<Vec<_>>();
            (site_id, nearest_by_size)
        })
        .collect::<Vec<_>>();
    for (site_id, nearest_by_size) in nearest_by_size {
        if let Some(site) = data.sites.get_mut(site_id) {
            site.nearby_sites_by_size = nearest_by_size;
        }
    }
}

fn on_health_change(ctx: EventCtx<SyncNpcs, OnHealthChange>) {
    let data = &mut *ctx.state.data_mut();

    // As this handler does not correctly handle death, ignore events that set the
    // health fraction to 0 (dead)
    if ctx.event.new_health_fraction != 0.0
        && let Some(actor) = data.actors.get_mut(ctx.event.actor)
        && let Some(presence) = &mut actor.presence
    {
        presence.health_fraction = ctx.event.new_health_fraction;
    }
}

fn on_death(ctx: EventCtx<SyncNpcs, OnDeath>) {
    let data = &mut *ctx.state.data_mut();

    if let Some(actor) = data.actors.get_mut(ctx.event.actor)
        && let Some(presence) = &mut actor.presence
    {
        // Mark the actor as dead, allowing us to clear them up later
        presence.health_fraction = 0.0;
    }
}

fn on_tick(ctx: EventCtx<SyncNpcs, OnTick>) {
    let data = &mut *ctx.state.data_mut();
    for (actor_id, actor) in data.actors.actors.iter_mut() {
        // Update the actor's current site, if any
        actor.current_site = ctx
            .world
            .sim()
            .get(actor.wpos.xy().as_().wpos_to_cpos())
            .and_then(|chunk| {
                chunk
                    .sites
                    .iter()
                    .find_map(|site| data.sites.world_site_map.get(site).copied())
            });

        // Share known reports with current site, if it's our home
        // TODO: Only share new reports
        if let Some(current_site) = actor.current_site
            && Some(current_site) == actor.home
            && let Some(site) = data.sites.get_mut(current_site)
            && let Some(npc) = actor.npc_mut()
        {
            // TODO: Sites should have an inbox and their own AI code...?
            site.known_reports.extend(npc.known_reports.iter().copied());
            npc.inbox.extend(
                site.known_reports
                    .iter()
                    .copied()
                    .filter(|report| !npc.known_reports.contains(report))
                    .map(NpcInput::Report),
            );
        }

        // Update the actor's grid cell
        let chunk_pos = if actor.presence.is_some() {
            Some(actor.wpos.xy().as_().wpos_to_cpos())
        } else {
            None
        };
        if actor.chunk_pos != chunk_pos {
            if let Some(cell) = actor
                .chunk_pos
                .and_then(|chunk_pos| data.actors.actor_grid.get_mut(chunk_pos))
                && let Some(index) = cell.actors.iter().position(|id| *id == actor_id)
            {
                cell.actors.swap_remove(index);
            }
            actor.chunk_pos = chunk_pos;
            if let Some(chunk_pos) = chunk_pos
                && let Some(cell) = data.actors.actor_grid.get_mut(chunk_pos)
            {
                cell.actors.push(actor_id);
            }
        }

        // Make characters that haven't been seen since the penultimate tick be no
        // longer present (likely because the player they represent has logged
        // off) TODO: Prune characters that we've not seen for a *long* time
        // once we hit some arbitrary cap
        if let Some(character) = actor.character()
            && let Some(last_present_at) = character.last_present_at
            && data.tick > last_present_at + 1
        {
            actor.presence = None;
        }
    }
}
