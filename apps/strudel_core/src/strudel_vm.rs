extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::fmt::Write as _;

use trueos_qjs::workbench::{EvalMode, Workbench};

use crate::native_rows::parse_native_command_rows;
use crate::PerformanceInputV1;
use trueos::audio::NativeRenderCommandV2;

const UPSTREAM_BUNDLE: &str = include_str!("../js/vendor/strudel-core.bundle.js");
const FALLBACK_CORE: &str = include_str!("../js/00_fallback_core.js");
const INSTRUMENT_CATALOG: &str = include_str!("../js/instrument_catalog.js");
const TRUEOS_ADAPTER: &str = include_str!("../js/10_trueos_adapter.js");
const DEMO_PATTERN: &str = include_str!("../js/20_demo_pattern.js");

pub struct InstallReport {
    pub core_status_json: String,
    pub initial_source: &'static str,
}

pub struct StrudelVm {
    workbench: Workbench,
}

impl StrudelVm {
    pub const fn new() -> Self {
        Self {
            workbench: Workbench::new(),
        }
    }

    pub fn install(&mut self) -> Result<InstallReport, String> {
        self.eval_unit(UPSTREAM_BUNDLE, "upstream bundle")?;
        self.eval_unit(FALLBACK_CORE, "fallback temporal kernel")?;
        self.eval_unit(INSTRUMENT_CATALOG, "TRUEOS instrument catalog")?;
        self.eval_unit(TRUEOS_ADAPTER, "TRUEOS pattern adapter")?;

        let smoke = self.eval(
            "globalThis.__TRUEOS_STRUDEL.selfTest()",
            "Strudel temporal smoke test",
        )?;
        if smoke != "\"a@0.000000-0.500000|b@0.500000-0.750000|c@0.750000-1.000000\"" {
            return Err(format!("unexpected temporal smoke result: {smoke}"));
        }

        let core_status_json = self.commit_expression(DEMO_PATTERN)?;
        Ok(InstallReport {
            core_status_json,
            initial_source: DEMO_PATTERN,
        })
    }

    /// Transactionally replace the active pattern with one JavaScript
    /// expression. The adapter validates the result before assignment, so any
    /// syntax/runtime/type error leaves the previous pattern sounding.
    pub fn commit_expression(&mut self, source: &str) -> Result<String, String> {
        let literal = js_string_literal(source);
        self.eval(
            &format!("globalThis.__TRUEOS_STRUDEL.commitExpression({literal})"),
            "live pattern commit",
        )
    }

    /// Query typed ABI commands from the VM boundary. The current adapter's
    /// Legacy and V1 rows are accepted by `native_rows`; the adapter emits the
    /// documented fixed-width V2 schema for native envelope semantics.
    pub fn query_native_commands(
        &mut self,
        absolute_start_frame: u64,
        block_frames: u32,
        sample_rate_hz: u32,
    ) -> Result<Vec<NativeRenderCommandV2>, String> {
        let source = format!(
            "globalThis.__TRUEOS_STRUDEL.queryFrames({absolute_start_frame},{block_frames},{sample_rate_hz})"
        );
        let text = self.eval(&source, "pattern query")?;
        parse_native_command_rows(&text)
            .map_err(|error| format!("native-command parse failed: {error}; text={text}"))
    }

    /// Install input edges into the persistent VM before querying patterns.
    /// The integer matrix is deliberately independent of QuickJS object ABI.
    pub fn apply_performance_inputs(
        &mut self,
        inputs: &[PerformanceInputV1],
    ) -> Result<(), String> {
        let mut source = String::from("globalThis.__TRUEOS_STRUDEL.applyInputs([");
        for (index, input) in inputs.iter().enumerate() {
            if index != 0 {
                source.push(',');
            }
            let gate = if input.gate { 1 } else { 0 };
            let _ = write!(
                source,
                "[{},{},{},{},{},{}]",
                input.source.code(),
                input.device,
                input.control,
                input.value,
                gate,
                input.frame
            );
        }
        source.push_str("])");
        self.eval(&source, "performance input batch").map(|_| ())
    }

    pub fn cps(&mut self) -> Result<(u32, u32), String> {
        let text = self.eval(
            "[globalThis.__TRUEOS_STRUDEL.status().cpsNumerator,globalThis.__TRUEOS_STRUDEL.status().cpsDenominator]",
            "runtime CPS status",
        )?;
        parse_cps(&text)
    }

    pub fn poll(&mut self) -> String {
        self.workbench.poll()
    }

    fn eval_unit(&mut self, source: &str, label: &str) -> Result<(), String> {
        let result = self.eval(source, label).map(|_| ());
        let _ = self.workbench.poll();
        result
    }

    fn eval(&mut self, source: &str, label: &str) -> Result<String, String> {
        let result = self
            .workbench
            .eval(source, EvalMode::Script)
            .map_err(|error| format!("{label}: {error}"))?;
        if !result.ok {
            return Err(format!("{label}: {}", result.text));
        }
        Ok(result.text)
    }
}

fn parse_cps(text: &str) -> Result<(u32, u32), String> {
    let trimmed = text
        .trim()
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or_else(|| text.trim())
        .trim_matches('[')
        .trim_matches(']');
    let mut parts = trimmed.split(',');
    let numerator = parts.next().and_then(|v| v.trim().parse().ok());
    let denominator = parts.next().and_then(|v| v.trim().parse().ok());
    match (numerator, denominator) {
        (Some(n), Some(d)) if n > 0 && d > 0 => Ok((n, d)),
        _ => Err(format!("invalid runtime CPS status: {text}")),
    }
}

impl Default for StrudelVm {
    fn default() -> Self {
        Self::new()
    }
}

fn js_string_literal(source: &str) -> String {
    let mut out = String::with_capacity(source.len().saturating_add(2));
    out.push('"');
    for ch in source.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            control if control <= '\u{001f}' => {
                let _ = write!(out, "\\u{:04x}", control as u32);
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::{js_string_literal, parse_cps};

    #[test]
    fn quotes_live_editor_source_for_javascript() {
        assert_eq!(
            js_string_literal("sequence(\"c4\", `g4`)\n// x\\y"),
            "\"sequence(\\\"c4\\\", `g4`)\\n// x\\\\y\""
        );
        assert_eq!(js_string_literal("\u{2028}\u{2029}"), "\"\\u2028\\u2029\"");
    }

    #[test]
    fn parses_runtime_cps_status() {
        assert_eq!(parse_cps("[3,4]"), Ok((3, 4)));
        assert_eq!(parse_cps("\"[1,2]\""), Ok((1, 2)));
        assert!(parse_cps("[0,2]").is_err());
    }
}
