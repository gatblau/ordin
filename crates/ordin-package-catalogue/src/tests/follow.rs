//! Installed agents follow the harness in use (PackageCatalogue.md, amendment 2026-02-05,
//! PKG-HAR-1 to PKG-HAR-4).

use ordin_types::domain::HarnessKind;

use super::support::*;
use crate::store;
use crate::HarnessFollowed;

async fn harness_of(fx: &Fx, id: ordin_types::AgentInstanceId) -> HarnessKind {
    let mut conn = fx.pool.acquire().await.expect("a connection");
    store::get_instance(&mut conn, id)
        .await
        .expect("read")
        .expect("the instance")
        .harness
}

fn use_harness(fx: &Fx, harness: HarnessKind) {
    *lock(&fx.harness.0) = harness;
}

// spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-1
// spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-2
#[tokio::test]
async fn happy_path_an_agent_installed_for_claude_code_is_installed_again_for_copilot() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    let version = agent.instance.package_version.clone();
    use_harness(&fx, HarnessKind::CopilotCli);
    fx.clear_log();

    let out = fx.pc.follow_harness().await;

    assert_eq!(
        out,
        HarnessFollowed {
            followed: 1,
            failed: 0,
            unchanged: 0
        }
    );
    assert_eq!(harness_of(&fx, id).await, HarnessKind::CopilotCli);
    let (_, request) = lock(&fx.installers.requests)
        .last()
        .cloned()
        .expect("an install request");
    assert_eq!(request.harness, HarnessKind::CopilotCli);
    let last = lock(&fx.resolver.requests)
        .last()
        .cloned()
        .expect("a resolve request");
    assert_eq!(last.target_harness, HarnessKind::CopilotCli);
    assert_eq!(last.root_version.as_deref(), Some(version.as_str()));
    assert_eq!(count(&fx.log_lines(), "installers.update"), 1);
}

// spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-1
#[tokio::test]
async fn edge_case_nothing_to_do_when_every_instance_already_uses_the_harness() {
    let fx = Fx::new().await;
    fx.add().await;
    fx.clear_log();
    let out = fx.pc.follow_harness().await;
    assert_eq!(
        out,
        HarnessFollowed {
            followed: 0,
            failed: 0,
            unchanged: 1
        }
    );
    assert_eq!(count(&fx.log_lines(), "resolver.build"), 0);
}

// spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-3
#[tokio::test]
async fn error_path_one_instance_fails_and_the_next_is_still_followed() {
    let fx = Fx::new().await;
    let first = fx.add().await;
    let other = fx.second_agent("urn:ordin:acme:agents:planner", "Release planner");
    let second = fx.add_agent(other).await;
    use_harness(&fx, HarnessKind::CopilotCli);
    // "Analyst" is listed before "Release planner"; its build fails.
    fx.resolver
        .build_failures
        .store(1, std::sync::atomic::Ordering::SeqCst);

    let out = fx.pc.follow_harness().await;

    assert_eq!(
        out,
        HarnessFollowed {
            followed: 1,
            failed: 1,
            unchanged: 0
        }
    );
    assert_eq!(
        harness_of(&fx, first.instance.id).await,
        HarnessKind::ClaudeCode
    );
    assert_eq!(
        harness_of(&fx, second.instance.id).await,
        HarnessKind::CopilotCli
    );
}

// spec: ordin-desktop/components/PackageCatalogue#PKG-HAR-4
#[tokio::test]
async fn an_instance_with_a_running_session_is_left_as_it_is() {
    let fx = Fx::new().await;
    let agent = fx.add().await;
    let id = agent.instance.id;
    fx.activity.start(id);
    use_harness(&fx, HarnessKind::CopilotCli);
    let out = fx.pc.follow_harness().await;
    assert_eq!(
        out,
        HarnessFollowed {
            followed: 0,
            failed: 1,
            unchanged: 0
        }
    );
    assert_eq!(harness_of(&fx, id).await, HarnessKind::ClaudeCode);
}
