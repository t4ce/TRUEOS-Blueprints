use trueos_blueprint::prelude::{diag, runtime, task, time};

fn main() {
    diag::set_max_level(diag::Level::Trace);
    trueos_blueprint::log!("hello_world: building current-thread Tokio runtime", info);

    let runtime = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build Tokio runtime");

    runtime.block_on(async {
        trueos_blueprint::log!("hello_world: entered async body", info);
        task::yield_now().await;
        trueos_blueprint::log!("hello_world: after first yield", debug);
        time::sleep(time::Duration::from_millis(1)).await;
        trueos_blueprint::log!("hello_world: done", info);
    });
}
