//! Character CRUD on redb. Gameplay conversions are shared with the native
//! backend.
use super::{
    Connection, EditableComponents, PersistedComponents,
    character_loader::{CharacterCreationResult, CharacterDataResult, CharacterListResult},
    character_updater::PetPersistenceData,
    error::PersistenceError,
    models::{AbilitySets, Body, Character, Item, Pet, SkillGroup},
    redb_store::{CHARACTERS, CrudRead, CrudWrite, ITEMS, META, PLAYERS},
};
use common::{
    character::{CharacterId, CharacterItem, MAX_CHARACTERS_PER_PLAYER},
    comp::{self, Content, Inventory},
    event::{PermanentChange, UpdateCharacterMetadata},
    npc::NPC_NAMES,
};
use redb::WriteTransaction;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroU64,
};
use tracing::warn;

mod conversions;
use conversions::*;
pub(crate) type EntityId = i64;

#[derive(Clone, Serialize, Deserialize)]
struct Containers {
    inventory: i64,
    loadout: i64,
    overflow: i64,
    recipes: i64,
}

#[derive(Clone, Serialize, Deserialize)]
struct CharacterRecord {
    character: Character,
    body: Body,
    containers: Containers,
    items: Vec<i64>,
    skills: Vec<SkillGroup>,
    pets: Vec<Pet>,
    abilities: AbilitySets,
}

#[derive(Serialize, Deserialize)]
struct StoredItem {
    owner: i64,
    item: Item,
}

fn record(store: &impl CrudRead, id: CharacterId) -> Result<CharacterRecord, PersistenceError> {
    store
        .get(CHARACTERS, &id.0.to_string())?
        .ok_or(PersistenceError::CharacterDataError)
}

fn owned_record(
    store: &impl CrudRead,
    uuid: &str,
    id: CharacterId,
) -> Result<CharacterRecord, PersistenceError> {
    let record = record(store, id)?;
    if record.character.player_uuid != uuid {
        return Err(PersistenceError::CharacterDataError);
    }
    Ok(record)
}

fn player_ids(store: &impl CrudRead, uuid: &str) -> Result<Vec<i64>, PersistenceError> {
    Ok(store.get(PLAYERS, uuid)?.unwrap_or_default())
}

fn allocate(transaction: &WriteTransaction, count: i64) -> Result<i64, PersistenceError> {
    let next: i64 = transaction
        .get(META, "next_entity_id")?
        .ok_or_else(|| PersistenceError::OtherError("Missing entity ID allocator".into()))?;
    let end = next
        .checked_add(count)
        .filter(|end| count >= 0 && *end > 0)
        .ok_or_else(|| PersistenceError::OtherError("Entity ID space exhausted".into()))?;
    transaction.update(META, "next_entity_id", &end)?;
    Ok(next)
}

/// Global item rows preserve identity when items move between characters. A
/// stale character snapshot cannot resurrect an item now owned by another one.
fn save_items(
    record: &mut CharacterRecord,
    inventory: &Inventory,
    transaction: &WriteTransaction,
) -> Result<(), PersistenceError> {
    let start: i64 = transaction
        .get(META, "next_entity_id")?
        .ok_or(PersistenceError::CharacterDataError)?;
    let mut next = start;
    let containers = &record.containers;
    let items = convert_items_to_database_items(
        containers.loadout,
        inventory,
        containers.inventory,
        containers.overflow,
        containers.recipes,
        &mut next,
    );
    allocate(
        transaction,
        next.checked_sub(start)
            .ok_or(PersistenceError::CharacterDataError)?,
    )?;
    let retained: HashSet<_> = items.iter().map(|pair| pair.model.item_id).collect();
    for id in &record.items {
        if !retained.contains(id) {
            remove_owned_item(transaction, *id, record.character.character_id)?;
        }
    }
    record.items = items.iter().map(|pair| pair.model.item_id).collect();
    for pair in items {
        debug_assert_eq!(
            pair.comp.load().map(|id| id.get() as i64),
            Some(pair.model.item_id)
        );
        let key = pair.model.item_id.to_string();
        let value = StoredItem {
            owner: record.character.character_id,
            item: pair.model,
        };
        if transaction.get::<StoredItem>(ITEMS, &key)?.is_some() {
            transaction.update(ITEMS, &key, &value)?;
        } else {
            transaction.create(ITEMS, &key, &value)?;
        }
    }
    Ok(())
}

