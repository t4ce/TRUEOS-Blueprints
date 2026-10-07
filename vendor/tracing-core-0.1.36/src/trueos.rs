//! Resident tracing transport. The host owns filtering and log storage.
//! Events use the existing structured log C ABI, with bounded single-line
//! formatting. Spans retain their IDs, fields, parentage, and carrier context.

use crate::{
    field::{Field, Visit},
    lazy::Lazy,
    span::{Attributes, Current, Id, Record},
    subscriber::Interest,
    sync::Mutex,
    Event, Level, LevelFilter, Metadata, Subscriber,
};
use alloc::{collections::BTreeMap, vec::Vec};
use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicU64, AtomicUsize, Ordering},
};

const QUERY: u32 = 1 << 31;
unsafe extern "C" {
    fn trueos_cabi_log(
        level: u32,
        target: *const u8,
        target_len: usize,
        message: *const u8,
        message_len: usize,
    ) -> i32;
    fn trueos_cabi_thread_current_id() -> usize;
}

/// The resident collector used by every TRUEOS Dispatch.
#[derive(Clone, Copy, Debug, Default)]
pub struct Resident;

#[derive(Clone)]
struct Text<const N: usize> {
    bytes: [u8; N],
    len: usize,
    truncated: bool,
}
impl<const N: usize> Text<N> {
    fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
            truncated: false,
        }
    }
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.len]).unwrap()
    }
    fn finish(&mut self) {
        if self.truncated {
            let mut end = self.len.min(N - 3);
            while !self.as_str().is_char_boundary(end) {
                end -= 1;
            }
            self.len = end;
            self.bytes[self.len..self.len + 3].copy_from_slice(b"...");
            self.len += 3;
            self.truncated = false;
        }
    }
}
impl<const N: usize> Write for Text<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for ch in s.chars() {
            let ch = if ch.is_control() { ' ' } else { ch };
            let mut encoded = [0; 4];
            let bytes = ch.encode_utf8(&mut encoded).as_bytes();
            if self.len + bytes.len() > N - 3 {
                self.truncated = true;
                return Err(fmt::Error);
            }
            self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
        Ok(())
    }
}
struct Fields<'a, const N: usize>(&'a mut Text<N>);
impl<const N: usize> Visit for Fields<'_, N> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if self.0.len != 0 {
            let _ = self.0.write_char(' ');
        }
        if field.name() != "message" {
            let _ = write!(self.0, "{}=", field.name());
        }
        let _ = write!(self.0, "{value:?}");
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            if self.0.len != 0 {
                let _ = self.0.write_char(' ');
            }
            let _ = self.0.write_str(value);
        } else {
            self.record_debug(field, &value);
        }
    }
}

fn code(level: &Level) -> u32 {
    match *level {
        Level::ERROR => 1,
        Level::WARN => 2,
        Level::INFO => 3,
        Level::DEBUG => 4,
        _ => 5,
    }
}
fn target(value: &str) -> &str {
    let mut end = value.len().min(256);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        "tracing"
    } else {
        &value[..end]
    }
}
fn enabled(level: u32, name: &str) -> bool {
    let name = target(name);
    // Older kernels reject the query flag. Their normal record path still
    // performs authoritative filtering, so do not silently discard events.
    unsafe {
        trueos_cabi_log(
            level | QUERY,
            name.as_ptr(),
            name.len(),
            core::ptr::null(),
            0,
        ) != 0
    }
}
fn send<const N: usize>(level: u32, name: &str, mut text: Text<N>) {
    text.finish();
    let name = target(name);
    unsafe {
        let _ = trueos_cabi_log(
            level,
            name.as_ptr(),
            name.len(),
            text.bytes.as_ptr(),
            text.len,
        );
    }
}

