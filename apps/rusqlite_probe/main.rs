use rusqlite_fork::{Connection, params};
use trueos::{logl, logl::level, platform};

fn run_probe() -> Result<(), rusqlite_fork::Error> {
    let conn = Connection::open_in_memory()?;
    let sqlite_version =
        conn.query_row("select sqlite_version()", [], |row| row.get::<_, String>(0))?;

    conn.execute(
        "create table notes (
            id integer primary key,
            title text not null,
            body text not null
        )",
        [],
    )?;
    conn.execute(
        "insert into notes (title, body) values (?1, ?2)",
        params!["blueprint", "rusqlite is alive inside TRUEOS"],
    )?;
    conn.execute(
        "insert into notes (title, body) values (?1, ?2)",
        params!["unix", "SQLite bundled C core linked into the app VM"],
    )?;
    conn.execute(
        "update notes set body = body || '!' where title = ?1",
        params!["blueprint"],
    )?;

    let count = conn.query_row("select count(*) from notes", [], |row| row.get::<_, i64>(0))?;
    let body = conn.query_row(
        "select body from notes where title = ?1",
        params!["blueprint"],
        |row| row.get::<_, String>(0),
    )?;

    logl::log(
        level::INFO,
        format_args!(
            "rusqlite-probe: sqlite_version={} rows={} blueprint_body={}",
            sqlite_version, count, body
        ),
    );
    Ok(())
}

fn main() {
    logl::log(level::INFO, "rusqlite-probe: blueprint start");
    match run_probe() {
        Ok(()) => logl::log(level::INFO, "rusqlite-probe: ok"),
        Err(err) => logl::log(level::ERROR, format_args!("rusqlite-probe: failed {}", err)),
    }
    platform::poll_once();
}