fn remove_owned_item(
    transaction: &WriteTransaction,
    id: i64,
    owner: i64,
) -> Result<(), PersistenceError> {
    let key = id.to_string();
    if transaction
        .get::<StoredItem>(ITEMS, &key)?
        .is_some_and(|item| item.owner == owner)
    {
        transaction.delete(ITEMS, &key)?;
    }
    Ok(())
}

/// Return parent-before-child order explicitly, including modular components.
fn items_under(
    store: &impl CrudRead,
    record: &CharacterRecord,
    root: i64,
) -> Result<Vec<Item>, PersistenceError> {
    let mut children: HashMap<i64, Vec<Item>> = HashMap::new();
    for id in &record.items {
        if let Some(stored) = store.get::<StoredItem>(ITEMS, &id.to_string())?
            && stored.owner == record.character.character_id
        {
            children
                .entry(stored.item.parent_container_item_id)
                .or_default()
                .push(stored.item);
        }
    }
    let mut queue = VecDeque::from([root]);
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    while let Some(parent) = queue.pop_front() {
        if !seen.insert(parent) {
            return Err(PersistenceError::ConversionError(
                "Cyclic inventory tree".into(),
            ));
        }
        if let Some(mut items) = children.remove(&parent) {
            items.sort_by_key(|item| item.item_id);
            for item in items {
                queue.push_back(item.item_id);
                result.push(item);
            }
        }
    }
    Ok(result)
}

fn waypoint(record: &CharacterRecord) -> (Option<comp::Waypoint>, Option<comp::MapMarker>) {
    match record
        .character
        .waypoint
        .as_deref()
        .map(convert_waypoint_from_database_json)
    {
        Some(Ok(value)) => value,
        Some(Err(error)) => {
            warn!(
                ?error,
                character_id = record.character.character_id,
                "Invalid stored waypoint"
            );
            (None, None)
        },
        None => (None, None),
    }
}

pub fn load_character_data(
    uuid: String,
    id: CharacterId,
    connection: &Connection,
) -> CharacterDataResult {
    let read = connection.read()?;
    let record = owned_record(&read, &uuid, id)?;
    let c = &record.containers;
    let inventory = convert_inventory_from_database_items(
        c.inventory,
        &items_under(&read, &record, c.inventory)?,
        c.loadout,
        &items_under(&read, &record, c.loadout)?,
        c.overflow,
        &items_under(&read, &record, c.overflow)?,
        &items_under(&read, &record, c.recipes)?,
    )?;
    let body = convert_body_from_database(&record.body.variant, &record.body.body_data)?;
    let (skill_set, skill_set_persistence_load_error) =
        convert_skill_set_from_database(&record.skills);
    let (waypoint, map_marker) = waypoint(&record);
    let pets = record
        .pets
        .iter()
        .filter_map(|stored| {
            let body = convert_body_from_database(&stored.body_variant, &stored.body_data).ok()?;
            let id = NonZeroU64::new(u64::try_from(stored.database_id).ok()?)?;
            let pet = comp::Pet::new_from_database(id);
            let stats = comp::Stats::new(
                NPC_NAMES
                    .read()
                    .get_default_name(&body)
                    .unwrap_or(Content::Plain(String::new())),
                body,
            );
            Some((pet, body, stats))
        })
        .collect();
    Ok((
        PersistedComponents {
            body,
            hardcore: convert_hardcore_from_database(record.character.hardcore)?,
            stats: convert_stats_from_database(record.character.alias, body),
            skill_set,
            inventory,
            waypoint,
            pets,
            active_abilities: convert_active_abilities_from_database(&record.abilities),
            map_marker,
        },
        UpdateCharacterMetadata {
            skill_set_persistence_load_error,
        },
    ))
}

