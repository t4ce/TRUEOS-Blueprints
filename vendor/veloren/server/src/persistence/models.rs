#[cfg_attr(
    feature = "redb-backend",
    derive(Clone, serde::Serialize, serde::Deserialize)
)]
pub struct Character {
    pub character_id: i64,
    #[cfg_attr(not(feature = "redb-backend"), expect(dead_code))]
    pub player_uuid: String,
    pub alias: String,
    pub waypoint: Option<String>,
    pub hardcore: i64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Item {
    pub item_id: i64,
    pub parent_container_item_id: i64,
    pub item_definition_id: String,
    /// `u32::MAX` must fit inside this type
    pub stack_size: i64,
    pub position: String,
    pub properties: String,
}

#[cfg_attr(
    feature = "redb-backend",
    derive(Clone, serde::Serialize, serde::Deserialize)
)]
pub struct Body {
    #[cfg_attr(not(feature = "redb-backend"), expect(dead_code))]
    pub body_id: i64,
    pub variant: String,
    pub body_data: String,
}

#[cfg_attr(
    feature = "redb-backend",
    derive(Clone, serde::Serialize, serde::Deserialize)
)]
pub struct SkillGroup {
    pub entity_id: i64,
    pub skill_group_kind: String,
    pub earned_exp: i64,
    pub spent_exp: i64,
    pub skills: String,
    pub hash_val: Vec<u8>,
}

#[cfg_attr(
    feature = "redb-backend",
    derive(Clone, serde::Serialize, serde::Deserialize)
)]
pub struct Pet {
    pub database_id: i64,
    // TODO: add ability to store and change pet names
    //
    // Originally we just stored hardcoded English names here, but that is a bit
    // impossible now that we translate names.
    //
    // If we do implement such functionality, we probably want to use something
    // similar to current npcs that have both translated and hardcoded names
    // using `name-misc-with-alias-template`.
    // Or even some better system for displaying complex names such as this.
    #[cfg_attr(not(feature = "redb-backend"), expect(unused))]
    pub name: String,
    pub body_variant: String,
    pub body_data: String,
}

#[cfg_attr(
    feature = "redb-backend",
    derive(Clone, serde::Serialize, serde::Deserialize)
)]
pub struct AbilitySets {
    #[cfg_attr(not(feature = "redb-backend"), expect(dead_code))]
    pub entity_id: i64,
    pub ability_sets: String,
}
