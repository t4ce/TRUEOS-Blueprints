// trueos-blueprint: features=["tokio-net-probe"]

mod ui;
mod weather;

use anyhow::Result;

fn main() -> Result<()> {
    let runtime = runtime()?;
    let initial = weather::demo_snapshot();
    let result = ui::run(initial, |status| {
        *status = String::from("refreshing live OpenWeather data");
        runtime.block_on(weather::load_weather_snapshot())
    });

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    trueos::vshell::leave_terminal_handoff();

    // Returning from a CLI blueprint hands control to the VMX minishell.  Do
    // not let Tokio's normal Runtime::drop teardown hold that handoff after
    // the TUI has already restored the primary terminal screen.
    runtime.shutdown_background();
    result
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn runtime() -> Result<trueos::runtime::Runtime> {
    Ok(trueos::runtime::current_thread_net().build()?)
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}
