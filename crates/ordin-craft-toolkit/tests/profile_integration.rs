//! Integration with the real Organisation Profile and Journal: the toolkit reads a verified profile
//! the service loaded from its signed cache (`current().document.craft`, SA-RECON-06), keeps its
//! rows through the Journal's datastore, and registers module `craft` in the real migration
//! registry.
#![cfg(unix)]

mod support;

use ordin_craft_toolkit::{CopyFormat, CraftInstallationStatus, ProgramCheckState};
use serde_json::json;
use support::*;

#[tokio::test]
async fn a_verified_profile_drives_the_map_the_pins_and_the_setup_check() {
    let stack = Stack::start(&[("resize", WRITES_OUTPUT)]).await;

    let entry = stack.toolkit.file_kind("png").expect("the map is applied");
    assert_eq!(entry.open_with_app.as_deref(), Some("photocraft"));
    assert_eq!(entry.profile_version, "1.0.0");
    assert_eq!(entry.media_types, ["image/png"]);
    assert_eq!(entry.craft_tools, ["resize", "crop"], "allowed and defined");
    assert!(stack.toolkit.file_kind("xyz").is_none());

    let rows = stack.toolkit.installations().await.unwrap();
    assert_eq!(rows[0].status, CraftInstallationStatus::Installed);

    let checks = stack.toolkit.check_apps(false).await.unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].version, "0.7.0");
    assert!(matches!(checks[0].state, ProgramCheckState::Ready));

    // C-28 and C-13 are unconfirmed for this definition: no conversion is offered.
    assert!(!stack.toolkit.conversion_available(CopyFormat::Word));
    assert!(!stack.toolkit.conversion_available(CopyFormat::Pdf));
    assert!(!stack.toolkit.conversion_available(CopyFormat::Slides));
    let error = stack
        .toolkit
        .to_markdown(CopyFormat::Word, b"docx".to_vec())
        .await
        .unwrap_err();
    assert_eq!(error.code(), "CRAFT_CONVERSION_UNAVAILABLE");

    // An app that has not listed its tools inside the 1.6 s budget is offered from its definition
    // (Internal Logic 5.3), which a loaded computer can reach: a new session asks again.
    let mut tries = 0;
    let (_session, grant, mut bridge) = loop {
        let (session, grant) = stack.open(&[]).await;
        let mut bridge = session.connect("craft_photocraft").await;
        tries += 1;
        if tries == 5 || bridge.tools().await == ["resize"] {
            break (session, grant, bridge);
        }
    };
    assert!(stack.toolkit.start_up_items(&grant).await.is_empty());
    assert_eq!(bridge.tools().await, ["resize"]);
    let out = stack.in_place("no-input.png");
    let answer = bridge
        .call(
            1,
            "resize",
            json!({ "input": "/nowhere/x.png", "output": out }),
        )
        .await;
    assert!(is_error(&answer), "{answer}");
    stack.toolkit.shutdown().await;
}
