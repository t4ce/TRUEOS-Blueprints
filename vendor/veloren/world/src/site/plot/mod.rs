mod adlet;
mod airship_dock;
mod barn;
mod bridge;
mod building;
mod camp;
mod castle;
mod citadel;
mod cliff_tower;
mod cliff_town_airship_dock;
mod coastal_airship_dock;
mod coastal_house;
mod coastal_workshop;
mod cultist;
mod desert_city_airship_dock;
mod desert_city_arena;
mod desert_city_multiplot;
mod desert_city_temple;
mod dwarven_mine;
mod farm_field;
mod giant_tree;
mod glider_finish;
mod glider_platform;
mod glider_ring;
mod gnarling;
mod haniwa;
mod house;
mod jungle_ruin;
mod myrmidon_arena;
mod myrmidon_house;
mod pirate_hideout;
mod plaza;
mod road;
mod rock_circle;
mod sahagin;
mod savannah_airship_dock;
mod savannah_guard_hut;
mod savannah_hut;
mod savannah_workshop;
mod sea_chapel;
pub mod tavern;
mod terracotta_house;
mod terracotta_palace;
mod terracotta_yard;
mod troll_cave;
mod vampire_castle;
mod workshop;

pub use self::{
    adlet::AdletStronghold,
    airship_dock::AirshipDock,
    barn::Barn,
    bridge::Bridge,
    building::Building,
    camp::Camp,
    castle::Castle,
    citadel::Citadel,
    cliff_tower::CliffTower,
    cliff_town_airship_dock::CliffTownAirshipDock,
    coastal_airship_dock::CoastalAirshipDock,
    coastal_house::CoastalHouse,
    coastal_workshop::CoastalWorkshop,
    cultist::Cultist,
    desert_city_airship_dock::DesertCityAirshipDock,
    desert_city_arena::DesertCityArena,
    desert_city_multiplot::DesertCityMultiPlot,
    desert_city_temple::DesertCityTemple,
    dwarven_mine::DwarvenMine,
    farm_field::FarmField,
    giant_tree::GiantTree,
    glider_finish::GliderFinish,
    glider_platform::GliderPlatform,
    glider_ring::GliderRing,
    gnarling::GnarlingFortification,
    haniwa::Haniwa,
    house::House,
    jungle_ruin::JungleRuin,
    myrmidon_arena::MyrmidonArena,
    myrmidon_house::MyrmidonHouse,
    pirate_hideout::PirateHideout,
    plaza::Plaza,
    road::{Road, RoadKind, RoadLights, RoadMaterial},
    rock_circle::RockCircle,
    sahagin::Sahagin,
    savannah_airship_dock::SavannahAirshipDock,
    savannah_guard_hut::SavannahGuardHut,
    savannah_hut::SavannahHut,
    savannah_workshop::SavannahWorkshop,
    sea_chapel::SeaChapel,
    tavern::Tavern,
    terracotta_house::TerracottaHouse,
    terracotta_palace::TerracottaPalace,
    terracotta_yard::TerracottaYard,
    troll_cave::TrollCave,
    vampire_castle::VampireCastle,
    workshop::Workshop,
};

use super::*;
use crate::{ColumnSample, util::DHashSet};
use common::{match_some, path::Path};
use rand_chacha::ChaCha8Rng;
use vek::*;

pub struct Plot {
    pub(crate) kind: PlotKind,
    pub(crate) root_tile: Vec2<i32>,
    pub(crate) tiles: DHashSet<Vec2<i32>>,
}

impl Plot {
    pub fn find_bounds(&self) -> Aabr<i32> {
        self.tiles
            .iter()
            .fold(Aabr::new_empty(self.root_tile), |b, t| {
                b.expanded_to_contain_point(*t)
            })
    }

    pub fn z_range(&self) -> Option<Range<i32>> {
        match_some!(&self.kind, PlotKind::House(house) => house.z_range())
    }

    pub fn kind(&self) -> &PlotKind { &self.kind }

    pub fn root_tile(&self) -> Vec2<i32> { self.root_tile }

    pub fn tiles(&self) -> impl ExactSizeIterator<Item = Vec2<i32>> + '_ {
        self.tiles.iter().copied()
    }

    pub fn is_house(&self) -> bool {
        // TODO: Better than this
        self.door_tile().is_some()
    }

    pub fn is_workshop(&self) -> bool {
        // TODO: Better than this
        matches!(
            &self.kind,
            PlotKind::Workshop(_) | PlotKind::CoastalWorkshop(_) | PlotKind::SavannahWorkshop(_)
        )
    }
}

#[derive(strum::Display)]
pub enum PlotKind {
    House(House),
    AirshipDock(AirshipDock),
    GliderRing(GliderRing),
    GliderPlatform(GliderPlatform),
    GliderFinish(GliderFinish),
    Tavern(Tavern),
    CoastalAirshipDock(CoastalAirshipDock),
    CoastalHouse(CoastalHouse),
    CoastalWorkshop(CoastalWorkshop),
    Workshop(Workshop),
    DesertCityMultiPlot(DesertCityMultiPlot),
    DesertCityTemple(DesertCityTemple),
    DesertCityArena(DesertCityArena),
    DesertCityAirshipDock(DesertCityAirshipDock),
    SeaChapel(SeaChapel),
    JungleRuin(JungleRuin),
    Plaza(Plaza),
    Castle(Castle),
    Cultist(Cultist),
    Road(Road),
    Gnarling(GnarlingFortification),
    Adlet(AdletStronghold),
    Haniwa(Haniwa),
    GiantTree(GiantTree),
    CliffTower(CliffTower),
    CliffTownAirshipDock(CliffTownAirshipDock),
    Sahagin(Sahagin),
    Citadel(Citadel),
    SavannahAirshipDock(SavannahAirshipDock),
    SavannahGuardHut(SavannahGuardHut),
    SavannahHut(SavannahHut),
    SavannahWorkshop(SavannahWorkshop),
    Barn(Barn),
    Bridge(Bridge),
    PirateHideout(PirateHideout),
    RockCircle(RockCircle),
    TrollCave(TrollCave),
    Camp(Camp),
    DwarvenMine(DwarvenMine),
    TerracottaPalace(TerracottaPalace),
    TerracottaHouse(TerracottaHouse),
    TerracottaYard(TerracottaYard),
    FarmField(FarmField),
    VampireCastle(VampireCastle),
    MyrmidonArena(MyrmidonArena),
    MyrmidonHouse(MyrmidonHouse),
    Building(Building),
}

