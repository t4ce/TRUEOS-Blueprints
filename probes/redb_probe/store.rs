use trueos::async_fs;
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition},
};

pub const USERS: TableDefinition<u64, &str> = TableDefinition::new("users");
pub const SETTINGS: TableDefinition<(u64, u64), &str> = TableDefinition::new("settings");

pub fn error(error: impl core::fmt::Display) -> String {
    format!("{error}")
}

pub fn open(path: &str) -> Result<(ImageDatabase, usize), String> {
    async_fs::block_on(async_fs::create_dir_all(b"/common")).map_err(error)?;
    let image = match async_fs::block_on(async_fs::read_file(path.as_bytes())) {
        Ok(image) if !image.is_empty() => image,
        Ok(_) => return Err("existing redb file is empty".into()),
        Err(async_fs::ERR_NOT_FOUND) => Vec::new(),
        Err(code) => return Err(format!("read database rc={code}")),
    };
    let loaded = image.len();
    let store = ImageDatabase::open(&image)?;
    let write = store.database().begin_write().map_err(error)?;
    {
        write
            .open_table(USERS)
            .map_err(error)?
            .insert(1, "blueprint-user")
            .map_err(error)?;
        write
            .open_table(SETTINGS)
            .map_err(error)?
            .insert((1, 1), "en")
            .map_err(error)?;
    }
    write.commit().map_err(error)?;
    verify(&store)?;
    Ok((store, loaded))
}

pub fn verify(store: &ImageDatabase) -> Result<(), String> {
    let read = store.database().begin_read().map_err(error)?;
    let users = read.open_table(USERS).map_err(error)?;
    let settings = read.open_table(SETTINGS).map_err(error)?;
    if users.len().map_err(error)? != 1
        || settings.len().map_err(error)? != 1
        || users
            .get(1)
            .map_err(error)?
            .is_none_or(|value| value.value() != "blueprint-user")
        || settings
            .get((1, 1))
            .map_err(error)?
            .is_none_or(|value| value.value() != "en")
    {
        return Err("redb user/settings round-trip mismatch".into());
    }
    Ok(())
}

/// Finish redb's cached writes before handing the image to async filesystem I/O.
pub fn persist(path: &str, store: ImageDatabase) -> Result<Vec<u8>, String> {
    let image = store.into_image()?;
    async_fs::block_on(async_fs::write_file(path.as_bytes(), &image)).map_err(error)?;
    let persisted = async_fs::block_on(async_fs::read_file(path.as_bytes())).map_err(error)?;
    if image != persisted {
        return Err("persisted redb image differs".into());
    }
    verify(&ImageDatabase::open(&persisted)?)?;
    Ok(image)
}
