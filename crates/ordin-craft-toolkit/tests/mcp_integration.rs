//! Integration with the real MCP Front and Rules Engine (plan 01 Standard step gate, Playbook 7.2):
//! the provider registers, a session is offered the app's tools, an allowed call reaches the fake
//! app over its stdio and becomes a version in the real File Service, and a denied one never reaches
//! the app. SQLite on a temporary file; nothing touches the network.
#![cfg(unix)]

mod support;

use ordin_journal::EntryFilter;
use ordin_types::domain::places::FileVersionOrigin;
use serde_json::json;
use support::*;

#[tokio::test]
async fn an_allowed_call_writes_a_version_and_a_denied_one_never_reaches_the_app() {
    let tools = [
        ("resize", WRITES_OUTPUT),
        ("crop", WRITES_OUTPUT),
        ("export_all", WRITES_OUTPUT),
    ];
    let stack = Stack::start(&tools).await;
    let (names, prefixes) = stack.front.server_names();
    assert!(names.iter().any(|n| n.as_str() == "craft_photocraft"));
    assert!(prefixes.contains(&"craft_"));

    let (session, _) = stack.open(&["crop"]).await;
    let offered: Vec<String> = stack
        .front
        .offered_tools(session.run)
        .unwrap()
        .into_iter()
        .map(|t| t.tool)
        .collect();
    assert!(offered.contains(&"resize".to_owned()));
    assert!(offered.contains(&"crop".to_owned()));
    assert!(!offered.contains(&"export_all".to_owned()), "not defined");
    assert!(
        !offered.contains(&"export".to_owned()),
        "not allowed, not listed"
    );

    let mut bridge = session.connect("craft_photocraft").await;
    let mut listed = bridge.tools().await;
    listed.sort();
    assert_eq!(listed, ["crop", "resize"]);

    std::fs::write(stack.in_place("logo.png"), b"png").unwrap();
    let output = stack.in_place("logo-512.png");
    let answer = bridge
        .call(
            1,
            "resize",
            json!({ "input": stack.in_place("logo.png"), "output": output, "width": 512 }),
        )
        .await;
    assert!(!is_error(&answer), "{answer}");
    assert!(text_of(&answer).contains("logo-512.png"), "{answer}");
    assert!(!text_of(&answer).contains("craft-scratch"), "{answer}");

    let versions = stack
        .files
        .versions(stack.place.id, "logo-512.png")
        .await
        .unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(
        versions[0].origin,
        FileVersionOrigin::Run {
            run_id: session.run,
            tool: "craft_photocraft.resize".to_owned()
        }
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"PNGDATA");

    // Refused by the agent's tool policy before the provider acts: no version, no journal line.
    let denied = stack.in_place("denied.png");
    let answer = bridge
        .call(
            2,
            "crop",
            json!({ "input": stack.in_place("logo.png"), "output": denied }),
        )
        .await;
    assert!(is_error(&answer), "{answer}");
    assert!(!std::path::Path::new(&denied).exists());
    let page = stack
        .journal
        .list_entries(
            EntryFilter {
                run_id: Some(session.run),
                kind: Some("craft_tool_called".to_owned()),
            },
            None,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].component, "craft");

    // A relative path is the MCP Front's argument finding, through `check_arguments`.
    let answer = bridge
        .call(
            3,
            "resize",
            json!({ "input": "logo.png", "output": output }),
        )
        .await;
    assert!(is_error(&answer), "{answer}");
    assert!(
        text_of(&answer).contains("Give the file as a full path."),
        "{answer}"
    );

    stack.front.close_session(session.run).await.unwrap();
    let scratch = stack
        .dir
        .0
        .join("data/craft-scratch")
        .join(session.run.to_string());
    assert!(
        !scratch.exists(),
        "session_closed deletes the scratch folder"
    );
    stack.toolkit.shutdown().await;
}
