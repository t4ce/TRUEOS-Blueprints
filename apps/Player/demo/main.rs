mod data;

fn main() -> anyhow::Result<()> {
    let result = player_scope::ui::run(data::config());

    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        trueos::vshell::leave_terminal_handoff();
        let _ = trueos::vshell::shutdown_current_blueprint("Player exited");
    }

    result
}
