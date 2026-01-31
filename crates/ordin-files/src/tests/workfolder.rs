//! The work folder's tools and operator routes on SQLite (the PostgreSQL run is in
//! `tests/pg_work_folder.rs`).

use ordin_types::paging::PAGE_SIZE;

use super::support::*;

#[tokio::test]
async fn the_agent_writes_reads_edits_and_deletes_text() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    folder
        .write("/plans//a.md", "line one\nline two\nline three\n")
        .await
        .unwrap();
    assert_eq!(
        folder.read("/plans/a.md", 0, 2000).await.unwrap(),
        "line one\nline two\nline three"
    );
    assert_eq!(folder.read("/plans/a.md", 1, 1).await.unwrap(), "line two");
    assert_eq!(folder.read("/plans/a.md", 0, 0).await.unwrap(), "");
    assert_eq!(
        folder.edit("/plans/a.md", "two", "2", false).await.unwrap(),
        1
    );
    assert_eq!(folder.read("/plans/a.md", 1, 1).await.unwrap(), "line 2");
    folder.write("/plans/b.md", "x x x").await.unwrap();
    assert!(folder
        .edit("/plans/b.md", "x", "y", false)
        .await
        .unwrap_err()
        .0
        .contains("occurs 3 times"));
    assert_eq!(folder.edit("/plans/b.md", "x", "y", true).await.unwrap(), 3);
    assert_eq!(folder.read("/plans/b.md", 0, 10).await.unwrap(), "y y y");
    assert!(folder
        .edit("/plans/b.md", "zzz", "y", false)
        .await
        .unwrap_err()
        .0
        .contains("not found"));
    assert!(folder
        .edit("/plans/b.md", "", "y", false)
        .await
        .unwrap_err()
        .0
        .contains("is empty"));
    assert!(folder
        .edit("/plans/none.md", "a", "b", false)
        .await
        .unwrap_err()
        .0
        .contains("does not exist"));
    // Overwriting keeps one row.
    folder.write("/plans/b.md", "fresh").await.unwrap();
    assert_eq!(folder.read("/plans/b.md", 0, 10).await.unwrap(), "fresh");
    folder.delete("/plans/b.md").await.unwrap();
    assert!(folder
        .delete("/plans/b.md")
        .await
        .unwrap_err()
        .0
        .contains("does not exist"));
    // A folder goes with everything under it.
    folder.delete("/plans").await.unwrap();
    assert!(folder
        .read("/plans/a.md", 0, 1)
        .await
        .unwrap_err()
        .0
        .contains("does not exist"));
}

#[tokio::test]
async fn paths_the_agent_gives_must_be_valid() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    for path in ["plans/a.md", "/a/../b", "/x~y"] {
        for refusal in [
            folder.read(path, 0, 1).await.unwrap_err(),
            folder.write(path, "x").await.unwrap_err(),
            folder.delete(path).await.unwrap_err(),
            folder.edit(path, "a", "b", false).await.unwrap_err(),
        ] {
            assert_eq!(
                refusal.0,
                format!("Error: {path} is not a valid work-folder path")
            );
        }
    }
    assert!(folder
        .ls("nope")
        .await
        .unwrap_err()
        .0
        .contains("not a valid"));
    assert!(folder.glob("*", Some("bad")).await.is_err());
    assert!(folder.grep("a", Some("bad"), None).await.is_err());
    assert!(folder
        .write("/dir/", "x")
        .await
        .unwrap_err()
        .0
        .contains("is a folder"));
    assert!(folder
        .read("/dir/", 0, 1)
        .await
        .unwrap_err()
        .0
        .contains("is a folder"));
}

#[tokio::test]
async fn the_agent_lists_the_bundle_and_the_work_folder_together() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    folder.write("/plans/a.md", "a").await.unwrap();
    folder.write("/plans/deep/b.md", "b").await.unwrap();
    folder.write("/notes.md", "n").await.unwrap();
    let root = folder.ls("/").await.unwrap();
    let paths: Vec<(&str, bool)> = root.iter().map(|e| (e.path.as_str(), e.bundle)).collect();
    assert_eq!(
        paths,
        vec![
            ("/AGENTS.md", true),
            ("/knowledge/", true),
            ("/notes.md", false),
            ("/plans/", false)
        ]
    );
    let plans = folder.ls("/plans/").await.unwrap();
    assert_eq!(
        plans.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        ["/plans/a.md", "/plans/deep/"]
    );
    assert_eq!(
        folder.ls("/plans").await.unwrap().len(),
        2,
        "a folder without its slash lists the same"
    );
    let bundle_dir = folder.ls("/knowledge/").await.unwrap();
    assert_eq!(bundle_dir.len(), 1);
    assert!(bundle_dir[0].bundle && bundle_dir[0].path == "/knowledge/note.md");
    let single = folder.ls("/notes.md").await.unwrap();
    assert_eq!((single.len(), single[0].size_bytes), (1, 1));
    let bundle_file = folder.ls("/AGENTS.md").await.unwrap();
    assert!(bundle_file[0].bundle && bundle_file[0].path == "/AGENTS.md");
    assert!(folder.ls("/missing/").await.unwrap().is_empty());
    // The bundle reads, with offset and limit in lines.
    assert_eq!(folder.read("/AGENTS.md", 1, 5).await.unwrap(), "line two");
    assert!(folder
        .read("/knowledge/none.md", 0, 5)
        .await
        .unwrap_err()
        .0
        .contains("does not exist"));
}

