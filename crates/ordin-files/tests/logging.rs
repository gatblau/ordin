//! Observability (FileService.md; Logging.md R-2): the events of the service, their fields, and
//! what they never hold (a path of the person's folders, file content). The only test in its own
//! binary: `tracing` caches per call site whether anything listens, so one process-wide subscriber
//! is installed before anything runs (plan FU-017).

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ordin_files::{
    FileVersionOrigin, FolderAccess, FolderGrant, Grantee, PlaceKind, Timestamp, WriteVersion,
};
use ordin_types::{AgentInstanceId, FolderGrantId, WorkspaceId};
use support::stack::stack;
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

fn lines(buffer: &Buffer) -> Vec<serde_json::Value> {
    let text = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).expect("a JSON line"))
        .collect()
}

fn named<'a>(all: &'a [serde_json::Value], event: &str) -> Vec<&'a serde_json::Value> {
    all.iter().filter(|line| line["event"] == event).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn each_event_is_logged_with_its_fields_and_never_with_a_path_or_content() {
    let buffer = Buffer::default();
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .event_format(ordin_types::log::JsonLineFormat)
            .with_writer(move || writer.clone()),
    );
    tracing::subscriber::set_global_default(subscriber).expect("install the subscriber");

    let s = stack().await;
    let place = s.place("Supplier quotes", PlaceKind::PersonsFolder).await;
    let root = std::path::PathBuf::from(&place.path);
    std::fs::write(root.join("quote.md"), "the secret quote text").unwrap();
    s.service.watch(&[place.id]).await.unwrap();
    for _ in 0..200 {
        let c = s.service.coverage(&[place.id]).await.unwrap();
        if c.total == 1 && c.indexed == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let version = s
        .service
        .write_version(WriteVersion {
            place_id: place.id,
            path: "quote.md".to_owned(),
            bytes: b"the secret second text".to_vec(),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
        })
        .await
        .unwrap();
    let workspace = WorkspaceId::new();
    s.service
        .publish_grants(
            workspace,
            vec![FolderGrant {
                id: FolderGrantId::new(),
                workspace_id: workspace,
                grantee: Grantee::AgentInstance {
                    agent_instance_id: AgentInstanceId::new(),
                },
                place_id: place.id,
                access: FolderAccess::Read,
                granted_at: Timestamp::now(),
            }],
        )
        .await
        .unwrap();
    let future = Timestamp::from_unix_millis(Timestamp::now().unix_millis() + 60_000).unwrap();
    s.service.prune_versions(future).await.unwrap();
    // A watched place whose folder is gone is logged at the next start.
    let gone = s.place("Gone", PlaceKind::PersonsFolder).await;
    let gone_root = std::path::PathBuf::from(&gone.path);
    sqlx::query("UPDATE places SET watched = 1 WHERE id = ?1")
        .bind(gone.id.to_string())
        .execute(s.store.sqlite().unwrap())
        .await
        .unwrap();
    std::fs::remove_dir_all(&gone_root).unwrap();
    s.service.shutdown().await;
    s.service.start().await.unwrap();
    s.service.shutdown().await;
    s.service.remove_place(place.id).await.unwrap();

    let all = lines(&buffer);
    let ours: Vec<&serde_json::Value> = all.iter().filter(|l| l["component"] == "files").collect();
    assert!(!ours.is_empty());
    // No field was dropped by the formatter (Logging.md R-2).
    for line in &all {
        assert!(line.get("dropped_fields").is_none(), "{line}");
    }
    let added = named(&all, "place_added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[0]["place_id"], place.id.to_string());
    assert_eq!(added[0]["kind"], "persons_folder");
    assert_eq!(
        named(&all, "index_build_started")[0]["place_id"],
        place.id.to_string()
    );
    let finished = &named(&all, "index_build_finished")[0];
    assert_eq!(
        (
            finished["indexed"].as_u64(),
            finished["total"].as_u64(),
            finished["skipped"].as_u64()
        ),
        (Some(1), Some(1), Some(0))
    );
    assert!(finished["elapsed_ms"].is_number());
    let written = &named(&all, "file_version_written")[0];
    assert_eq!(written["place_id"], place.id.to_string());
    assert_eq!(written["version"], u64::from(version.version));
    assert_eq!(written["origin_kind"], "person");
    let granted = &named(&all, "grants_published")[0];
    assert_eq!(granted["workspace_id"], workspace.to_string());
    assert_eq!(granted["count"], 1);
    let pruned = &named(&all, "versions_pruned")[0];
    assert_eq!(pruned["removed"], 1);
    assert!(pruned["bytes_freed"].is_number());
    assert_eq!(
        named(&all, "place_missing")[0]["place_id"],
        gone.id.to_string()
    );
    assert_eq!(
        named(&all, "place_removed")[0]["place_id"],
        place.id.to_string()
    );

    // Never a path of the person's folders, never file content.
    let everything = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(
        !everything.contains(&place.path),
        "an absolute path was logged"
    );
    assert!(
        !everything.contains("Supplier quotes"),
        "a friendly name was logged"
    );
    assert!(!everything.contains("secret"), "file content was logged");
    assert!(!everything.contains("quote.md"), "a file name was logged");
}
