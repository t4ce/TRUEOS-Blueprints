// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use core::error::Error;
use core::time::Duration;

use alloc::string::String;

use trueos::logl::{self, level};
use trueos_currency::{CurrencyAppConfig, FXFEED_URL, run_currency_app};

fn main() {
    run_currency_app(
        CurrencyAppConfig {
            transport_label: "reqwest",
            window_title: "Currency Rates",
            tex_id: 4_724,
            window_x: 210,
            window_y: 90,
            window_z: 39,
        },
        fetch_feed_text,
    );
}

async fn fetch_feed_text() -> Result<String, String> {
    logl::log(
        level::WARN,
        format_args!("currency_bp: reqwest client build insecure_tls=accept_invalid_certs"),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(30_000))
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(|err| {
            logl::log(
                level::ERROR,
                format_args!("currency_bp: reqwest client failed: {}", err),
            );
            logl::log(
                level::ERROR,
                format_args!("currency_bp: reqwest client debug={:?}", err),
            );
            log_reqwest_sources("currency_bp: reqwest client", &err);
            format!("client {err}")
        })?;

    logl::log(
        level::INFO,
        format_args!("currency_bp: fetching live FX rates"),
    );
    let response = client.get(FXFEED_URL).send().await.map_err(|err| {
        logl::log(
            level::ERROR,
            format_args!("currency_bp: reqwest request failed: {}", err),
        );
        logl::log(
            level::ERROR,
            format_args!("currency_bp: reqwest request debug={:?}", err),
        );
        log_reqwest_sources("currency_bp: reqwest request", &err);
        format!("request {err}")
    })?;

    let status = response.status();
    if !status.is_success() {
        logl::log(
            level::ERROR,
            format_args!("currency_bp: fxfeed status={}", status),
        );
        return Err(format!("status {status}"));
    }

    let body = response.text().await.map_err(|err| {
        logl::log(
            level::ERROR,
            format_args!("currency_bp: response text failed: {}", err),
        );
        format!("body {err}")
    })?;
    logl::log(
        level::INFO,
        format_args!("currency_bp: rates received bytes={}", body.len()),
    );
    Ok(body)
}

fn log_reqwest_sources(prefix: &'static str, err: &reqwest::Error) {
    let mut depth = 0usize;
    let mut source = err.source();
    while let Some(cause) = source {
        logl::log(
            level::ERROR,
            format_args!("{} source[{}]: {}", prefix, depth, cause),
        );
        source = cause.source();
        depth += 1;
        if depth >= 4 {
            break;
        }
    }
}
