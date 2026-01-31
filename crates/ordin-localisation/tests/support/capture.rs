//! A process-wide `tracing` subscriber that records every event, for tests that assert on log lines.
//! It is installed globally and once, never scoped to a thread, so the callsite interest cache is
//! right for every thread (plan 01, FU-017).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Mutex, Once, PoisonError};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::{Layer, Registry};

#[derive(Debug, Clone)]
pub struct Line {
    pub level: Level,
    pub target: String,
    /// The event name: the message of the `tracing` call.
    pub event: String,
    pub fields: BTreeMap<String, String>,
}

impl Line {
    pub fn field(&self, name: &str) -> &str {
        self.fields.get(name).map_or("", String::as_str)
    }
}

static LINES: Mutex<Vec<Line>> = Mutex::new(Vec::new());
static ONCE: Once = Once::new();

struct Collect;

#[derive(Default)]
struct Fields(BTreeMap<String, String>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

impl<S: Subscriber> Layer<S> for Collect {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let message = fields.0.remove("message").unwrap_or_default();
        LINES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Line {
                level: *event.metadata().level(),
                target: event.metadata().target().to_string(),
                event: message,
                fields: fields.0,
            });
    }
}

/// Installs the recorder (the first call only).
pub fn install() {
    ONCE.call_once(|| {
        let subscriber = Registry::default().with(Collect);
        // Another global subscriber would make every assertion below fail loudly, not pass silently.
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

pub fn clear() {
    LINES.lock().unwrap_or_else(PoisonError::into_inner).clear();
}

pub fn lines() -> Vec<Line> {
    LINES.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The recorded lines of this crate (`component = "localisation"`).
pub fn ours() -> Vec<Line> {
    lines()
        .into_iter()
        .filter(|line| line.field("component") == "localisation")
        .collect()
}

pub fn event(name: &str) -> Vec<Line> {
    ours()
        .into_iter()
        .filter(|line| line.event == name)
        .collect()
}
