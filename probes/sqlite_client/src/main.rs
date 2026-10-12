use serde_json::{Value, json};
use std::time::Duration;
use trueos::{
    logl,
    logl::level,
    vshell::{self, LaunchDestination},
};

fn endpoint(instance: &str) -> String {
    let hash = instance.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    format!("http://127.0.0.1:{}", 20_000 + hash % 40_000)
}

async fn ready(client: &reqwest::Client, name: &str) -> Result<(), String> {
    for _ in 0..100 {
        if let Ok(response) = client
            .get(format!("{}/healthz", endpoint(name)))
            .send()
            .await
        {
            if let Ok(value) = response.json::<Value>().await {
                if value["instance"] == name
                    && value["endpoint"]
                        .as_str()
                        .is_some_and(|s| s.starts_with("http://localhost:"))
                {
                    return Ok(());
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!("instance {name} did not advertise localhost"))
}

async fn batch(
    client: &reqwest::Client,
    instance: &str,
    statements: Value,
    expected: u16,
) -> Result<Value, String> {
    let response = client
        .post(format!("{}/v1/batch", endpoint(instance)))
        .json(&json!({"database":"client_probe", "statements":statements}))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let value = response.json::<Value>().await.map_err(|e| e.to_string())?;
    if status != expected {
        return Err(format!("HTTP {status}: {value}"));
    }
    Ok(value)
}

async fn run() -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| e.to_string())?;
    ready(&client, "default").await?;
    logl::log(level::INFO, "sqlite-service-client: phase=default-ready");
    batch(&client, "default", json!([
        {"sql":"CREATE TABLE IF NOT EXISTS counter(id INTEGER PRIMARY KEY, value INTEGER NOT NULL)"},
        {"sql":"INSERT INTO counter VALUES(1,0) ON CONFLICT(id) DO UPDATE SET value=0"},
        {"sql":"DELETE FROM counter WHERE id != 1"}
    ]), 200).await?;
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            batch(
                &client,
                "default",
                json!([{"sql":"UPDATE counter SET value=value+1 WHERE id=1"}]),
                200,
            )
            .await
        }));
    }
    for task in tasks {
        task.await.map_err(|e| e.to_string())??;
    }
    let result = batch(
        &client,
        "default",
        json!([{"sql":"SELECT value FROM counter WHERE id=1"}]),
        200,
    )
    .await?;
    if result["results"][0]["rows"][0][0]["value"] != 8 {
        return Err(format!("counter mismatch {result}"));
    }
    logl::log(
        level::INFO,
        "sqlite-service-client: phase=concurrent-writers-pass",
    );
    batch(
        &client,
        "default",
        json!([
            {"sql":"INSERT INTO counter VALUES(2,99)"},
            {"sql":"INSERT INTO nonexistent VALUES(1)"}
        ]),
        400,
    )
    .await?;
    let result = batch(
        &client,
        "default",
        json!([{"sql":"SELECT count(*) FROM counter"}]),
        200,
    )
    .await?;
    if result["results"][0]["rows"][0][0]["value"] != 1 {
        return Err("rollback did not remove partial write".into());
    }
    logl::log(level::INFO, "sqlite-service-client: phase=rollback-pass");

    // Online Blueprints live in the download store rather than the built-in
    // AppDB. Ask the installed App Store to launch the named instance.
    vshell::launch_with_destination(
        "appstore",
        "launch --headless sqlite -- instance client-probe-second\n",
        LaunchDestination::Headless,
    )
    .map_err(|code| format!("launch second instance: {code}"))?;
    ready(&client, "client-probe-second").await?;
    let result = batch(
        &client,
        "client-probe-second",
        json!([
            {"sql":"CREATE TABLE IF NOT EXISTS counter(value INTEGER)"},
            {"sql":"DELETE FROM counter"},
            {"sql":"INSERT INTO counter VALUES(999)"},
            {"sql":"SELECT value FROM counter"}
        ]),
        200,
    )
    .await?;
    if result["results"][3]["rows"][0][0]["value"] != 999 {
        return Err("second instance data mismatch".into());
    }
    let result = batch(
        &client,
        "default",
        json!([{"sql":"SELECT value FROM counter WHERE id=1"}]),
        200,
    )
    .await?;
    if result["results"][0]["rows"][0][0]["value"] != 8 {
        return Err("instances shared database state".into());
    }
    Ok(())
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let result = runtime
        .map_err(|e| e.to_string())
        .and_then(|runtime| tokio::task::LocalSet::new().block_on(&runtime, run()));
    let report = match &result {
        Ok(()) => {
            json!({"result":"PASS", "localhost":true, "concurrent_writers":8, "rollback":true, "instances":2, "isolated":true})
        }
        Err(error) => json!({"result":"FAIL", "error":error}),
    };
    let _ = trueos::async_fs::block_on(trueos::async_fs::write_file(
        b"/common/sqlite-client-report.json",
        report.to_string().as_bytes(),
    ));
    match result {
        Ok(()) => logl::log(
            level::INFO,
            "sqlite-service-client: PASS localhost=true concurrent_writers=8 rollback=true instances=2 isolated=true",
        ),
        Err(error) => logl::log(
            level::ERROR,
            format_args!("sqlite-service-client: FAIL {error}"),
        ),
    }
}
