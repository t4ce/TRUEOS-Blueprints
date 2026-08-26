// trueos-blueprint: features=["lifecycle-net"]

mod server;

use trueos::{
    logl::{self, level},
    platform, runtime, tokio,
};

fn main() {
    logl::log(level::INFO, "strudel-core-http: blueprint start");
    let runtime = match runtime::current_thread_net().build() {
        Ok(runtime) => runtime,
        Err(error) => {
            logl::log(
                level::ERROR,
                format_args!("strudel-core-http: runtime build failed {error}"),
            );
            return;
        }
    };
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=runtime-ready",
    );

    let local = tokio::task::LocalSet::new();
    logl::log(
        level::INFO,
        "strudel-core-http: startup stage=local-runtime-enter",
    );
    local.block_on(&runtime, async {
        if let Err(error) = server::run().await {
            logl::log(
                level::ERROR,
                format_args!("strudel-core-http: fatal integration error: {error}"),
            );
        }
    });
    platform::poll_once();
}
