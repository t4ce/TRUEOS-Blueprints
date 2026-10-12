use std::{env, fs, path::Path};
fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).ancestors()
        .find(|p| p.join("apps.json").is_file() && p.join("probes.json").is_file()).unwrap();
    let state = root.parent().unwrap().join("codex/codex-rs/state");
    let mut generated = String::from("fn migrators() -> Vec<(&'static str, sqlx::migrate::Migrator)> { vec![\n");
    for name in ["migrations", "logs_migrations", "goals_migrations", "memory_migrations", "queue_migrations", "thread_history_migrations"] {
        let path = state.join(name);
        println!("cargo:rerun-if-changed={}", path.display());
        generated.push_str(&format!("({name:?}, sqlx::migrate::Migrator::with_migrations(vec![\n"));
        for (migration, _) in sqlx_core::migrate::resolve_blocking(&path).unwrap() {
            generated.push_str(&format!("sqlx::migrate::Migration::new({}, {:?}.into(), sqlx::migrate::MigrationType::{:?}, sqlx::SqlStr::from_static({:?}), {}),\n", migration.version, migration.description, migration.migration_type, migration.sql.as_str(), migration.no_tx));
        }
        generated.push_str("])),\n");
    }
    generated.push_str("] }\n");
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("migrators.rs"), generated).unwrap();
}
