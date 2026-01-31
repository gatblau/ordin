//! The log events and the exported span (Observability). A subscriber local to the test thread keeps
//! every event and span; the Logging rules R-2 and R-3 are checked on the field names, and the
//! secret, the MAC and the arguments are checked to appear nowhere.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ordin_types::wire::hook::HarnessKind;
use ordin_types::ApprovalId;
use tracing::field::{Field, Visit};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

use super::*;

#[derive(Debug, Clone, Default)]
struct Kept {
    name: String,
    target: String,
    fields: HashMap<String, String>,
}

#[derive(Default)]
struct Fields(HashMap<String, String>);

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
}

#[derive(Clone, Default)]
struct Keep {
    events: Arc<Mutex<Vec<Kept>>>,
    spans: Arc<Mutex<HashMap<u64, Kept>>>,
}

impl<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>> Layer<S>
    for Keep
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        event.record(&mut fields);
        let name = fields.0.remove("message").unwrap_or_default();
        self.events.lock().unwrap().push(Kept {
            name,
            target: event.metadata().target().to_owned(),
            fields: fields.0,
        });
    }

    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _ctx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        self.spans.lock().unwrap().insert(
            id.into_u64(),
            Kept {
                name: attrs.metadata().name().to_owned(),
                target: attrs.metadata().target().to_owned(),
                fields: fields.0,
            },
        );
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        _ctx: Context<'_, S>,
    ) {
        let mut fields = Fields::default();
        values.record(&mut fields);
        if let Some(span) = self.spans.lock().unwrap().get_mut(&id.into_u64()) {
            span.fields.extend(fields.0);
        }
    }
}

fn is_event_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 64
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
}

fn is_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 40
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Logging R-2: a field written under one of these names is dropped by the formatter.
fn would_be_dropped(name: &str) -> bool {
    use ordin_types::log::{FORBIDDEN_FIELDS, RESERVED_KEYS};
    let bound = ["run_id", "crew_run_id", "thread_id"];
    !is_field_name(name)
        || (RESERVED_KEYS.contains(&name) && !bound.contains(&name))
        || FORBIDDEN_FIELDS
            .iter()
            .any(|word| name == *word || name.ends_with(&format!("_{word}")))
}

