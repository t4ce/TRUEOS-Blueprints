//! WC3 diagnostics are opt-in. Fatal reports remain available in quiet builds.
pub use trueos::logl::level;

pub const ENABLED: bool = !cfg!(feature = "nolog")
    && (cfg!(feature = "diagnostics")
        || cfg!(feature = "trace-api")
        || cfg!(feature = "trace-seh")
        || cfg!(feature = "trace-scan")
        || cfg!(feature = "trace-init"));

#[inline(always)]
pub const fn enabled(level: u8) -> bool {
    level == self::level::ERROR || ENABLED
}

// Keep the condition outside format_args!: even argument expressions can read
// guest memory or allocate. A function accepting Arguments is too late.
macro_rules! log {
    ($level:expr, $message:expr $(,)?) => {{
        let level = $level;
        if $crate::logl::enabled(level) {
            $crate::logl::emit(level, $message);
        }
    }};
}
pub(crate) use log;

macro_rules! trace {
    ($feature:literal, $level:expr, $message:expr $(,)?) => {
        if $crate::logl::ENABLED && cfg!(feature = $feature) {
            $crate::logl::log!($level, $message);
        }
    };
}
pub(crate) use trace;

/// Explicit minishell replies and fatal diagnostics use the same retained sink.
#[inline]
pub fn emit(level: u8, message: core::fmt::Arguments<'_>) {
    trueos::logl::log(level, message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_reports_survive_quiet_and_nolog_builds() {
        assert!(enabled(level::ERROR));
        assert_eq!(enabled(level::IMPORTANT), ENABLED);
    }

    #[test]
    fn quiet_log_arguments_are_not_evaluated() {
        if ENABLED {
            return;
        }
        let evaluated = core::cell::Cell::new(false);
        log!(
            level::IMPORTANT,
            format_args!("{}", {
                evaluated.set(true);
                1
            })
        );
        assert!(!evaluated.get());
    }
}
