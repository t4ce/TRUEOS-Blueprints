extern crate alloc;

use alloc::{format, string::String, vec::Vec};
use core::fmt::Write as _;

use trueos_qjs::workbench::{EvalMode, Workbench};

use crate::{event::RenderEvent, json_rows::parse_event_rows};

const UPSTREAM_BUNDLE: &str = include_str!("../js/vendor/strudel-core.bundle.js");
const FALLBACK_CORE: &str = include_str!("../js/00_fallback_core.js");
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

    pub fn query_frames(
        &mut self,
        absolute_start_frame: u64,
        block_frames: u32,
        sample_rate_hz: u32,
        cps_numerator: u32,
        cps_denominator: u32,
    ) -> Result<Vec<RenderEvent>, String> {
        let source = format!(
            "globalThis.__TRUEOS_STRUDEL.queryFrames({absolute_start_frame},{block_frames},{sample_rate_hz},{cps_numerator},{cps_denominator})"
        );
        let text = self.eval(&source, "pattern query")?;
        parse_event_rows(&text).map_err(|error| format!("event-row parse failed: {error}; text={text}"))
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
    use super::js_string_literal;

    #[test]
    fn quotes_live_editor_source_for_javascript() {
        assert_eq!(
            js_string_literal("sequence(\"c4\", `g4`)\n// x\\y"),
            "\"sequence(\\\"c4\\\", `g4`)\\n// x\\\\y\""
        );
        assert_eq!(js_string_literal("\u{2028}\u{2029}"), "\"\\u2028\\u2029\"");
    }
}
