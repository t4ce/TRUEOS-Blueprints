use super::*;
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition},
};

const CIRCUITS: TableDefinition<&str, &[u8]> = TableDefinition::new("circuits");
const REVISIONS: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("circuit_revisions");
const NEXT_REVISION: TableDefinition<&str, u64> = TableDefinition::new("next_revision");

pub(super) struct CircuitDatabase {
    store: ImageDatabase,
    pub(super) existed_before_open: bool,
    pub(super) loaded_bytes: usize,
}

fn db_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(format!("circuit database: {error}"))
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
        Ok(Self {
            store,
            existed_before_open: existed,
            loaded_bytes: image.len(),
        })
    }

    pub(super) async fn open() -> Result<Self, ApiError> {
        if app_fs::try_exists(CIRCUIT_DB_PATH)
            .await
            .map_err(db_error)?
        {
            let image = app_fs::read(CIRCUIT_DB_PATH).await.map_err(db_error)?;
            if image.is_empty() {
                return Err(db_error("existing database image is empty"));
            }
            return Self::from_image(&image, true);
        }
        let database = Self::from_image(&[], false)?;
        let image = database.persisted_image()?;
        persist_circuit_database(image.clone()).await?;
        println!("prismq: redb initialized");
        Self::from_image(&image, false)
    }

    pub(super) fn persisted_image(self) -> Result<Vec<u8>, ApiError> {
        self.store.into_image().map_err(db_error)
    }

    pub(super) fn revision_count(&self) -> Result<usize, ApiError> {
        let read = self.store.database().begin_read().map_err(db_error)?;
        let count = read
            .open_table(REVISIONS)
            .map_err(db_error)?
            .len()
            .map_err(db_error)?;
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
            let circuit: JsonCircuit =
                serde_json::from_slice(document.value()).map_err(db_error)?;
            let mut history = Vec::new();
            for revision in revisions
                .range((name.as_str(), 0)..=(name.as_str(), u64::MAX))
                .map_err(db_error)?
            {
                history.push(revision.map_err(db_error)?.0.value().1);
            }
            rows.push((name, circuit.qubits, history));
        }
        rows.sort_by(|left, right| {
            (left.0.to_ascii_lowercase(), &left.0).cmp(&(right.0.to_ascii_lowercase(), &right.0))
        });
        Ok(rows
            .into_iter()
            .map(|(name, qubits, revisions)| {
                serde_json::json!({
                    "name": name, "qubits": qubits, "revisions": revisions,
                })
            })
            .collect())
    }

    pub(super) fn load(
        &self,
        name: &str,
        revision: Option<usize>,
    ) -> Result<Option<JsonCircuit>, ApiError> {
        let read = self.store.database().begin_read().map_err(db_error)?;
        let bytes = match revision {
            Some(revision) => read
                .open_table(REVISIONS)
                .map_err(db_error)?
                .get((name, revision as u64))
                .map_err(db_error)?
                .map(|value| value.value().to_vec()),
            None => read
                .open_table(CIRCUITS)
                .map_err(db_error)?
                .get(name)
                .map_err(db_error)?
                .map(|value| value.value().to_vec()),
        };
        bytes
            .map(|bytes| serde_json::from_slice(&bytes).map_err(db_error))
            .transpose()
    }

    pub(super) fn save(
        &mut self,
        name: &str,
        circuit: &JsonCircuit,
    ) -> Result<Option<usize>, ApiError> {
        let bytes = serde_json::to_vec_pretty(circuit).map_err(db_error)?;
        let write = self.store.database().begin_write().map_err(db_error)?;
        let archived;
        {
            let mut circuits = write.open_table(CIRCUITS).map_err(db_error)?;
            let mut revisions = write.open_table(REVISIONS).map_err(db_error)?;
            let mut next = write.open_table(NEXT_REVISION).map_err(db_error)?;
            let previous = circuits
                .get(name)
                .map_err(db_error)?
                .map(|value| value.value().to_vec());
            archived = if let Some(previous) = previous {
                let revision = next
                    .get(name)
                    .map_err(db_error)?
                    .map_or(1, |value| value.value());
                let following = revision
                    .checked_add(1)
                    .ok_or_else(|| db_error("revision counter overflow"))?;
                revisions
                    .insert((name, revision), previous.as_slice())
                    .map_err(db_error)?;
                next.insert(name, following).map_err(db_error)?;
                Some(usize::try_from(revision).map_err(db_error)?)
            } else {
                None
            };
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
            let keys = revisions
                .range((name, 0)..=(name, u64::MAX))
                .map_err(db_error)?
                .map(|entry| entry.map(|(key, _)| key.value().1))
                .collect::<Result<Vec<_>, _>>()
                .map_err(db_error)?;
            for revision in keys {
                revisions.remove((name, revision)).map_err(db_error)?;
            }
            write
                .open_table(NEXT_REVISION)
                .map_err(db_error)?
                .remove(name)
                .map_err(db_error)?;
        }
        write.commit().map_err(db_error)?;
        Ok(deleted)
    }

}

pub(super) async fn persist_circuit_database(bytes: Vec<u8>) -> Result<usize, ApiError> {
    let len = bytes.len();
    app_fs::write(CIRCUIT_DB_PATH, &bytes).await.map_err(db_error)?;
    println!("prismq: redb image persisted path={CIRCUIT_DB_PATH} bytes={len}");
    Ok(len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circuit(seed: u64) -> JsonCircuit {
        serde_json::from_value(serde_json::json!({"qubits": 2, "seed": seed, "gates": []})).unwrap()
    }

    #[test]
    fn current_and_archived_documents_survive_image_reopen() {
        let mut database = CircuitDatabase::from_image(&[], false).unwrap();
        assert_eq!(database.save("Bell", &circuit(1)).unwrap(), None);
        assert_eq!(database.save("Bell", &circuit(2)).unwrap(), Some(1));
        let image = database.persisted_image().unwrap();
        let mut restored = CircuitDatabase::from_image(&image, true).unwrap();
        assert_eq!(restored.load("Bell", None).unwrap().unwrap().seed, 2);
        assert_eq!(restored.load("Bell", Some(1)).unwrap().unwrap().seed, 1);
        assert_eq!(restored.save("Bell", &circuit(3)).unwrap(), Some(2));
    }

    #[test]
    fn delete_removes_history_and_resets_revision_numbering() {
        let mut database = CircuitDatabase::from_image(&[], false).unwrap();
        database.save("Bell", &circuit(1)).unwrap();
        database.save("Bell", &circuit(2)).unwrap();
        assert!(database.delete("Bell").unwrap());
        assert!(!database.delete("Bell").unwrap());
        assert_eq!(database.revision_count().unwrap(), 0);
        assert!(database.load("Bell", Some(1)).unwrap().is_none());
        database.save("Bell", &circuit(3)).unwrap();
        assert_eq!(database.save("Bell", &circuit(4)).unwrap(), Some(1));
    }

}
