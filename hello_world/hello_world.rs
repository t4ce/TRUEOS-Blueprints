use trueos_blueprint::prelude::{diag, runtime, task, time};

fn main() {
    diag::set_max_level(diag::Level::Trace);
    trueos_blueprint::bp_info!("hello_world: building current-thread Tokio runtime");

    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");

    runtime.block_on(async {
        trueos_blueprint::bp_info!("hello_world: entered async body");
        task::yield_now().await;
        trueos_blueprint::bp_debug!("hello_world: after first yield");
        time::sleep(time::Duration::from_millis(1)).await;
        trueos_blueprint::bp_info!("hello_world: done");
    });
}