//! One-shot launch stream: `pull [--sh3] NAME_OR_ID` (one request per line).
//! Empty lines and # comments are ignored; duplicates intentionally launch twice.
#[derive(Debug, PartialEq, Eq)]
pub struct Request {
    pub selector: String,
    pub start_script: String,
    pub new_shell3: bool,
}
/// `launch NAME -- COMMAND` forwards COMMAND as the downloaded app's start script.
/// `launch --sh3 NAME -- COMMAND` also creates a kernel-owned Shell3 window.
/// Plain `pull NAME` and `launch NAME` retain their existing behavior.
pub fn requests(script: &str) -> Vec<Result<Request, String>> {
    script
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            Some(match line.split_once(char::is_whitespace) {
                Some(("pull" | "launch", value)) if !value.trim().is_empty() => {
                    let value = value.trim();
                    let (new_shell3, value) = match value.strip_prefix("--sh3") {
                        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => (true, rest.trim()),
                        _ => (false, value),
                    };
                    let (selector, command) = value
                        .trim()
                        .split_once(" -- ")
                        .map_or((value.trim(), ""), |(selector, command)| {
                            (selector.trim(), command.trim())
                        });
                    if selector.is_empty() || (value.contains(" -- ") && command.is_empty()) {
                        Err(format!("invalid request: {line}"))
                    } else {
                        Ok(Request {
                            selector: selector.to_string(),
                            new_shell3,
                            start_script: if command.is_empty() {
                                String::new()
                            } else {
                                format!("{command}\n")
                            },
                        })
                    }
                }
                _ => Err(format!("invalid request: {line}")),
            })
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn plain(selector: &str) -> Result<Request, String> {
        Ok(Request {
            selector: selector.into(),
            start_script: String::new(),
            new_shell3: false,
        })
    }
    #[test]
    fn forwards_frog_command_without_consuming_its_coordinates() {
        assert_eq!(
            requests("launch Frog -- weather -74.123456 40.987654\n"),
            vec![Ok(Request {
                selector: "Frog".into(),
                start_script: "weather -74.123456 40.987654\n".into(),
                new_shell3: false,
            })]
        );
    }
    #[test]
    fn forwards_arbitrary_commands_and_preserves_other_requests() {
        let batch = requests(
            "pull cubes\nlaunch solara -- open https://example.com/a?q=x -- y\npull Frog\n",
        );
        assert_eq!(batch[0], plain("cubes"));
        assert_eq!(
            batch[1].as_ref().unwrap().start_script,
            "open https://example.com/a?q=x -- y\n"
        );
        assert_eq!(batch[2], plain("Frog"));
    }

    #[test]
    fn shell_destination_does_not_change_the_child_script() {
        let batch = requests("launch --sh3 Frog -- weather 13 51\npull --sh3 12\nlaunch --sh3\n");
        assert_eq!(batch[0], Ok(Request { selector: "Frog".into(), start_script: "weather 13 51\n".into(), new_shell3: true }));
        assert_eq!(batch[1], Ok(Request { selector: "12".into(), start_script: String::new(), new_shell3: true }));
        assert!(batch[2].is_err());
        assert!(!requests("launch Frog -- weather --sh3 51\n")[0].as_ref().unwrap().new_shell3);
    }

    #[test]
    fn drains_multiple_requests_in_order_and_keeps_duplicates() {
        assert_eq!(
            requests("# batch\npull gridpaper\n\nlaunch cubes\r\npull gridpaper\n"),
            vec![plain("gridpaper"), plain("cubes"), plain("gridpaper")]
        );
    }
    #[test]
    fn bad_lines_do_not_discard_later_requests() {
        let requests = requests("pull\nunknown cubes\npull 12\n");
        assert!(requests[0].is_err());
        assert!(requests[1].is_err());
        assert_eq!(requests[2], plain("12"));
    }
    #[test]
    fn blank_script_is_an_empty_headless_batch() {
        assert!(requests("\n# empty\n").is_empty());
    }
}
pub fn report(message: &str) {
    let _ = trueos::logl::log_record(
        trueos::logl::level::IMPORTANT,
        "apps",
        format_args!("appstore pullbot: {message}"),
    );
}
pub async fn drain(bytes: &[u8]) {
    let script = match std::str::from_utf8(bytes) {
        Ok(script) => script,
        Err(_) => {
            report("startscript is not UTF-8");
            return;
        }
    };
    let requests = requests(script);
    if requests.is_empty() {
        report("drained: 0 requests");
        return;
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            report(&format!("client: {error}"));
            return;
        }
    };
    let catalog = match super::fetch(&client, super::CATALOG, 1024 * 1024).await {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(html) => super::catalog::parse(html),
            Err(_) => {
                report("catalog is not UTF-8");
                return;
            }
        },
        Err(error) => {
            report(&format!("catalog: {error}"));
            return;
        }
    };
    let mut queued = 0;
    let mut failed = 0;
    for request in requests {
        let result = async {
            let Request {
                selector,
                start_script,
                new_shell3,
            } = request?;
            let app = super::catalog::resolve(&catalog, &selector)
                .ok_or_else(|| format!("not in catalog: {selector}"))?;
            let path = super::download(&client, app).await?;
            // Backpressure preserves requests when the host launch worker pool is full.
            for _ in 0..1800 {
                let destination = if new_shell3 {
                    trueos::vshell::LaunchDestination::NewShell3
                } else {
                    trueos::vshell::LaunchDestination::CurrentShell
                };
                match trueos::vshell::launch_with_destination(&path, &start_script, destination) {
                    Ok(()) => {
                        report(&format!("queued {}", app.archive_name));
                        return Ok::<(), String>(());
                    }
                    Err(-16) => trueos::time::sleep(std::time::Duration::from_millis(25)).await,
                    Err(code) => {
                        return Err(format!("{}: launch enqueue failed ({code})", app.name));
                    }
                }
            }
            Err("launch queue remained busy for 45 seconds".into())
        }
        .await;
        match result {
            Ok(()) => queued += 1,
            Err(error) => {
                failed += 1;
                report(&error);
            }
        }
    }
    report(&format!("drained: queued={queued} failed={failed}"));
}
