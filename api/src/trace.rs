//! Thin `tracing` subscriber backed by the TRUEOS structured-log ABI.
//!
//! Blueprint applications retain their static callsites and fields. Formatting,
//! timestamping, and log transport stay on the kernel side of `log_record`.

use alloc::string::String;
use core::fmt::{self, Write as _};
use core::num::NonZeroU64;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tracing::field::{Field, Visit};
use tracing::metadata::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

use crate::logl;

static NEXT_SPAN_ID: AtomicU64 = AtomicU64::new(1);
static EMITTED_EVENTS: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Copy, Debug, Default)]
pub struct KernelSubscriber;

impl Subscriber for KernelSubscriber {
    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= &Level::INFO
    }

    #[inline]
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::INFO)
    }

    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        let raw = NEXT_SPAN_ID.fetch_add(1, Ordering::Relaxed);
        let nonzero = NonZeroU64::new(raw).unwrap_or(NonZeroU64::MIN);
        let metadata = attributes.metadata();
        let mut visitor = FieldVisitor::default();
        let _ = write!(
            visitor.line,
            "span.new id={} name={}",
            nonzero,
            metadata.name()
        );
        attributes.record(&mut visitor);
        let _ = logl::log_record(
            level(metadata.level()),
            metadata.target(),
            visitor.line.as_str(),
        );
        Id::from_non_zero_u64(nonzero)
    }

    #[inline]
    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    #[inline]
    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        if visitor.line.is_empty() {
            visitor.line.push_str(metadata.name());
        }
        let _ = logl::log_record(
            level(metadata.level()),
            metadata.target(),
            visitor.line.as_str(),
        );
        EMITTED_EVENTS.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn enter(&self, _span: &Id) {}

    #[inline]
    fn exit(&self, _span: &Id) {}
}

#[derive(Default)]
struct FieldVisitor {
    line: String,
}

impl FieldVisitor {
    fn separator(&mut self, field: &Field) {
        if !self.line.is_empty() {
            self.line.push(' ');
        }
        if field.name() != "message" {
            self.line.push_str(field.name());
            self.line.push('=');
        }
    }
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.separator(field);
        self.line.push_str(value);
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.separator(field);
        let _ = write!(self.line, "{value:?}");
    }
}

#[inline]
fn level(level: &Level) -> u8 {
    match *level {
        Level::ERROR => logl::level::ERROR,
        Level::WARN => logl::level::WARN,
        Level::INFO => logl::level::INFO,
        Level::DEBUG => logl::level::DEBUG,
        Level::TRACE => logl::level::TRACE,
    }
}

/// Runs `f` with TRUEOS's kernel-backed tracing subscriber as the local default.
#[inline]
pub fn with_default<T>(f: impl FnOnce() -> T) -> T {
    tracing::subscriber::with_default(KernelSubscriber, f)
}

/// Number of tracing events forwarded by this Blueprint instance.
#[inline]
pub fn emitted_events() -> usize {
    EMITTED_EVENTS.load(Ordering::Relaxed)
}
