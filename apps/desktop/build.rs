//! The Tauri build step for macOS and Windows (OrdinDesktop.md *Internal Logic* step 5): the
//! Command Bridge's commands are registered as the inline plug-in `ordin`, whose default permission
//! allows every one of them (`ordin:default`, CommandBridge.md). On any other target nothing runs,
//! because the program and Tauri are not built there.

fn main() {
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target != "macos" && target != "windows" {
        return;
    }
    let attributes = tauri_build::Attributes::new().plugin(
        "ordin",
        tauri_build::InlinedPlugin::new()
            .commands(ordin_command_bridge::IPC_COMMANDS)
            .default_permission(tauri_build::DefaultPermissionRule::AllowAllCommands),
    );
    if let Err(error) = tauri_build::try_build(attributes) {
        eprintln!("tauri build step failed: {error:#}");
        std::process::exit(1);
    }
}