#[tokio::test]
async fn the_events_the_spec_names_are_logged_without_secrets_and_without_dropped_fields() {
    let keep = Keep::default();
    let subscriber = tracing_subscriber::registry().with(keep.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let approval = ApprovalId::new();
    // A decided call, a refusal, a held call that passes its deadline, an after-tool event nobody
    // matches, a stop, an in-process call and a closed session.
    let mut request = signed(&opened, HookCallId::new(), pre("Read", Some("toolu_log")));
    let good = rig
        .send(&{
            let mut b = serde_json::to_vec(&request).unwrap();
            b.push(b'\n');
            b
        })
        .await;
    assert!(matches!(good[0], HookReply::Allow { .. }));
    request.call_id = HookCallId::new();
    request.mac = mac_of(&[1u8; 32], &request.message);
    let mut bytes = serde_json::to_vec(&request).unwrap();
    bytes.push(b'\n');
    let _ = rig.send(&bytes).await;
    rig.rules.set_decide(Decide::Ask(Some(approval)));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let _ = rig.call(&opened, pre("Bash", None)).await;
    let _ = rig
        .call(&opened, post("Read", Some("unmatched"), true))
        .await;
    let _ = rig.call(&opened, stop()).await;
    let _ = rig
        .relay
        .decide_in_process(opened.run_id, pre("Read", None))
        .await;
    rig.supervisor.live.lock().unwrap().clear();
    let _ = rig
        .relay
        .decide_in_process(opened.run_id, pre("Read", None))
        .await;
    rig.relay.close_session(opened.run_id);

    let events = keep.events.lock().unwrap().clone();
    let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
    for expected in [
        "hook_session_opened",
        "hook_decided",
        "hook_refused",
        "hook_holding",
        "hook_post_unmatched",
        "hook_stop",
        "hook_session_closed",
    ] {
        assert!(
            names.contains(&expected),
            "{expected} was not logged; saw {names:?}"
        );
    }
    let ours: Vec<&Kept> = events
        .iter()
        .filter(|e| e.target == "ordin_hook_relay")
        .collect();
    assert!(!ours.is_empty());
    for event in &ours {
        assert!(is_event_name(&event.name), "{}", event.name);
        for field in event.fields.keys() {
            assert!(
                !would_be_dropped(field),
                "{} writes the field {field}",
                event.name
            );
        }
    }
    let decided: Vec<&&Kept> = ours.iter().filter(|e| e.name == "hook_decided").collect();
    assert!(decided.iter().any(|e| e.fields["route"] == "socket"));
    assert!(decided.iter().any(|e| e.fields["route"] == "in_process"));
    assert!(decided.iter().all(|e| e.fields.contains_key("elapsed_ms")
        && e.fields.contains_key("verdict")
        && e.fields.contains_key("call_id")));
    // Nothing logged holds the secret, a MAC or the arguments.
    let everything = format!("{events:?}");
    assert!(!everything.contains(&opened.env.secret));
    assert!(!everything.contains(&request.mac));
    assert!(!everything.contains("a.txt"));
}

#[tokio::test]
async fn a_built_in_tool_call_opens_the_exported_span_with_its_four_fields_only() {
    let keep = Keep::default();
    let subscriber = tracing_subscriber::registry().with(keep.clone());
    let _guard = tracing::subscriber::set_default(subscriber);

    let rig = Rig::new().await;
    let opened = rig.open(HarnessKind::ClaudeCode);
    let _ = rig.call(&opened, pre("Read", None)).await;
    rig.rules.set_decide(Decide::Deny("no".into()));
    let _ = rig.call(&opened, pre("Bash", None)).await;
    rig.rules.set_decide(Decide::Ask(Some(ApprovalId::new())));
    *rig.rules.waited.lock().unwrap() = Waited::UntilDeadline;
    let _ = rig.call(&opened, pre("Write", None)).await;
    rig.rules.set_decide(Decide::Slow(Duration::from_secs(1)));
    let _ = rig.call(&opened, pre("Edit", None)).await;
    // A package tool is not the Hook Relay's span.
    rig.rules.set_decide(Decide::Allow);
    let mut package = pre("mcp__x__y", None);
    if let ordin_types::wire::hook::HookMessage::PreToolUse { origin, .. } = &mut package {
        *origin = ordin_types::wire::rules::ToolOrigin::PackageMcpServer { server: "x".into() };
    }
    let _ = rig.call(&opened, package).await;

    let spans: Vec<Kept> = keep
        .spans
        .lock()
        .unwrap()
        .values()
        .filter(|s| s.name == "ordin.tool_call")
        .cloned()
        .collect();
    assert_eq!(
        spans.len(),
        4,
        "one span per built-in call, none for the package tool"
    );
    let outcome_of = |tool: &str| {
        spans
            .iter()
            .find(|s| s.fields.get("tool").map(String::as_str) == Some(tool))
            .unwrap_or_else(|| panic!("no span for {tool}"))
    };
    assert_eq!(outcome_of("Read").fields["outcome"], "allowed");
    assert_eq!(outcome_of("Bash").fields["outcome"], "refused");
    assert_eq!(outcome_of("Write").fields["outcome"], "paused");
    assert_eq!(outcome_of("Edit").fields["outcome"], "failed");
    for span in &spans {
        assert_eq!(span.fields["origin"], "built_in");
        assert!(span.fields.contains_key("control"));
        let mut keys: Vec<&str> = span.fields.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["control", "origin", "outcome", "tool"],
            "the allow list, no more"
        );
    }
    assert_eq!(outcome_of("Read").fields["control"], "none");
}
