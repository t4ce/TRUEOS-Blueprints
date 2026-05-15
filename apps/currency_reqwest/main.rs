// trueos-blueprint: features=["tokio-net-probe"]

extern crate alloc;

use alloc::string::String;

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
    let client = reqwest::Client::builder()
        .build()
        .map_err(|err| format!("client {err}"))?;
    let response = client
        .get(FXFEED_URL)
        .send()
        .await
        .map_err(|err| format!("request {err}"))?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|err| format!("body {err}"))?;
    if !status.is_success() {
        return Err(format!("http {}", status.as_u16()));
    }
    String::from_utf8(body.to_vec()).map_err(|_| String::from("bad utf8"))
}