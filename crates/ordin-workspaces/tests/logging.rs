//! The log lines of *Observability* and what must never be in them (*Never logged*: workspace names,
//! folder paths, display names, email addresses and domains, origin locations, file names), alone in
//! its test binary: one global subscriber is installed before any test logs.

mod support;

use std::sync::{Arc, Mutex, OnceLock};

use ordin_workspaces::{InboxOrigin, LinkLocation, NewEmailLink};
use support::*;
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

fn buffer() -> &'static Buffer {
    static BUFFER: OnceLock<Buffer> = OnceLock::new();
    BUFFER.get_or_init(|| {
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(ordin_types::log::JsonLineFormat)
                .with_writer(move || writer.clone()),
        );
        tracing::subscriber::set_global_default(subscriber).expect("one global subscriber");
        buffer.clone()
    })
}

fn log_text() -> String {
    String::from_utf8_lossy(&buffer().0.lock().unwrap()).into_owned()
}

#[tokio::test]
async fn edge_case_no_name_path_address_or_file_name_reaches_a_log_line() {
    buffer();
    let env = Env::new().await;
    let secret_name = "Zebra-Quartz-Workspace";
    let secret_folder = "Hidden-Orchid-Folder";
    let secret_file = "Violet-Pelican-Notes.pdf";
    let secret_address = "violet.pelican@zebra-quartz.example";
    let workspace = env.service.create(secret_name.to_owned()).await.unwrap();
    let folder = env.folder(secret_folder);
    let link = env
        .service
        .add_link(
            workspace.id,
            LinkLocation::LocalFolder {
                path: folder.to_string_lossy().into_owned(),
            },
            Some("Walnut-Marmot-Label".to_owned()),
        )
        .await
        .unwrap();
    env.service
        .add_link(
            workspace.id,
            LinkLocation::GitRemote {
                url: "https://github.com/zebra-quartz/orchid-repo".to_owned(),
                branch: "main".to_owned(),
                path: String::new(),
            },
            None,
        )
        .await
        .unwrap();
    env.service
        .propose_email_link(
            workspace.id,
            NewEmailLink {
                kind: ordin_workspaces::EmailLinkKind::PeopleAndDomains,
                folder_or_label: None,
                addresses: vec![secret_address.to_owned()],
                domains: vec!["zebra-quartz.example".to_owned()],
                display_name: "Zebra-Quartz-Suppliers".to_owned(),
            },
        )
        .await
        .unwrap();
    env.service
        .save_to_inbox(
            workspace.id,
            secret_file,
            bytes::Bytes::from_static(b"x"),
            InboxOrigin::Chat {
                channel: ordin_workspaces::ChannelKind::Telegram,
            },
        )
        .await
        .unwrap();
    env.service.remove_link(link.id).await.unwrap();
    let outcome = env
        .service
        .save_to_inbox(
            workspace.id,
            "bad/name.pdf",
            bytes::Bytes::from_static(b"x"),
            InboxOrigin::Chat {
                channel: ordin_workspaces::ChannelKind::Telegram,
            },
        )
        .await;
    assert!(outcome.is_err());
    env.service.move_to_bin(workspace.id).await.unwrap();
    env.service.delete_from_bin(workspace.id).await.unwrap();

    let text = log_text();
    for marker in [
        secret_name,
        secret_folder,
        secret_file,
        secret_address,
        "zebra-quartz",
        "Walnut-Marmot-Label",
        "Zebra-Quartz-Suppliers",
        "orchid-repo",
    ] {
        assert!(!text.contains(marker), "{marker} reached a log line");
    }
    for event in [
        "workspace_created",
        "source_link_added",
        "source_link_removed",
        "workspace_moved_to_bin",
        "workspace_deleted",
    ] {
        assert!(text.contains(event), "{event} was not logged");
    }
    assert!(text.contains("\"component\":\"workspaces\""));
}
