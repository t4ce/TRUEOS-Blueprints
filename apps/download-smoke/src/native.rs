#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub fn run(_config: &crate::Config) {
    crate::emit(
        serde_json::json!({"schema":crate::SCHEMA,"event":"error","error":"native production HTTPS requires TRUEOS; --list inspects the finite matrix on a host"}),
    );
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod implementation {
    use crate::{Config, Route, SCHEMA, emit, stamp};
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::time::{Duration, Instant};
    use trueos::{
        netfs,
        ui4_scene::{self as ui, Error, Frame},
    };

    fn pause(ms: u64) {
        trueos::vsys::poll_once();
        trueos::vsys::sleep_ms(ms);
    }
    fn expired(start: Instant, config: &Config) -> bool {
        start.elapsed() >= Duration::from_millis(config.timeout_ms)
    }
    fn image_error(error: Error) -> String {
        format!("{error:?}")
    }

    pub fn run(config: &Config) {
        let run = stamp().to_string();
        emit(
            json!({"schema":SCHEMA,"event":"start","run":run,"urls":config.urls.iter().map(|url|crate::display_url(url)).collect::<Vec<_>>(),"routes":config.routes.iter().map(|r| r.name()).collect::<Vec<_>>(),
            "requests":config.urls.len()*config.routes.len(),"timeout_ms":config.timeout_ms,"clock":"lfence-rdtsc; coarse *_us uses 1ms ticks", "repeats":1,"warmup":0,
            "image_profile":"production Hull image service","general_profile":"production netfs fetch_bytes","http_status_source":"kernel per-hop diagnostics; API error codes are not HTTP statuses"}),
        );
        let frame = if config.routes.contains(&Route::Image) {
            match Frame::open(48, 48, 64, 64) {
                Ok(frame) => Some(frame),
                Err(error) => {
                    emit(
                        json!({"schema":SCHEMA,"event":"error","run":run,"stage":"frame-open","error":image_error(error)}),
                    );
                    return;
                }
            }
        } else {
            None
        };
        let mut complete = 0;
        let mut successful = 0;
        let mut case = 0;
        for route in &config.routes {
            for url in &config.urls {
                let mut record = json!({"schema":SCHEMA,"event":"request","run":run,"case":case,"url":crate::display_url(url),"route":route.name(),"elapsed_cycles":null,"elapsed_us":null,"code":null});
                emit(
                    json!({"schema":SCHEMA,"event":"begin","run":run,"case":case,"url":crate::display_url(url),"route":route.name()}),
                );
                let clock = Instant::now();
                let cycles = stamp();
                let result = match route {
                    Route::Image => image(
                        frame.as_ref().unwrap(),
                        config,
                        url,
                        case as u64 + 1,
                        &mut record,
                    ),
                    Route::General | Route::GeneralHappy => {
                        general(config, url, *route, &mut record)
                    }
                };
                record["elapsed_cycles"] = json!(stamp().saturating_sub(cycles));
                record["elapsed_us"] = json!(clock.elapsed().as_micros());
                match result {
                    Ok(success) => {
                        complete += 1;
                        successful += usize::from(success);
                        record["completed"] = json!(true);
                        record["api_success"] = json!(success);
                    }
                    Err(error) => {
                        record["completed"] = json!(false);
                        record["api_success"] = json!(false);
                        record["error"] = json!(error);
                    }
                }
                emit(record);
                case += 1;
                pause(1000);
            }
        }
        emit(
            json!({"schema":SCHEMA,"event":"summary","run":run,"cases":case,"completed":complete,"api_successes":successful,
            "interpretation":"inspect per-hop kernel HTTP status before classifying image API decoder rejection as download failure"}),
        );
    }

    fn general(
        config: &Config,
        url: &str,
        route: Route,
        record: &mut Value,
    ) -> Result<bool, String> {
        let operation = match route {
            Route::GeneralHappy => netfs::fetch_bytes_happy_eyeballs(url.as_bytes()),
            _ => netfs::fetch_bytes(url.as_bytes()),
        }
        .map_err(|code| format!("general admission failed: {code}"))?;
        record["operation"] = json!(operation);
        let started = Instant::now();
        let result = loop {
            match netfs::fetch_bytes_result_len(operation) {
                Err(-8) if !expired(started, config) => pause(1),
                Err(-8) => break Err(String::from("general bounded wait expired")),
                Err(code) => {
                    record["code"] = json!(code);
                    break Ok(false);
                }
                Ok(length) if length > 4 * 1024 * 1024 => {
                    break Err(String::from("general body exceeds diagnostic4MiB budget"));
                }
                Ok(_) => {
                    // Reading the body consumes the result slot. Capture the
                    // final redirect destination while its metadata is live.
                    match netfs::fetch_bytes_final_url(operation) {
                        Ok(url) => record["final_url"] = json!(crate::display_url(&url)),
                        Err(code) => {
                            record["stage"] = json!("final-url-metadata");
                            record["code"] = json!(code);
                            break Err(format!("general final URL metadata failed: {code}"));
                        }
                    }
                    match netfs::fetch_bytes_read(operation) {
                        Ok(bytes) => {
                            record["code"] = json!(0);
                            record["bytes"] = json!(bytes.len());
                            record["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
                            record["first16_hex"] = json!(
                                bytes
                                    .iter()
                                    .take(16)
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>()
                            );
                            record["png_signature"] =
                                json!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
                            break Ok(true);
                        }
                        Err(code) => {
                            record["code"] = json!(code);
                            break Ok(false);
                        }
                    }
                }
            }
        };
        let _ = netfs::fetch_bytes_discard(operation);
        result
    }

    fn image(
        frame: &Frame,
        config: &Config,
        url: &str,
        generation: u64,
        record: &mut Value,
    ) -> Result<bool, String> {
        let started = Instant::now();
        loop {
            match ui::hull_image_reset(frame.window_id(), generation) {
                Ok(()) => break,
                Err(Error::Busy) if !expired(started, config) => pause(1),
                Err(error) => return Err(format!("image reset: {}", image_error(error))),
            }
        }
        let ticket = loop {
            match ui::hull_image_request(
                frame.window_id(),
                url,
                "apps/download-smoke",
                "download-smoke",
                64,
                64,
                generation,
            ) {
                Ok(ticket) => break ticket,
                Err(Error::Busy) if !expired(started, config) => pause(1),
                Err(error) => return Err(format!("image admission: {}", image_error(error))),
            }
        };
        record["ticket"] = json!(ticket);
        record["window"] = json!(frame.window_id());
        let result = loop {
            match ui::hull_image_status(frame.window_id(), ticket) {
                Ok(info) if info.state == ui::HULL_IMAGE_PENDING => {
                    if expired(started, config) {
                        break Err(String::from("image bounded wait expired"));
                    }
                    pause(1);
                }
                Ok(info) => {
                    record["code"] = json!(info.error);
                    record["state"] = json!(info.state);
                    record["sprite"] = json!(info.sprite_id);
                    record["width"] = json!(info.width);
                    record["height"] = json!(info.height);
                    record["diagnostic_note"] = json!(
                        "READY proves fetch+decode+prepare+install; FAILED must be correlated with kernel per-hop HTTP diagnostics"
                    );
                    break Ok(info.state == ui::HULL_IMAGE_READY
                        && info.error == 0
                        && info.sprite_id != 0);
                }
                Err(Error::Busy) if !expired(started, config) => pause(1),
                Err(error) => break Err(format!("image status: {}", image_error(error))),
            }
        };
        let _ = ui::hull_image_release(frame.window_id(), ticket);
        result
    }
}
#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub use implementation::run;
