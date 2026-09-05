use super::*;
use std::collections::BTreeSet;
use trueos_redb::{ImageDatabase, redb::{ReadableDatabase, ReadableTable, TableDefinition}};

const CIRCUITS: TableDefinition<&str, &[u8]> = TableDefinition::new("circuits");
const REVISIONS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("circuit_revisions");
const NEXT_REVISION: TableDefinition<&str, u64> = TableDefinition::new("next_revision");
const IMPORT_PATH: &str = "prismq-circuits-v1.json";
const OLD_DATABASE_PATH: &str = "prismq.sqlite3";

pub(super) struct CircuitDatabase {
    store: ImageDatabase,
    pub(super) existed_before_open: bool,
    pub(super) loaded_bytes: usize,
}

fn db_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("circuit database: {error}"))
}

#[derive(Deserialize)]
struct Import {
    schema: String,
    circuits: Vec<ImportedCircuit>,
}

#[derive(Deserialize)]
struct ImportedCircuit {
    name: String,
    document: JsonCircuit,
    revisions: Vec<ImportedRevision>,
}

#[derive(Deserialize)]
struct ImportedRevision {
    revision: u64,
    document: JsonCircuit,
}

impl CircuitDatabase {
    fn from_image(image: &[u8], existed: bool) -> Result<Self, ApiError> {
        let store = ImageDatabase::open(image).map_err(db_error)?;
        let write = store.database().begin_write().map_err(db_error)?;
        {
            write.open_table(CIRCUITS).map_err(db_error)?;
            write.open_table(REVISIONS).map_err(db_error)?;
            write.open_table(NEXT_REVISION).map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(Self { store, existed_before_open: existed, loaded_bytes: image.len() })
    }

    pub(super) async fn open() -> Result<Self, ApiError> {
        if app_fs::try_exists(CIRCUIT_DB_PATH).await.map_err(db_error)? {
            let image = app_fs::read(CIRCUIT_DB_PATH).await.map_err(db_error)?;
            if image.is_empty() { return Err(db_error("existing database image is empty")); }
            return Self::from_image(&image, true);
        }
        let import_exists = app_fs::try_exists(IMPORT_PATH).await.map_err(db_error)?;
        if !import_exists && app_fs::try_exists(OLD_DATABASE_PATH).await.map_err(db_error)? {
            return Err(db_error(
                "existing circuit database requires conversion: run tools/export_prismq_circuits.py on a copy and place prismq-circuits-v1.json beside it"
            ));
        }
        let database = Self::from_image(&[], false)?;
        let imported = if import_exists {
            let import: Import = serde_json::from_slice(&app_fs::read(IMPORT_PATH).await.map_err(db_error)?)
                .map_err(db_error)?;
            if import.schema != "prismq.circuit-export.v1" { return Err(db_error("unknown circuit export schema")); }
            database.import(import.circuits)?
        } else {
            database.import_legacy_json().await?
        };
        // Persist an empty new database too: a fresh start is an explicit state,
        // not a reason to import an old JSON tree on every later request.
        let image = database.persisted_image()?;
        persist_circuit_database(image.clone()).await?;
        println!("prismq: redb initialized imported_circuits={imported}");
        Self::from_image(&image, false)
    }

    pub(super) fn persisted_image(self) -> Result<Vec<u8>, ApiError> {
        self.store.into_image().map_err(db_error)
    }

    pub(super) fn revision_count(&self) -> Result<usize, ApiError> {
        let read = self.store.database().begin_read().map_err(db_error)?;
        let count = read.open_table(REVISIONS).map_err(db_error)?.len().map_err(db_error)?;
        usize::try_from(count).map_err(db_error)
    }

    pub(super) fn list(&self) -> Result<Vec<serde_json::Value>, ApiError> {
        let read = self.store.database().begin_read().map_err(db_error)?;
        let circuits = read.open_table(CIRCUITS).map_err(db_error)?;
        let revisions = read.open_table(REVISIONS).map_err(db_error)?;
        let mut rows = Vec::new();
        for row in circuits.iter().map_err(db_error)? {
            let (name, document) = row.map_err(db_error)?;
            let name = name.value().to_owned();
            let circuit: JsonCircuit = serde_json::from_slice(document.value()).map_err(db_error)?;
            let mut history = Vec::new();
            for revision in revisions.range((name.as_str(), 0)..=(name.as_str(), u64::MAX)).map_err(db_error)? {
                history.push(revision.map_err(db_error)?.0.value().1);
            }
            rows.push((name, circuit.qubits, history));
        }
        rows.sort_by(|left, right| (left.0.to_ascii_lowercase(), &left.0)
            .cmp(&(right.0.to_ascii_lowercase(), &right.0)));
        Ok(rows.into_iter().map(|(name, qubits, revisions)| serde_json::json!({
            "name": name, "qubits": qubits, "revisions": revisions,
        })).collect())
    }

    pub(super) fn load(&self, name: &str, revision: Option<usize>) -> Result<Option<JsonCircuit>, ApiError> {
        let read = self.store.database().begin_read().map_err(db_error)?;
        let bytes = match revision {
            Some(revision) => read.open_table(REVISIONS).map_err(db_error)?
                .get((name, revision as u64)).map_err(db_error)?.map(|value| value.value().to_vec()),
            None => read.open_table(CIRCUITS).map_err(db_error)?
                .get(name).map_err(db_error)?.map(|value| value.value().to_vec()),
        };
        bytes.map(|bytes| serde_json::from_slice(&bytes).map_err(db_error)).transpose()
    }

    pub(super) fn save(&mut self, name: &str, circuit: &JsonCircuit) -> Result<Option<usize>, ApiError> {
        let bytes = serde_json::to_vec_pretty(circuit).map_err(db_error)?;
        let write = self.store.database().begin_write().map_err(db_error)?;
        let archived;
        {
            let mut circuits = write.open_table(CIRCUITS).map_err(db_error)?;
            let mut revisions = write.open_table(REVISIONS).map_err(db_error)?;
            let mut next = write.open_table(NEXT_REVISION).map_err(db_error)?;
            let previous = circuits.get(name).map_err(db_error)?.map(|value| value.value().to_vec());
            archived = if let Some(previous) = previous {
                let revision = next.get(name).map_err(db_error)?.map_or(1, |value| value.value());
                let following = revision.checked_add(1).ok_or_else(|| db_error("revision counter overflow"))?;
                revisions.insert((name, revision), previous.as_slice()).map_err(db_error)?;
                next.insert(name, following).map_err(db_error)?;
                Some(usize::try_from(revision).map_err(db_error)?)
            } else { None };
            circuits.insert(name, bytes.as_slice()).map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(archived)
    }

    pub(super) fn delete(&mut self, name: &str) -> Result<bool, ApiError> {
        let write = self.store.database().begin_write().map_err(db_error)?;
        let deleted;
        {
            let mut circuits = write.open_table(CIRCUITS).map_err(db_error)?;
            let mut revisions = write.open_table(REVISIONS).map_err(db_error)?;
            deleted = circuits.remove(name).map_err(db_error)?.is_some();
            let keys = revisions.range((name, 0)..=(name, u64::MAX)).map_err(db_error)?
                .map(|entry| entry.map(|(key, _)| key.value().1)).collect::<Result<Vec<_>, _>>().map_err(db_error)?;
            for revision in keys { revisions.remove((name, revision)).map_err(db_error)?; }
            write.open_table(NEXT_REVISION).map_err(db_error)?.remove(name).map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(deleted)
    }

    fn import(&self, imported: Vec<ImportedCircuit>) -> Result<usize, ApiError> {
        let write = self.store.database().begin_write().map_err(db_error)?;
        let mut names = BTreeSet::new();
        {
            let mut circuits = write.open_table(CIRCUITS).map_err(db_error)?;
            let mut revisions = write.open_table(REVISIONS).map_err(db_error)?;
            let mut next = write.open_table(NEXT_REVISION).map_err(db_error)?;
            for circuit in &imported {
                let name = normalize_circuit_name(&circuit.name)?;
                if name != circuit.name || !names.insert(name.clone()) { return Err(db_error("duplicate or noncanonical imported circuit name")); }
                validate_circuit_document(&circuit.document)?;
                let bytes = serde_json::to_vec_pretty(&circuit.document).map_err(db_error)?;
                circuits.insert(name.as_str(), bytes.as_slice()).map_err(db_error)?;
                let mut seen = BTreeSet::new();
                let mut following = 1;
                for history in &circuit.revisions {
                    if history.revision == 0 || !seen.insert(history.revision) { return Err(db_error("duplicate or zero imported revision")); }
                    following = following.max(history.revision.checked_add(1).ok_or_else(|| db_error("revision counter overflow"))?);
                    validate_circuit_document(&history.document)?;
                    let bytes = serde_json::to_vec_pretty(&history.document).map_err(db_error)?;
                    revisions.insert((name.as_str(), history.revision), bytes.as_slice()).map_err(db_error)?;
                }
                next.insert(name.as_str(), following).map_err(db_error)?;
            }
        }
        write.commit().map_err(db_error)?;
        Ok(imported.len())
    }

    async fn import_legacy_json(&self) -> Result<usize, ApiError> {
        if !app_fs::try_exists(LEGACY_CIRCUIT_INDEX_PATH).await.map_err(db_error)? { return Ok(0); }
        let names: Vec<String> = serde_json::from_slice(&app_fs::read(LEGACY_CIRCUIT_INDEX_PATH).await.map_err(db_error)?).map_err(db_error)?;
        let mut imported = Vec::new();
        for name in names {
            let name = normalize_circuit_name(&name)?;
            let document = serde_json::from_slice(&app_fs::read(format!("circuits/{name}")).await.map_err(db_error)?).map_err(db_error)?;
            let mut revisions = Vec::new();
            for revision in 1..=u64::MAX {
                let path = format!("circuits/{name}_rev{revision}");
                if !app_fs::try_exists(&path).await.map_err(db_error)? { break; }
                let document = serde_json::from_slice(&app_fs::read(path).await.map_err(db_error)?).map_err(db_error)?;
                revisions.push(ImportedRevision { revision, document });
            }
            imported.push(ImportedCircuit { name, document, revisions });
        }
        self.import(imported)
    }
}

pub(super) async fn persist_circuit_database(bytes: Vec<u8>) -> Result<usize, ApiError> {
    let len = bytes.len();
    let staging = format!("{CIRCUIT_DB_PATH}.next");
    app_fs::write(&staging, &bytes).await.map_err(db_error)?;
    app_fs::rename(&staging, CIRCUIT_DB_PATH).await.map_err(db_error)?;
    println!("prismq: redb image persisted path={CIRCUIT_DB_PATH} bytes={len}");
    Ok(len)
}