pub fn load_character_list(uuid: &str, connection: &Connection) -> CharacterListResult {
    list(uuid, &connection.read()?)
}

fn list(uuid: &str, store: &impl CrudRead) -> CharacterListResult {
    let mut ids = player_ids(store, uuid)?;
    ids.sort_unstable();
    ids.into_iter()
        .map(|id| {
            let record = owned_record(store, uuid, CharacterId(id))?;
            let body = convert_body_from_database(&record.body.variant, &record.body.body_data)?;
            let loadout = convert_loadout_from_database_items(
                record.containers.loadout,
                &items_under(store, &record, record.containers.loadout)?,
            )?;
            let (recipes, _) = convert_recipe_book_from_database_items(&items_under(
                store,
                &record,
                record.containers.recipes,
            )?)?;
            Ok(CharacterItem {
                character: convert_character_from_database(&record.character),
                body,
                hardcore: convert_hardcore_from_database(record.character.hardcore)?.is_some(),
                inventory: Inventory::with_loadout(loadout, body).with_recipe_book(recipes),
                location: waypoint(&record).0.map(|waypoint| waypoint.get_pos()),
            })
        })
        .collect()
}

pub fn create_character(
    uuid: &str,
    alias: &str,
    components: PersistedComponents,
    transaction: &mut WriteTransaction,
) -> CharacterCreationResult {
    let mut ids = player_ids(transaction, uuid)?;
    if ids.len() >= MAX_CHARACTERS_PER_PLAYER as usize {
        return Err(PersistenceError::CharacterLimitReached);
    }
    let id = allocate(transaction, 5)?;
    let (variant, body_data) = convert_body_to_database_json(&components.body)?;
    let mut record = CharacterRecord {
        character: Character {
            character_id: id,
            player_uuid: uuid.into(),
            alias: alias.into(),
            waypoint: convert_waypoint_to_database_json(components.waypoint, components.map_marker),
            hardcore: convert_hardcore_to_database(components.hardcore),
        },
        body: Body {
            body_id: id,
            variant: variant.to_string(),
            body_data,
        },
        containers: Containers {
            inventory: id + 1,
            loadout: id + 2,
            overflow: id + 3,
            recipes: id + 4,
        },
        items: Vec::new(),
        skills: convert_skill_groups_to_database(
            CharacterId(id),
            components.skill_set.skill_groups(),
        ),
        pets: Vec::new(),
        abilities: convert_active_abilities_to_database(
            CharacterId(id),
            &components.active_abilities,
        ),
    };
    save_items(&mut record, &components.inventory, transaction)?;
    save_pets(&mut record, components.pets, transaction)?;
    transaction.create(CHARACTERS, &id.to_string(), &record)?;
    ids.push(id);
    if transaction.get::<Vec<i64>>(PLAYERS, uuid)?.is_some() {
        transaction.update(PLAYERS, uuid, &ids)?;
    } else {
        transaction.create(PLAYERS, uuid, &ids)?;
    }
    list(uuid, transaction).map(|list| (CharacterId(id), list))
}

pub fn edit_character(
    components: EditableComponents,
    trusted: Option<PermanentChange>,
    transaction: &mut WriteTransaction,
    id: CharacterId,
    uuid: &str,
    alias: Option<&str>,
) -> CharacterCreationResult {
    let mut record = owned_record(transaction, uuid, id)?;
    let (body,) = components;
    let old = convert_body_from_database(&record.body.variant, &record.body.body_data)?;
    let allowed = match trusted {
        Some(change) => change.expected_old_body == old,
        None => match (body, old) {
            (comp::Body::Humanoid(new), comp::Body::Humanoid(old)) => {
                new.species == old.species && new.body_type == old.body_type
            },
            _ => body == old,
        },
    };
    if !allowed {
        return Err(PersistenceError::CharacterDataError);
    }
    let (variant, body_data) = convert_body_to_database_json(&body)?;
    record.body.variant = variant.to_string();
    record.body.body_data = body_data;
    if let Some(alias) = alias {
        record.character.alias = alias.into();
    }
    transaction.update(CHARACTERS, &id.0.to_string(), &record)?;
    list(uuid, transaction).map(|list| (id, list))
}

