//! One-shot launch stream: `pull NAME_OR_ID` (one request per line).
//! Empty lines and # comments are ignored; duplicates intentionally launch twice.
pub fn requests(script: &str) -> Vec<Result<String, String>> {
    script.lines().filter_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { return None; }
        Some(match line.split_once(char::is_whitespace) {
            Some(("pull" | "launch", selector)) if !selector.trim().is_empty() => Ok(selector.trim().to_string()),
            _ => Err(format!("invalid request: {line}")),
        })
    }).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn drains_multiple_requests_in_order_and_keeps_duplicates() {
        assert_eq!(requests("# batch\npull gridpaper\n\nlaunch cubes\r\npull gridpaper\n"), vec![Ok("gridpaper".into()),Ok("cubes".into()),Ok("gridpaper".into())]);
    }
    #[test] fn bad_lines_do_not_discard_later_requests() {
        let requests=requests("pull\nunknown cubes\npull 12\n");
        assert!(requests[0].is_err());assert!(requests[1].is_err());assert_eq!(requests[2],Ok("12".into()));
    }
    #[test] fn blank_script_is_an_empty_headless_batch() { assert!(requests("\n# empty\n").is_empty()); }
}
pub fn report(message: &str) {
    let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "apps", format_args!("appstore pullbot: {message}"));
}
pub async fn drain(bytes: &[u8]) {
    let script = match std::str::from_utf8(bytes) { Ok(script) => script, Err(_) => { report("startscript is not UTF-8"); return; } };
    let requests = requests(script);
    if requests.is_empty() { report("drained: 0 requests"); return; }
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(90)).build() {
        Ok(client) => client, Err(error) => { report(&format!("client: {error}")); return; }
    };
    let catalog = match super::fetch(&client, super::CATALOG, 1024 * 1024).await {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(html) => super::catalog::parse(html), Err(_) => { report("catalog is not UTF-8"); return; }
        },
        Err(error) => { report(&format!("catalog: {error}")); return; }
    };
    let mut queued = 0;
    let mut failed = 0;
    for request in requests {
        let result = async {
            let selector = request?;
            let app = super::catalog::resolve(&catalog, &selector).ok_or_else(||format!("not in catalog: {selector}"))?;
            let path = super::download(&client, app).await?;
            // Backpressure preserves requests when the host launch worker pool is full.
            for _ in 0..1800 {
                match trueos::vshell::launch_with_script(&path, "") {
                    Ok(()) => { report(&format!("queued {}",app.archive_name)); return Ok::<(),String>(()); }
                    Err(-16) => trueos::time::sleep(std::time::Duration::from_millis(25)).await,
                    Err(code) => return Err(format!("{}: launch enqueue failed ({code})", app.name)),
                }
            }
            Err("launch queue remained busy for 45 seconds".into())
        }.await;
        match result { Ok(()) => queued += 1, Err(error) => { failed += 1; report(&error); } }
    }
    report(&format!("drained: queued={queued} failed={failed}"));
}
