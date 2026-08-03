use std::fmt::Write as _;
use std::time::Duration;
use serde_json::{Value, json};
use tokio::path::{Path, PathBuf};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(()) => {}
        Err(message) => {
            error(message.as_str());
            std::process::exit(1);
        }
    }
}