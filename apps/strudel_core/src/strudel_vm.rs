extern crate alloc;

use alloc::{format, string::String, vec::Vec};

use trueos_qjs::workbench::{EvalMode, Workbench};

use crate::{event::RenderEvent, json_rows::parse_event_rows};

const UPSTREAM_BUNDLE: &str = include_str!("../js/vendor/strudel-core.bundle.js");
const FALLBACK_CORE: &str = include_str!("../js/00_fallback_core.js");
const TRUEOS_ADAPTER: &str = include_str!("../js/10_trueos_adapter.js");
const DEMO_PATTERN: &str = include_str!("../js/20_demo_pattern.js");

pub struct StrudelVm {
    workbench: Workbench,
}

impl StrudelVm {
    pub fn new() -> Self {
        Self {
            workbench: Workbench::new(),
        }
    }

    pub fn install(&mut self) -> Result<(), String> {
        self.eval_unit(UPSTREAM_BUNDLE, "upstream bundle")?;
        self.eval_unit(FALLBACK_CORE, "fallback temporal kernel")?;
        self.eval_unit(TRUEOS_ADAPTER, "TRUEOS pattern adapter")?;
        self.eval_unit(DEMO_PATTERN, "demo pattern")?;

        let smoke = self.eval(
            "globalThis.__TRUEOS_STRUDEL.selfTest()",
            "Strudel temporal smoke test",
        )?;
        if smoke != "\"a@0.000000-0.500000|b@0.500000-0.750000|c@0.750000-1.000000\"" {
            return Err(format!("unexpected temporal smoke result: {smoke}"));
        }
        Ok(())
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
        let _ = self.eval(source, label)?;
        let _ = self.workbench.poll();
        Ok(())
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