/// # Syntax
/// ```ignore
/// foreach_plot!(expr, plot => plot.something())
/// ```
#[macro_export]
macro_rules! foreach_plot {
    ($p:expr, $x:ident => $y:expr $(,)?) => {
        match $p {
            PlotKind::House($x) => $y,
            PlotKind::AirshipDock($x) => $y,
            PlotKind::CoastalAirshipDock($x) => $y,
            PlotKind::CoastalHouse($x) => $y,
            PlotKind::CoastalWorkshop($x) => $y,
            PlotKind::Workshop($x) => $y,
            PlotKind::DesertCityAirshipDock($x) => $y,
            PlotKind::DesertCityMultiPlot($x) => $y,
            PlotKind::DesertCityTemple($x) => $y,
            PlotKind::DesertCityArena($x) => $y,
            PlotKind::SeaChapel($x) => $y,
            PlotKind::JungleRuin($x) => $y,
            PlotKind::Plaza($x) => $y,
            PlotKind::Castle($x) => $y,
            PlotKind::Road($x) => $y,
            PlotKind::Gnarling($x) => $y,
            PlotKind::Adlet($x) => $y,
            PlotKind::GiantTree($x) => $y,
            PlotKind::CliffTower($x) => $y,
            PlotKind::CliffTownAirshipDock($x) => $y,
            PlotKind::Citadel($x) => $y,
            PlotKind::SavannahAirshipDock($x) => $y,
            PlotKind::SavannahGuardHut($x) => $y,
            PlotKind::SavannahHut($x) => $y,
            PlotKind::SavannahWorkshop($x) => $y,
            PlotKind::Barn($x) => $y,
            PlotKind::Bridge($x) => $y,
            PlotKind::PirateHideout($x) => $y,
            PlotKind::Tavern($x) => $y,
            PlotKind::Cultist($x) => $y,
            PlotKind::Haniwa($x) => $y,
            PlotKind::Sahagin($x) => $y,
            PlotKind::RockCircle($x) => $y,
            PlotKind::TrollCave($x) => $y,
            PlotKind::Camp($x) => $y,
            PlotKind::DwarvenMine($x) => $y,
            PlotKind::TerracottaPalace($x) => $y,
            PlotKind::TerracottaHouse($x) => $y,
            PlotKind::TerracottaYard($x) => $y,
            PlotKind::FarmField($x) => $y,
            PlotKind::VampireCastle($x) => $y,
            PlotKind::GliderRing($x) => $y,
            PlotKind::GliderPlatform($x) => $y,
            PlotKind::GliderFinish($x) => $y,
            PlotKind::MyrmidonArena($x) => $y,
            PlotKind::MyrmidonHouse($x) => $y,
            PlotKind::Building($x) => $y,
        }
    };
}

pub use foreach_plot;

impl Structure for Plot {
    #[cfg(feature = "dyn-lib")]
    #[unsafe(export_name = "as_dyn_structure_plot")]
    fn as_dyn_outer(&self) -> Option<(&dyn Structure, &'static str)> {
        Some((Self::as_dyn_impl(self), "as_dyn_structure_plot"))
    }

    fn render_inner(&self, site: &Site, land: &Land, painter: &Painter) {
        foreach_plot!(&self.kind, plot => plot.render(site, land, painter))
    }

    fn spawn_rules_inner(
        &self,
        spawn_rules: &mut SpawnRules,
        land: &Land,
        wpos: Vec2<i32>,
        weight: f32,
    ) {
        foreach_plot!(&self.kind, plot => plot.spawn_rules(spawn_rules, land, wpos, weight))
    }

    fn rel_terrain_offset(&self, col: &ColumnSample) -> i32 {
        foreach_plot!(&self.kind, plot => plot.rel_terrain_offset(col))
    }

    fn terrain_surface_at_inner(
        &self,
        wpos: Vec2<i32>,
        old: Block,
        rng: &mut ChaCha8Rng,
        col: &ColumnSample,
        z_off: i32,
        site: &Site,
    ) -> Option<Block> {
        foreach_plot!(&self.kind, plot => plot.terrain_surface_at(wpos, old, rng, col, z_off, site))
    }

    fn airship_dock_info(&self) -> Option<AirshipDockInfo<'_>> {
        foreach_plot!(&self.kind, plot => plot.airship_dock_info())
    }

    fn door_tile(&self) -> Option<Vec2<i32>> { foreach_plot!(&self.kind, plot => plot.door_tile()) }

    fn render_ordering(&self) -> u32 { foreach_plot!(&self.kind, plot => plot.render_ordering()) }
}

pub struct AirshipDockInfo<'plot> {
    pub door_tile: Vec2<i32>,
    pub center: Vec2<i32>,
    pub docking_positions: &'plot [Vec3<i32>],
}