pub fn delete_character(
    uuid: &str,
    id: CharacterId,
    transaction: &mut WriteTransaction,
) -> Result<(), PersistenceError> {
    let Some(record) = transaction.get::<CharacterRecord>(CHARACTERS, &id.0.to_string())? else {
        return Ok(());
    };
    if record.character.player_uuid != uuid {
        return Ok(());
    }
    for item in record.items {
        remove_owned_item(transaction, item, id.0)?;
    }
    transaction.delete(CHARACTERS, &id.0.to_string())?;
    let mut ids = player_ids(transaction, uuid)?;
    ids.retain(|stored| *stored != id.0);
    if ids.is_empty() {
        transaction.delete(PLAYERS, uuid)?;
    } else {
        transaction.update(PLAYERS, uuid, &ids)?;
    }
    Ok(())
}

fn save_pets(
    record: &mut CharacterRecord,
    pets: Vec<PetPersistenceData>,
    transaction: &WriteTransaction,
) -> Result<(), PersistenceError> {
    let mut saved = Vec::new();
    for (pet, body, _) in pets {
        if let Some(id) = pet.get_database_id().load()
            && let Some(existing) = record
                .pets
                .iter()
                .find(|stored| stored.database_id as u64 == id.get())
        {
            saved.push(existing.clone());
            continue;
        }
        let id = allocate(transaction, 1)?;
        let (variant, body_data) = convert_body_to_database_json(&body)?;
        saved.push(Pet {
            database_id: id,
            name: String::new(),
            body_variant: variant.to_string(),
            body_data,
        });
        pet.get_database_id().store(NonZeroU64::new(id as u64));
    }
    record.pets = saved;
    Ok(())
}