#[tokio::test]
async fn glob_and_grep_search_the_work_folder_and_the_bundle() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    folder
        .write("/plans/a.md", "alpha\nbeta needle\n")
        .await
        .unwrap();
    folder
        .write("/plans/sub/b.md", "needle again\n")
        .await
        .unwrap();
    folder.write("/plans/c.txt", "none here").await.unwrap();
    assert_eq!(
        folder.glob("/plans/*.md", None).await.unwrap(),
        ["/plans/a.md"]
    );
    assert_eq!(
        folder.glob("**/*.md", None).await.unwrap(),
        [
            "/AGENTS.md",
            "/knowledge/note.md",
            "/plans/a.md",
            "/plans/sub/b.md"
        ]
    );
    assert_eq!(
        folder.glob("*.md", Some("/plans")).await.unwrap(),
        ["/plans/a.md"]
    );
    assert_eq!(
        folder.glob("/AGENTS.md", None).await.unwrap(),
        ["/AGENTS.md"]
    );
    let found = folder.grep("needle", None, None).await.unwrap();
    assert_eq!(
        found
            .iter()
            .map(|m| (m.path.as_str(), m.line))
            .collect::<Vec<_>>(),
        [("/plans/a.md", 2), ("/plans/sub/b.md", 1)]
    );
    assert_eq!(found[0].text, "beta needle");
    assert_eq!(
        folder
            .grep("needle", Some("/plans/sub"), None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(folder.grep("needle", None, Some(1)).await.unwrap().len(), 1);
    assert_eq!(
        folder.grep("bundle note", None, None).await.unwrap()[0].path,
        "/knowledge/note.md"
    );
    assert!(folder
        .grep("(", None, None)
        .await
        .unwrap_err()
        .0
        .contains("not a valid regular expression"));
    // A file that is not text is not searched.
    folder
        .operator_write("/blob.bin", vec![0xff, 0xfe, 0x00])
        .await
        .unwrap();
    assert!(folder
        .grep(".", Some("/blob.bin"), None)
        .await
        .unwrap()
        .is_empty());
    assert!(folder
        .read("/blob.bin", 0, 1)
        .await
        .unwrap_err()
        .0
        .contains("not a text file"));
}

#[tokio::test]
async fn the_operator_pages_lists_and_round_trips_bytes() {
    let fx = fixture().await;
    let folder = work_folder(&fx, None, vec![]);
    for n in 0..(PAGE_SIZE + 25) {
        folder
            .operator_write(&format!("/many/f{n:03}.txt"), vec![b'x'])
            .await
            .unwrap();
    }
    folder
        .operator_write("/other.txt", vec![b'y'])
        .await
        .unwrap();
    let first = folder.operator_list("/many", None).await.unwrap();
    assert_eq!(first.items.len(), PAGE_SIZE);
    assert!(first.next_cursor.is_some());
    let second = folder
        .operator_list("/many/", first.next_cursor)
        .await
        .unwrap();
    assert_eq!(second.items.len(), 25);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.items[0].path, format!("/many/f{PAGE_SIZE:03}.txt"));
    assert!(!second.items[0].bundle);
    assert_eq!(
        folder.operator_list("/", None).await.unwrap().items.len(),
        PAGE_SIZE
    );
    // Bytes that are not UTF-8 come back unchanged; text is stored as text.
    let bytes = vec![0xff, 0x00, 0xfe, 0x41];
    folder
        .operator_write("/raw.bin", bytes.clone())
        .await
        .unwrap();
    assert_eq!(folder.operator_read("/raw.bin").await.unwrap(), bytes);
    let encoding: String =
        sqlx::query_scalar("SELECT encoding FROM work_folder_files WHERE path = '/raw.bin'")
            .fetch_one(fx.pool())
            .await
            .unwrap();
    assert_eq!(encoding, "base64");
    folder
        .operator_write("/text.txt", b"plain".to_vec())
        .await
        .unwrap();
    assert_eq!(folder.operator_read("/text.txt/").await.unwrap(), b"plain");
    let encoding: String =
        sqlx::query_scalar("SELECT encoding FROM work_folder_files WHERE path = '/text.txt'")
            .fetch_one(fx.pool())
            .await
            .unwrap();
    assert_eq!(encoding, "utf-8");
    // The operator may write anything not in the bundle and remove protected paths.
    assert_eq!(
        folder.operator_write("/", vec![]).await.unwrap_err().code(),
        "WF_BUNDLE_PATH"
    );
}

#[tokio::test]
async fn namespaces_do_not_see_each_other() {
    let fx = fixture().await;
    let one = work_folder(&fx, None, vec![]);
    let other = crate::WorkFolder::new(
        fx.store.clone(),
        crate::WorkFolderConfig {
            namespace: "other-agent".into(),
            bundle_dir: fx.dir.0.join("bundle"),
            writable_paths: Some(vec!["/".into()]),
            protected_paths: vec![],
        },
    )
    .unwrap();
    one.write("/shared.md", "mine").await.unwrap();
    assert!(!other.operator_exists("/shared.md").await.unwrap());
    other.write("/shared.md", "theirs").await.unwrap();
    assert_eq!(one.read("/shared.md", 0, 1).await.unwrap(), "mine");
    one.delete("/shared.md").await.unwrap();
    assert!(other.operator_exists("/shared.md").await.unwrap());
}

#[test]
fn work_folder_constants_match_the_spec() {
    assert_eq!(crate::WF_MAX_FILE_BYTES, 10_485_760);
    assert_eq!(crate::WF_MAX_PATH_CHARS, 1_024);
    assert_eq!(crate::WF_BUNDLE_FILES.len(), 3);
    assert_eq!(crate::WF_BUNDLE_DIR_PREFIXES.len(), 4);
    assert!(crate::is_bundle_path("/brand/logo.png") && !crate::is_bundle_path("/brands"));
}
