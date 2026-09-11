//! One redb image, serialized across users; acknowledge only after durable file IO.
use crate::plateau::{self, Create, Profile, Save};
use alloc::{format, string::String, sync::Arc, vec::Vec};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    routing::get,
};
use trueos::{async_fs as fs, tokio::sync::Mutex};
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition},
};

const USERS: TableDefinition<&str, &[u8]> = TableDefinition::new("cube_users");
const META: TableDefinition<&str, u64> = TableDefinition::new("metadata");
const MAX_DB_BYTES: u64 = 256 * 1024 * 1024;
fn db_error(error: impl core::fmt::Display) -> StatusCode {
    trueos::logl::log(
        trueos::logl::level::WARN,
        format_args!("cubesrv: profile database: {error}"),
    );
    StatusCode::INTERNAL_SERVER_ERROR
}
struct DatabaseState {
    database: Option<ImageDatabase>,
    durable: Vec<u8>,
}
pub struct Store {
    path: String,
    state: Mutex<Option<DatabaseState>>,
}
impl Store {
    pub fn new(path: &str) -> Self {
        Self {
            path: path.into(),
            state: Mutex::new(None),
        }
    }
    fn username(username: &str) -> Result<(), StatusCode> {
        plateau::valid_username(username)
            .then_some(())
            .ok_or(StatusCode::BAD_REQUEST)
    }
    async fn open(&self) -> Result<DatabaseState, StatusCode> {
        let bytes = match fs::metadata(self.path.as_bytes()).await {
            Ok(meta) => {
                if !meta.is_file() || meta.len == 0 || meta.len > MAX_DB_BYTES {
                    return Err(db_error("invalid database length"));
                }
                fs::read_file(self.path.as_bytes())
                    .await
                    .map_err(db_error)?
            }
            Err(fs::ERR_NOT_FOUND) => Vec::new(),
            Err(e) => return Err(db_error(e)),
        };
        let database = ImageDatabase::open(&bytes).map_err(db_error)?;
        if bytes.is_empty() {
            let tx = database.database().begin_write().map_err(db_error)?;
            tx.open_table(USERS).map_err(db_error)?;
            tx.open_table(META).map_err(db_error)?;
            tx.commit().map_err(db_error)?;
        }
        let mut state = DatabaseState {
            database: Some(database),
            durable: bytes,
        };
        if state.durable.is_empty() {
            self.persist(&mut state).await?;
        }
        Ok(state)
    }
    async fn persist(&self, state: &mut DatabaseState) -> Result<(), StatusCode> {
        // trueos-redb requires closing the instance before publishing its image.
        let database = state
            .database
            .take()
            .ok_or_else(|| db_error("database unavailable after IO failure"))?;
        let image = match database.into_image() {
            Ok(image) => image,
            Err(error) => {
                state.database = Some(ImageDatabase::open(&state.durable).map_err(db_error)?);
                return Err(db_error(error));
            }
        };
        let result = async {
            if image.len() as u64 > MAX_DB_BYTES {
                return Err(StatusCode::INSUFFICIENT_STORAGE);
            }
            let parent = self
                .path
                .rsplit_once('/')
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?
                .0;
            fs::create_dir_all(parent.as_bytes())
                .await
                .map_err(db_error)?;
            fs::write_file_typed(self.path.as_bytes(), &image, fs::ContentTypeId::BLOB)
                .await
                .map_err(db_error)
        }
        .await;
        // Failed persistence must not make an unacknowledged RAM revision visible.
        let live = if result.is_ok() {
            &image
        } else {
            &state.durable
        };
        state.database = Some(ImageDatabase::open(live).map_err(db_error)?);
        if result.is_ok() {
            state.durable = image;
        }
        result
    }
    fn read(state: &DatabaseState, username: &str) -> Result<Option<Profile>, StatusCode> {
        let tx = state
            .database
            .as_ref()
            .ok_or_else(|| db_error("database unavailable after IO failure"))?
            .database()
            .begin_read()
            .map_err(db_error)?;
        let table = tx.open_table(USERS).map_err(db_error)?;
        let Some(value) = table.get(username).map_err(db_error)? else {
            return Ok(None);
        };
        let profile: Profile = serde_json::from_slice(value.value()).map_err(db_error)?;
        if !profile.valid(username) {
            return Err(db_error("invalid profile record"));
        }
        Ok(Some(profile))
    }
    pub async fn load(&self, username: &str) -> Result<Profile, StatusCode> {
        Self::username(username)?;
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.open().await?);
        }
        Self::read(guard.as_ref().unwrap(), username)?.ok_or(StatusCode::NOT_FOUND)
    }
    pub async fn create(&self, username: &str, theme: u8) -> Result<Profile, StatusCode> {
        Self::username(username)?;
        if !(1..=6).contains(&theme) {
            return Err(StatusCode::BAD_REQUEST);
        }
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.open().await?);
        }
        let state = guard.as_mut().unwrap();
        if let Some(profile) = Self::read(state, username)? {
            return Ok(profile);
        }
        let tx = state
            .database
            .as_ref()
            .ok_or_else(|| db_error("database unavailable after IO failure"))?
            .database()
            .begin_write()
            .map_err(db_error)?;
        let generation = {
            let mut meta = tx.open_table(META).map_err(db_error)?;
            let n = meta
                .get("generation")
                .map_err(db_error)?
                .map_or(0, |v| v.value());
            let next = n.checked_add(1).ok_or(StatusCode::CONFLICT)?;
            meta.insert("generation", next).map_err(db_error)?;
            next
        };
        let profile = Profile::new(username, theme, generation).ok_or(StatusCode::BAD_REQUEST)?;
        let bytes = serde_json::to_vec(&profile).map_err(db_error)?;
        tx.open_table(USERS)
            .map_err(db_error)?
            .insert(username, bytes.as_slice())
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.persist(state).await?;
        Ok(profile)
    }
    pub async fn save(&self, username: &str, update: Save) -> Result<Profile, StatusCode> {
        Self::username(username)?;
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.open().await?);
        }
        let state = guard.as_mut().unwrap();
        let mut profile = Self::read(state, username)?.ok_or(StatusCode::NOT_FOUND)?;
        if profile.generation != update.generation {
            return Err(StatusCode::CONFLICT);
        }
        if profile.revision == update.revision.saturating_add(1) && profile.placed == update.placed
        {
            return Ok(profile);
        }
        if profile.revision != update.revision {
            return Err(StatusCode::CONFLICT);
        }
        if !plateau::valid_placements(&update.placed) || !update.placed.starts_with(&profile.placed)
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        profile.placed = update.placed;
        profile.revision = profile
            .revision
            .checked_add(1)
            .ok_or(StatusCode::CONFLICT)?;
        let bytes = serde_json::to_vec(&profile).map_err(db_error)?;
        if bytes.len() > plateau::MAX_PROFILE_BYTES {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        let tx = state
            .database
            .as_ref()
            .ok_or_else(|| db_error("database unavailable after IO failure"))?
            .database()
            .begin_write()
            .map_err(db_error)?;
        tx.open_table(USERS)
            .map_err(db_error)?
            .insert(username, bytes.as_slice())
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.persist(state).await?;
        Ok(profile)
    }
    pub async fn delete(&self, username: &str) -> Result<(), StatusCode> {
        Self::username(username)?;
        let mut guard = self.state.lock().await;
        if guard.is_none() {
            *guard = Some(self.open().await?);
        }
        let state = guard.as_mut().unwrap();
        let tx = state
            .database
            .as_ref()
            .ok_or_else(|| db_error("database unavailable after IO failure"))?
            .database()
            .begin_write()
            .map_err(db_error)?;
        tx.open_table(USERS)
            .map_err(db_error)?
            .remove(username)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        self.persist(state).await
    }
}
pub fn router(store: Arc<Store>) -> Router {
    Router::new()
        .route(
            "/plateau/{username}",
            get(load).post(create).put(save).delete(delete),
        )
        .layer(DefaultBodyLimit::max(plateau::MAX_PROFILE_BYTES))
        .with_state(store)
}
async fn load(
    State(store): State<Arc<Store>>,
    Path(username): Path<String>,
) -> Result<Json<Profile>, StatusCode> {
    store.load(&username).await.map(Json)
}
async fn create(
    State(store): State<Arc<Store>>,
    Path(username): Path<String>,
    Json(input): Json<Create>,
) -> Result<Json<Profile>, StatusCode> {
    store.create(&username, input.theme).await.map(Json)
}
async fn save(
    State(store): State<Arc<Store>>,
    Path(username): Path<String>,
    Json(input): Json<Save>,
) -> Result<Json<Profile>, StatusCode> {
    store.save(&username, input).await.map(Json)
}
async fn delete(
    State(store): State<Arc<Store>>,
    Path(username): Path<String>,
) -> Result<StatusCode, StatusCode> {
    store
        .delete(&username)
        .await
        .map(|()| StatusCode::NO_CONTENT)
}
