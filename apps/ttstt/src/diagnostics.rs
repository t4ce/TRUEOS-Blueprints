//! Short work notices in the slot; detailed counters in the global log.
pub fn record(warning: bool, message: std::fmt::Arguments<'_>) {
    emit(warning, message, true);
}

pub fn detail(warning: bool, message: std::fmt::Arguments<'_>) {
    emit(warning, message, false);
}

fn emit(warning: bool, message: std::fmt::Arguments<'_>, visible: bool) {
    let text = message.to_string();
    #[cfg(target_os = "trueos")]
    {
        let level = if warning {
            trueos::logl::level::WARN
        } else {
            trueos::logl::level::INFO
        };
        if visible {
            trueos::logl::log(level, text.trim_end());
        }
        // These sparse lifecycle and per-request summaries must survive the
        // rig's Apps=Warn policy, which otherwise drops normal INFO records.
        let global_level = if warning {
            level
        } else {
            trueos::logl::level::IMPORTANT
        };
        let _ = trueos::logl::log_record(global_level, "blueprint/ttstt", text.trim_end());
    }
    #[cfg(not(target_os = "trueos"))]
    {
        let _ = warning;
        if visible {
            eprintln!("{}", text.trim_end());
        }
    }
}
