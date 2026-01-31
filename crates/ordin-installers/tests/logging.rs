//! The events of Installers.md *Observability* (SA-OBS-01).
//!
//! The only test in its own binary: `tracing` caches per call site whether anything listens, so a
//! test thread hitting a call site with no subscriber could leave a scoped subscriber unasked. Alone
//! in the process, no other thread logs (plan FU-017). The runtime is the current-thread one, so the
//! scoped subscriber sees every event the installers write.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use ordin_installers::*;
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const SESSION_TEXT: &str = "the session's private text";

#[tokio::test]
async fn every_event_carries_identifiers_counts_and_relative_paths_only() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    let guard = tracing::subscriber::set_default(subscriber);

    let world = World::new().await;
    let plain = world.bundle(CSPA, HarnessKind::ClaudeCode, &[]).await;
    let with_pack = world
        .bundle(CSPA, HarnessKind::ClaudeCode, &[TEST_PYRAMID])
        .await;

    // installers.installed, then installers.verified.
    let agent = world.agent().await;
    let id = agent.agent_instance_id;
    let folder = agent.folder_path.clone();
    world
        .installers
        .install(&world.request(&agent, HarnessKind::ClaudeCode, &with_pack))
        .await
        .expect("installs");
    world.installers.verify(id).await.expect("verifies");

    // installers.updated with a backup and a kept changed file: apply.md changed (kept in both
    // bundles) and the pack's rule changed (dropped by the plain bundle).
    std::fs::write(folder.join(".claude/commands/apply.md"), SESSION_TEXT).unwrap();
    std::fs::write(
        folder.join(".claude/rules/testing/pyramid.md"),
        SESSION_TEXT,
    )
    .unwrap();
    let updated = world
        .installers
        .update(&world.request(&agent, HarnessKind::ClaudeCode, &plain))
        .await
        .expect("updates");

    // installers.mismatch, then installers.repaired.
    std::fs::write(folder.join(".claude/rules/tone.md"), SESSION_TEXT).unwrap();
    world
        .installers
        .verify_and_repair(id, &scope())
        .await
        .expect("repairs");

    // installers.backup_mismatch: the backup of apply.md altered before uninstall.
    let backup = updated
        .record
        .files
        .iter()
        .find(|f| f.path == ".claude/commands/apply.md")
        .and_then(|f| f.backup.clone())
        .expect("a backup of apply.md");
    std::fs::write(agent.meta_path.join(&backup), "altered backup").unwrap();
    world.installers.uninstall(id).await.expect("uninstalls");

    // installers.import_missing, from a pure render (no instance).
    let dir = TempDir::new();
    let input = ClaudeSessionFormInput {
        agent_name: "support-agent".to_owned(),
        version: "2026.10.1".to_owned(),
        description: String::new(),
        instructions: "@rules/absent.md\n".to_owned(),
        skills: BTreeMap::new(),
        commands: BTreeMap::new(),
    };
    render_claude_session_form(&input, dir.path()).expect("renders");

    // installers.repair_failed: the bundle gone and the parts no longer in the clone.
    let unbound =
        World::bound_to(Arc::new(empty_source()), Some(Arc::new(catalogue_source()))).await;
    let (lost, lost_bundle, _) = unbound.installed_cspa().await;
    std::fs::write(lost.folder_path.join("CLAUDE.md"), SESSION_TEXT).unwrap();
    std::fs::remove_dir_all(&lost_bundle.dir).unwrap();
    unbound
        .installers
        .verify_and_repair(lost.agent_instance_id, &scope())
        .await
        .expect_err("cannot repair");

    drop(guard);
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<serde_json::Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{l}: {e}")))
        .collect();
    let ours: Vec<&serde_json::Value> = lines
        .iter()
        .filter(|l| {
            l["event"]
                .as_str()
                .is_some_and(|e| e.starts_with("installers."))
        })
        .collect();
    for line in &ours {
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }
    let events = |name: &str| -> Vec<&serde_json::Value> {
        ours.iter()
            .copied()
            .filter(|l| l["event"] == name)
            .collect()
    };
    let id_text = id.to_string();

    let installed = events("installers.installed");
    assert_eq!(installed.len(), 2, "{text}");
    let first = installed[0];
    assert_eq!(first["level"], "INFO");
    assert_eq!(first["agent_instance_id"], id_text.as_str());
    assert_eq!(first["installer"], "claude-code");
    assert_eq!(first["plan_hash"], with_pack.plan.plan_hash.as_str());
    assert!(first["created"].as_u64().unwrap() > 0);
    for field in ["replaced", "unchanged", "removed", "backed_up"] {
        assert_eq!(first[field], 0, "{field}");
    }
    assert!(first["duration_ms"].is_number());

    let updates = events("installers.updated");
    assert_eq!(updates.len(), 1, "{text}");
    let update = updates[0];
    assert_eq!(update["level"], "INFO");
    assert_eq!(update["plan_hash"], plain.plan.plan_hash.as_str());
    assert_eq!(update["backed_up"], 2);
    assert!(update["replaced"].as_u64().unwrap() >= 1);
    assert!(update["unchanged"].as_u64().unwrap() >= 1);

    let kept = events("installers.kept_changed_file");
    assert_eq!(kept.len(), 1, "{text}");
    assert_eq!(kept[0]["level"], "WARNING");
    assert_eq!(kept[0]["agent_instance_id"], id_text.as_str());
    assert_eq!(kept[0]["path"], ".claude/rules/testing/pyramid.md");

    let verified = events("installers.verified");
    assert_eq!(verified.len(), 1, "{text}");
    assert_eq!(verified[0]["level"], "DEBUG");
    assert_eq!(verified[0]["changed"], 0);
    assert_eq!(verified[0]["missing"], 0);

    let mismatches = events("installers.mismatch");
    assert_eq!(mismatches.len(), 2, "{text}");
    assert_eq!(mismatches[0]["level"], "WARNING");
    assert_eq!(mismatches[0]["agent_instance_id"], id_text.as_str());
    assert_eq!(mismatches[0]["changed"], 1);
    assert_eq!(mismatches[0]["missing"], 0);

    let repaired = events("installers.repaired");
    assert_eq!(repaired.len(), 1, "{text}");
    assert_eq!(repaired[0]["level"], "WARNING");
    assert_eq!(repaired[0]["agent_instance_id"], id_text.as_str());
    assert_eq!(repaired[0]["changed"], 1);

    let backup_mismatch = events("installers.backup_mismatch");
    assert_eq!(backup_mismatch.len(), 1, "{text}");
    assert_eq!(backup_mismatch[0]["level"], "WARNING");
    assert_eq!(backup_mismatch[0]["path"], ".claude/commands/apply.md");

    let missing = events("installers.import_missing");
    assert_eq!(missing.len(), 1, "{text}");
    assert_eq!(missing[0]["level"], "WARNING");
    assert_eq!(missing[0]["path"], "rules/absent.md");

    let failed = events("installers.repair_failed");
    assert_eq!(failed.len(), 1, "{text}");
    assert_eq!(failed[0]["level"], "ERROR");
    assert_eq!(
        failed[0]["agent_instance_id"],
        lost.agent_instance_id.to_string().as_str()
    );
    assert_eq!(failed[0]["error_code"], "INST_REPAIR_FAILED");

    // Never file content, and never an absolute path.
    let ours_text: String = ours.iter().map(|l| format!("{l}\n")).collect();
    for forbidden in [
        SESSION_TEXT,
        "altered backup",
        "Use short sentences",
        "Builds components",
        world.dir.path().to_str().unwrap(),
        unbound.dir.path().to_str().unwrap(),
    ] {
        assert!(
            !ours_text.contains(forbidden),
            "the log holds {forbidden}: {ours_text}"
        );
    }
}
