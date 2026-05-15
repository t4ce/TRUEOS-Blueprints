// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use core::error::Error as _;

use alloc::string::String;

use trueos::logl::{self, level};
use trueos_currency::{CurrencyAppConfig, FXFEED_URL, run_currency_app};

fn main() {
    run_currency_app(
        CurrencyAppConfig {
            transport_label: "reqwest",
            window_title: "Currency Reqwest",
            tex_id: 4_724,
            window_x: 210,
            window_y: 90,
            window_z: 39,
        },
        fetch_feed_text,
    );
}

async fn fetch_feed_text() -> Result<String, String> {
    logl::log(level::INFO, format_args!("currency_bp: stage reqwest.client.build"));
    let client = build_reqwest_client()?;
    logl::log(level::INFO, format_args!("currency_bp: success reqwest.client.build"));

    logl::log(level::INFO, format_args!("currency_bp: stage reqwest.request.send"));
    let response = client.get(FXFEED_URL).send().await.map_err(|err| {
        logl::log(
            level::ERROR,
            format_args!("currency_bp: reqwest request failed debug={:?}", err),
        );
        log_error_sources("reqwest request", &err);
        format!("request {err}")
    })?;
    let status = response.status();
    logl::log(
        level::INFO,
        format_args!("currency_bp: success reqwest.request.send status={}", status.as_u16()),
    );

    logl::log(level::INFO, format_args!("currency_bp: stage reqwest.response.bytes"));
    let body = response.bytes().await.map_err(|err| {
        logl::log(level::ERROR, format_args!("currency_bp: reqwest body failed debug={:?}", err));
        log_error_sources("reqwest body", &err);
        format!("body {err}")
    })?;
    logl::log(
        level::INFO,
        format_args!("currency_bp: success reqwest.response.bytes len={}", body.len()),
    );
    if !status.is_success() {
        return Err(format!("http {}", status.as_u16()));
    }
    String::from_utf8(body.to_vec()).map_err(|_| String::from("bad utf8"))
}

fn build_reqwest_client() -> Result<reqwest::Client, String> {
    logl::log(
        level::WARN,
        format_args!("currency_bp: stage reqwest.client.build.insecure_tls"),
    );
    reqwest::Client::builder()
        .tls_danger_accept_invalid_certs(true)
        .build()
        .map_err(|err| {
            logl::log(
                level::ERROR,
                format_args!("currency_bp: reqwest insecure builder failed debug={:?}", err),
            );
            if let Some(source) = err.source() {
                logl::log(
                    level::ERROR,
                    format_args!("currency_bp: reqwest insecure builder source={}", source),
                );
            }
            format!("client {err}")
        })
}

fn log_error_sources(label: &str, err: &(dyn core::error::Error + 'static)) {
    let mut source = err.source();
    let mut depth = 0usize;
    while let Some(error) = source {
        logl::log(level::ERROR, format_args!("currency_bp: {} source{}={}", label, depth, error));
        source = error.source();
        depth += 1;
        if depth >= 8 {
            break;
        }
    }
}