struct SpanData {
    meta: &'static Metadata<'static>,
    fields: Text<256>,
    parent: Option<u64>,
    refs: usize,
}
#[derive(Default)]
struct Carrier {
    stack: Vec<u64>,
    formatting: bool,
}
#[derive(Default)]
struct State {
    spans: BTreeMap<u64, SpanData>,
    carriers: BTreeMap<usize, Carrier>,
}
static STATE: Lazy<Mutex<State>> = Lazy::new(Mutex::default);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static EMITTED_EVENTS: AtomicUsize = AtomicUsize::new(0);
/// Number of event records sent through this Blueprint instance's host boundary.
pub fn emitted_events() -> usize {
    EMITTED_EVENTS.load(Ordering::Relaxed)
}
fn carrier() -> usize {
    unsafe { trueos_cabi_thread_current_id() }
}
fn current(state: &State, carrier: usize) -> Option<u64> {
    state
        .carriers
        .get(&carrier)
        .and_then(|c| c.stack.last().copied())
}
// Never hold the state lock while formatting user Debug implementations or
// invoking the host. Nested formatting events are suppressed on that carrier.
struct Formatting(usize);
impl Formatting {
    fn enter() -> Option<Self> {
        let id = carrier();
        let mut state = STATE.lock().unwrap();
        let context = state.carriers.entry(id).or_default();
        if context.formatting {
            return None;
        }
        context.formatting = true;
        Some(Self(id))
    }
}
impl Drop for Formatting {
    fn drop(&mut self) {
        let mut state = STATE.lock().unwrap();
        if let Some(context) = state.carriers.get_mut(&self.0) {
            context.formatting = false;
            if context.stack.is_empty() {
                state.carriers.remove(&self.0);
            }
        }
    }
}
fn lifecycle(action: &str, id: u64, meta: &'static Metadata<'static>, fields: &str) {
    if !enabled(5, meta.target()) {
        return;
    }
    let Some(_formatting) = Formatting::enter() else {
        return;
    };
    let mut text = Text::<1024>::new();
    let _ = write!(text, "span.{action} id={id} name={} {fields}", meta.name());
    send(5, meta.target(), text);
}
fn close(id: u64) -> bool {
    let mut next = Some(id);
    let mut closed = false;
    while let Some(current) = next.take() {
        let removed = {
            let mut state = STATE.lock().unwrap();
            let Some(span) = state.spans.get_mut(&current) else {
                break;
            };
            span.refs -= 1;
            if span.refs != 0 {
                break;
            }
            state.spans.remove(&current)
        };
        if let Some(span) = removed {
            closed |= current == id;
            next = span.parent;
            lifecycle("close", current, span.meta, "");
        }
    }
    closed
}

