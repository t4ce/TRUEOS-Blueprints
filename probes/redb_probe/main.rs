use redb_probe as store;
use trueos::{
    logl::{self, level},
    platform,
};
use trueos_redb::ImageDatabase;

const DB_PATH: &str = "/common/usersettings.redb";

fn run_probe() -> Result<(usize, usize), String> {
    let (database, loaded_bytes) = store::open(DB_PATH)?;
    let image = store::persist(DB_PATH, database)?;
    store::verify(&ImageDatabase::open(&image)?)?;
    Ok((loaded_bytes, image.len()))
}

fn main() {
    match run_probe() {
        Ok((loaded_bytes, persisted_bytes)) => logl::log(
            level::INFO,
            format_args!(
                "redb-probe: PASS backend=redb-4.2.0 db_path={DB_PATH} loaded_bytes={loaded_bytes} persisted_bytes={persisted_bytes} users=1 settings=1 reopened=true"
            ),
        ),
        Err(error) => logl::log(level::ERROR, format_args!("redb-probe: FAIL {error}")),
    }
    platform::poll_once();
}