pub fn update(
    id: CharacterId,
    skills: comp::SkillSet,
    inventory: Inventory,
    pets: Vec<PetPersistenceData>,
    waypoint: Option<comp::Waypoint>,
    abilities: comp::ability::ActiveAbilities,
    marker: Option<comp::MapMarker>,
    transaction: &mut WriteTransaction,
) -> Result<(), PersistenceError> {
    let mut record = record(transaction, id)?;
    save_items(&mut record, &inventory, transaction)?;
    save_pets(&mut record, pets, transaction)?;
    record.skills = convert_skill_groups_to_database(id, skills.skill_groups());
    record.character.waypoint = convert_waypoint_to_database_json(waypoint, marker);
    record.abilities = convert_active_abilities_to_database(id, &abilities);
    transaction.update(CHARACTERS, &id.0.to_string(), &record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::{ConnectionMode, DatabaseSettings, SqlLogMode, establish_connection};
    use common::{comp::item::Item as GameItem, resources::Time};
    use std::path::PathBuf;
    use vek::{Vec2, Vec3};

    fn components(with_items: bool) -> PersistedComponents {
        let body = comp::Body::Humanoid(comp::humanoid::Body::iter().next().unwrap());
        let mut inventory = Inventory::with_empty();
        if with_items {
            inventory
                .push(GameItem::new_from_asset_expect("common.items.food.cheese"))
                .unwrap();
            inventory
                .push_recipe_group(GameItem::new_from_asset_expect(
                    "common.items.recipes.default",
                ))
                .unwrap();
        }
        PersistedComponents {
            body,
            hardcore: Some(comp::Hardcore),
            stats: comp::Stats::empty(body),
            skill_set: comp::SkillSet::default(),
            inventory,
            waypoint: Some(comp::Waypoint::new(Vec3::new(1.0, 2.0, 3.0), Time(0.0))),
            pets: vec![],
            active_abilities: comp::ActiveAbilities::default(),
            map_marker: Some(comp::MapMarker(Vec2::new(4, 5))),
        }
    }

    #[test]
    fn character_crud_round_trip_ownership_and_rollback() {
        let settings = DatabaseSettings::new(PathBuf::from("unused"), SqlLogMode::Disabled);
        let mut connection = establish_connection(&settings, ConnectionMode::ReadWrite);
        let mut transaction = connection.connection.transaction().unwrap();
        let (id, created) =
            create_character("owner", "First", components(true), &mut transaction).unwrap();
        assert_eq!(created.len(), 1);
        assert!(created[0].hardcore);
        transaction.commit().unwrap();
        let (loaded, metadata) = load_character_data("owner".into(), id, &connection).unwrap();
        assert!(metadata.skill_set_persistence_load_error.is_none());
        assert_eq!(
            loaded
                .inventory
                .slots()
                .filter(|slot| slot.is_some())
                .count(),
            1
        );
        assert_eq!(
            loaded
                .inventory
                .persistence_recipes_iter_with_index()
                .count(),
            1
        );
        assert_eq!(loaded.waypoint.unwrap().get_pos(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(loaded.map_marker.unwrap().0, Vec2::new(4, 5));
        assert!(load_character_data("other".into(), id, &connection).is_err());
        assert!(
            load_character_list("other", &connection)
                .unwrap()
                .is_empty()
        );

        let mut transaction = connection.connection.transaction().unwrap();
        assert!(
            edit_character(
                (loaded.body,),
                None,
                &mut transaction,
                id,
                "other",
                Some("Stolen")
            )
            .is_err()
        );
        delete_character("other", id, &mut transaction).unwrap();
        transaction.commit().unwrap();
        assert_eq!(
            load_character_list("owner", &connection).unwrap()[0]
                .character
                .alias,
            "First"
        );

        let mut transaction = connection.connection.transaction().unwrap();
        let (_, edited) = edit_character(
            (loaded.body,),
            None,
            &mut transaction,
            id,
            "owner",
            Some("Second"),
        )
        .unwrap();
        assert_eq!(edited[0].character.alias, "Second");
        drop(transaction);
        assert_eq!(
            load_character_list("owner", &connection).unwrap()[0]
                .character
                .alias,
            "First"
        );

        let mut transaction = connection.connection.transaction().unwrap();
        edit_character(
            (loaded.body,),
            None,
            &mut transaction,
            id,
            "owner",
            Some("Second"),
        )
        .unwrap();
        update(
            id,
            loaded.skill_set,
            Inventory::with_empty(),
            vec![],
            None,
            loaded.active_abilities,
            None,
            &mut transaction,
        )
        .unwrap();
        transaction.commit().unwrap();
        let (updated, _) = load_character_data("owner".into(), id, &connection).unwrap();
        assert!(updated.inventory.slots().all(|slot| slot.is_none()));
        assert!(updated.waypoint.is_none());
        assert!(updated.map_marker.is_none());
        assert_eq!(
            load_character_list("owner", &connection).unwrap()[0]
                .character
                .alias,
            "Second"
        );

        let mut transaction = connection.connection.transaction().unwrap();
        delete_character("owner", id, &mut transaction).unwrap();
        transaction.commit().unwrap();
        assert!(
            load_character_list("owner", &connection)
                .unwrap()
                .is_empty()
        );
        assert!(load_character_data("owner".into(), id, &connection).is_err());
        let read = connection.read().unwrap();
        assert!(read.get::<Vec<i64>>(PLAYERS, "owner").unwrap().is_none());
        let mut transaction = connection.connection.transaction().unwrap();
        let (next, _) =
            create_character("owner", "Next", components(false), &mut transaction).unwrap();
        assert!(next.0 > id.0);
    }

    #[test]
    fn character_limit_is_enforced_inside_the_transaction() {
        let settings = DatabaseSettings::new(PathBuf::from("unused"), SqlLogMode::Disabled);
        let mut connection = establish_connection(&settings, ConnectionMode::ReadWrite);
        let mut transaction = connection.connection.transaction().unwrap();
        for _ in 0..MAX_CHARACTERS_PER_PLAYER {
            create_character("owner", "Character", components(false), &mut transaction).unwrap();
        }
        assert!(matches!(
            create_character("owner", "Too many", components(false), &mut transaction),
            Err(PersistenceError::CharacterLimitReached)
        ));
        assert_eq!(
            list("owner", &transaction).unwrap().len(),
            MAX_CHARACTERS_PER_PLAYER as usize
        );
        transaction.commit().unwrap();
    }

    #[test]
    fn item_transfer_survives_deletion_of_the_previous_owner() {
        let settings = DatabaseSettings::new(PathBuf::from("unused"), SqlLogMode::Disabled);
        let mut connection = establish_connection(&settings, ConnectionMode::ReadWrite);
        let mut transaction = connection.connection.transaction().unwrap();
        let (source, _) =
            create_character("source", "Source", components(true), &mut transaction).unwrap();
        let (destination, _) = create_character(
            "destination",
            "Destination",
            components(false),
            &mut transaction,
        )
        .unwrap();
        transaction.commit().unwrap();
        let (mut loaded, _) = load_character_data("source".into(), source, &connection).unwrap();
        let slot = loaded
            .inventory
            .slots_with_id()
            .find(|(_, item)| item.is_some())
            .unwrap()
            .0;
        let item = loaded.inventory.remove(slot).unwrap();
        let item_id = item.get_item_id_for_database().load().unwrap().get() as i64;
        let mut inventory = Inventory::with_empty();
        inventory.push(item).unwrap();
        let mut transaction = connection.connection.transaction().unwrap();
        let mut target = record(&transaction, destination).unwrap();
        save_items(&mut target, &inventory, &transaction).unwrap();
        transaction
            .update(CHARACTERS, &destination.0.to_string(), &target)
            .unwrap();
        delete_character("source", source, &mut transaction).unwrap();
        transaction.commit().unwrap();
        let (loaded, _) =
            load_character_data("destination".into(), destination, &connection).unwrap();
        assert_eq!(
            loaded
                .inventory
                .slots()
                .filter(|slot| slot.is_some())
                .count(),
            1
        );
        let read = connection.read().unwrap();
        assert_eq!(
            read.get::<StoredItem>(ITEMS, &item_id.to_string())
                .unwrap()
                .unwrap()
                .owner,
            destination.0
        );
    }

    #[test]
    fn pets_round_trip_and_uncommitted_ids_can_be_retried() {
        let settings = DatabaseSettings::new(PathBuf::from("unused"), SqlLogMode::Disabled);
        let mut connection = establish_connection(&settings, ConnectionMode::ReadWrite);
        let mut transaction = connection.connection.transaction().unwrap();
        let (id, _) =
            create_character("owner", "Character", components(false), &mut transaction).unwrap();
        transaction.commit().unwrap();
        let pet = comp::Pet::default();
        let body = comp::Body::QuadrupedSmall(comp::quadruped_small::Body::random());
        let stats = comp::Stats::empty(body);
        let mut transaction = connection.connection.transaction().unwrap();
        let mut stored = record(&transaction, id).unwrap();
        save_pets(
            &mut stored,
            vec![(pet.clone(), body, stats.clone())],
            &transaction,
        )
        .unwrap();
        assert!(pet.get_database_id().load().is_some());
        transaction
            .update(CHARACTERS, &id.0.to_string(), &stored)
            .unwrap();
        drop(transaction);
        let mut transaction = connection.connection.transaction().unwrap();
        let mut stored = record(&transaction, id).unwrap();
        save_pets(&mut stored, vec![(pet.clone(), body, stats)], &transaction).unwrap();
        transaction
            .update(CHARACTERS, &id.0.to_string(), &stored)
            .unwrap();
        transaction.commit().unwrap();
        let (loaded, _) = load_character_data("owner".into(), id, &connection).unwrap();
        assert_eq!(loaded.pets.len(), 1);
        assert_eq!(loaded.pets[0].1, body);
        assert_eq!(
            loaded.pets[0].0.get_database_id().load(),
            pet.get_database_id().load()
        );
        let mut transaction = connection.connection.transaction().unwrap();
        let mut stored = record(&transaction, id).unwrap();
        save_pets(&mut stored, vec![], &transaction).unwrap();
        transaction
            .update(CHARACTERS, &id.0.to_string(), &stored)
            .unwrap();
        transaction.commit().unwrap();
        assert!(
            load_character_data("owner".into(), id, &connection)
                .unwrap()
                .0
                .pets
                .is_empty()
        );
    }
}