impl Subscriber for Resident {
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }
    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(LevelFilter::TRACE)
    }
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        enabled(code(meta.level()), meta.target())
    }
    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let mut fields = Text::<256>::new();
        if let Some(_formatting) = Formatting::enter() {
            attrs.record(&mut Fields(&mut fields));
        }
        fields.finish();
        let parent = {
            let mut state = STATE.lock().unwrap();
            let parent = attrs.parent().map(Id::into_u64).or_else(|| {
                if attrs.is_contextual() {
                    current(&state, carrier())
                } else {
                    None
                }
            });
            if let Some(parent) = parent.and_then(|p| state.spans.get_mut(&p)) {
                parent.refs += 1;
            }
            state.spans.insert(
                id,
                SpanData {
                    meta: attrs.metadata(),
                    fields: fields.clone(),
                    parent,
                    refs: 1,
                },
            );
            parent
        };
        let mut description = Text::<512>::new();
        let _ = write!(
            description,
            "parent={} {}",
            parent.unwrap_or(0),
            fields.as_str()
        );
        lifecycle("create", id, attrs.metadata(), description.as_str());
        Id::from_u64(id)
    }
    fn record(&self, id: &Id, values: &Record<'_>) {
        let Some(_formatting) = Formatting::enter() else {
            return;
        };
        let mut fields = Text::<256>::new();
        values.record(&mut Fields(&mut fields));
        fields.finish();
        let meta = {
            let mut state = STATE.lock().unwrap();
            let Some(span) = state.spans.get_mut(&id.into_u64()) else {
                return;
            };
            let _ = write!(span.fields, " {}", fields.as_str());
            span.fields.finish();
            span.meta
        };
        drop(_formatting);
        lifecycle("record", id.into_u64(), meta, fields.as_str());
    }
    fn record_follows_from(&self, id: &Id, follows: &Id) {
        let meta = STATE
            .lock()
            .unwrap()
            .spans
            .get(&id.into_u64())
            .map(|s| s.meta);
        if let Some(meta) = meta {
            let mut fields = Text::<64>::new();
            let _ = write!(fields, "follows={}", follows.into_u64());
            lifecycle("follows", id.into_u64(), meta, fields.as_str());
        }
    }
    fn event(&self, event: &Event<'_>) {
        let Some(_formatting) = Formatting::enter() else {
            return;
        };
        let mut text = Text::<1024>::new();
        {
            let state = STATE.lock().unwrap();
            let mut parent = event.parent().map(Id::into_u64).or_else(|| {
                if event.is_contextual() {
                    current(&state, _formatting.0)
                } else {
                    None
                }
            });
            // Bound context traversal and output even for deeply nested spans.
            for _ in 0..8 {
                let Some(id) = parent else {
                    break;
                };
                let Some(span) = state.spans.get(&id) else {
                    break;
                };
                let _ = write!(
                    text,
                    "[span={id} {} {}] ",
                    span.meta.name(),
                    span.fields.as_str()
                );
                parent = span.parent;
            }
        }
        event.record(&mut Fields(&mut text));
        if text.len == 0 {
            let _ = text.write_str(event.metadata().name());
        }
        send(
            code(event.metadata().level()),
            event.metadata().target(),
            text,
        );
        EMITTED_EVENTS.fetch_add(1, Ordering::Relaxed);
    }
    fn enter(&self, id: &Id) {
        let meta = {
            let mut state = STATE.lock().unwrap();
            let Some(span) = state.spans.get_mut(&id.into_u64()) else {
                return;
            };
            span.refs += 1;
            let meta = span.meta;
            state
                .carriers
                .entry(carrier())
                .or_default()
                .stack
                .push(id.into_u64());
            meta
        };
        lifecycle("enter", id.into_u64(), meta, "");
    }
    fn exit(&self, id: &Id) {
        let (meta, entered) = {
            let mut state = STATE.lock().unwrap();
            let meta = state.spans.get(&id.into_u64()).map(|s| s.meta);
            let carrier = carrier();
            let mut entered = false;
            if let Some(context) = state.carriers.get_mut(&carrier) {
                if let Some(pos) = context.stack.iter().rposition(|s| *s == id.into_u64()) {
                    context.stack.remove(pos);
                    entered = true;
                }
                if context.stack.is_empty() && !context.formatting {
                    state.carriers.remove(&carrier);
                }
            }
            (meta, entered)
        };
        if let Some(meta) = meta {
            lifecycle("exit", id.into_u64(), meta, "");
        }
        if entered {
            close(id.into_u64());
        }
    }
    fn clone_span(&self, id: &Id) -> Id {
        if let Some(span) = STATE.lock().unwrap().spans.get_mut(&id.into_u64()) {
            span.refs += 1;
        }
        id.clone()
    }
    fn try_close(&self, id: Id) -> bool {
        close(id.into_u64())
    }
    fn current_span(&self) -> Current {
        let state = STATE.lock().unwrap();
        current(&state, carrier())
            .and_then(|id| {
                state
                    .spans
                    .get(&id)
                    .map(|s| Current::new(Id::from_u64(id), s.meta))
            })
            .unwrap_or_else(Current::none)
    }
}

/// Transport for public appender Write APIs, which carry no level metadata.
/// Event APIs use their actual tracing level instead.
pub fn write_bytes(bytes: &[u8]) {
    if !enabled(3, "tracing-appender") {
        return;
    }
    let Some(_formatting) = Formatting::enter() else {
        return;
    };
    let mut text = Text::<1024>::new();
    // Convert only a bounded UTF-8 prefix; no allocation for a giant log line.
    let mut end = bytes.len().min(1024);
    while end > 0 {
        match core::str::from_utf8(&bytes[..end]) {
            Ok(value) => {
                let _ = text.write_str(value);
                break;
            }
            Err(error) => end = error.valid_up_to(),
        }
    }
    text.truncated |= end < bytes.len();
    send(3, "tracing-appender", text);
}
