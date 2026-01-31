//! Logging.md R-2: every event this crate writes keeps all its fields (no `dropped_fields`), has a
//! valid event name and carries `component = "organisation_profile"`; R-7: no line holds the profile
//! address, the document or the signature.

use serde_json::json;

use super::support::*;

#[tokio::test]
async fn every_event_keeps_its_fields_and_names_the_component() {
    let (lines, _guard) = Lines::capture();
    let r = running_with(
        "1.0.0",
        json!({"remote_agents": [{"name": "A", "route": "hosted_agent",
            "address": "https://agent.example.com",
            "identity": {"issuer": "https://idp.example.com", "client_id": "c", "audience": "a"}}]}),
    )
    .await;
    let agent = r.service.remote_agents().await.unwrap().remove(0);
    r.service.mark_remote_agent_enabled(agent.id).await.unwrap();
    refresh_with(&r, "1.1.0", json!({"usage": {"agents_at_once_max": 4}})).await;
    refresh_with(&r, "1.2.0", json!({})).await;
    r.service
        .set_profile_address(&r.site.url(), vec![Key::k1().public()])
        .await
        .unwrap();
    r.service.shutdown().await;

    let all = lines.lines();
    for event in [
        "profile.fetch.done",
        "profile.activated",
        "profile.remote_agents.synced",
        "profile.remote_agent.status",
        "profile.refused",
        "profile.address.set",
    ] {
        assert!(!lines.event(event).is_empty(), "{event} was written");
    }
    let refused = &lines.event("profile.refused")[0];
    assert_eq!(refused["code"], "PROF_CONTENT_INVALID");
    assert_eq!(refused["key_path"], "usage.agents_at_once_max");
    assert_eq!(
        lines.event("profile.activated")[1]["previous_version"],
        "1.0.0"
    );
    for line in &all {
        if line["logger"]
            .as_str()
            .is_some_and(|l| l.starts_with("ordin_organisation_profile"))
        {
            assert!(line.get("dropped_fields").is_none(), "{line}");
            assert_ne!(line["event"], "invalid_event_name", "{line}");
            assert_eq!(line["component"], "organisation_profile", "{line}");
            let text = line.to_string();
            assert!(!text.contains(&r.site.server.uri()), "no address: {text}");
            assert!(!text.contains("EdDSA"), "no signature: {text}");
        }
    }
}
