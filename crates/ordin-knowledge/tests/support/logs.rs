//! A log capture for one test: every event's message and fields, as text.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

#[derive(Debug, Clone)]
pub struct Line {
    pub event: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<Vec<Line>>>);

impl Captured {
    pub fn named(&self, event: &str) -> Vec<Line> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.event == event)
            .cloned()
            .collect()
    }

    pub fn all(&self) -> Vec<Line> {
        self.0.lock().unwrap().clone()
    }
}

struct Visitor<'a>(&'a mut Line);

impl Visit for Visitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let text = format!("{value:?}");
        if field.name() == "message" {
            self.0.event = text.trim_matches('"').to_owned();
        } else {
            self.0
                .fields
                .insert(field.name().to_owned(), text.trim_matches('"').to_owned());
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.0.event = value.to_owned();
        } else {
            self.0
                .fields
                .insert(field.name().to_owned(), value.to_owned());
        }
    }
}

struct Capture(Captured);

impl<S: Subscriber> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut line = Line {
            event: String::new(),
            fields: BTreeMap::new(),
        };
        event.record(&mut Visitor(&mut line));
        self.0 .0.lock().unwrap().push(line);
    }
}

/// Installs a capture for the current thread (use a current-thread runtime); keep the guard.
pub fn capture() -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let subscriber = tracing_subscriber::registry().with(Capture(captured.clone()));
    let guard = tracing::subscriber::set_default(subscriber);
    (captured, guard)
}
