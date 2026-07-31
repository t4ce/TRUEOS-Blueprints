mod data;

fn main() -> anyhow::Result<()> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        let result = player_scope::ui::run_vmx_minishell(data::config());
        if matches!(result.as_ref(), Ok(player_scope::ui::UiExit::Terminate)) {
            let _ = trueos::vshell::shutdown_current_blueprint("Player terminated");
        }
        return result.map(|_| ());
    }

    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    let result = player_scope::ui::run(data::config());
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    return result.map(|_| ());
}
