//! Settings › Memory's behaviour settings, enforced by this service (MEM-SET-1 to MEM-SET-5;
//! amended January 2026; user decision; Approved by the user January 2026).

use std::time::{Duration, Instant};

use ordin_types::AgentInstanceId;

use super::support::*;
use crate::model::*;

fn with(change: impl FnOnce(&mut MemorySettings)) -> MemorySettings {
    let mut s = MemorySettings::default();
    change(&mut s);
    s
}

fn document() -> FactSource {
    FactSource {
        origin_ref: "Reports/Q3 board pack.docx".to_owned(),
        version: "7".to_owned(),
    }
}

async fn held_count(rig: &Rig, w: ordin_types::WorkspaceId, count: usize) {
    let started = Instant::now();
    loop {
        let held = rig
            .svc
            .held_writes(MemoryPlace::Workspace(w))
            .await
            .expect("held writes");
        if held.len() >= count {
            assert_eq!(held.len(), count);
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "nothing held");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

// spec: ordin-desktop/components/MemoryService#MEM-SET-1
#[tokio::test]
async fn the_defaults_are_todays_behaviour_and_a_change_is_read_back() {
    let rig = Rig::desktop().await;
    assert_eq!(rig.svc.settings(), MemorySettings::default());
    assert_eq!(rig.svc.settings().keep, KeepMode::OnItsOwn);
    assert!(rig.svc.settings().learn_from_conversations);
    assert!(rig.svc.settings().learn_from_documents);
    assert_eq!(rig.svc.settings().suggest_removal_after_days, None);
    assert!(rig.svc.settings().use_everywhere);
    assert!(!rig.svc.settings().say_which_facts);
    let s = with(|s| {
        s.keep = KeepMode::AskFirst;
        s.suggest_removal_after_days = Some(90);
        s.say_which_facts = true;
    });
    rig.svc.set_settings(s).expect("set");
    assert_eq!(rig.svc.settings(), s);
    rig.stop().await;
}

#[tokio::test]
async fn a_period_outside_1_to_3650_days_is_refused_and_nothing_changes() {
    let rig = Rig::desktop().await;
    for days in [0u16, 3651] {
        let s = with(|s| s.suggest_removal_after_days = Some(days));
        assert_eq!(
            code_of(rig.svc.set_settings(s)),
            ("MEM_LIMIT_INVALID", false)
        );
    }
    assert_eq!(rig.svc.settings(), MemorySettings::default());
    for days in [1u16, 3650] {
        rig.svc
            .set_settings(with(|s| s.suggest_removal_after_days = Some(days)))
            .expect("in range");
    }
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-SET-2
#[tokio::test]
async fn ask_first_holds_ordins_conversation_fact_for_the_person() {
    let rig = Rig::desktop().await;
    rig.svc
        .set_settings(with(|s| s.keep = KeepMode::AskFirst))
        .expect("set");
    rig.svc
        .remember_text(rig.ordin(rig.w1), input("Board meets on Tuesdays.", None))
        .await
        .expect("remember");
    held_count(&rig, rig.w1, 1).await;
    // The person keeps it: their keep is never held.
    let kept = rig.keep_held_writes(rig.w1, 1).await;
    assert_eq!(rig.state(kept[0]).await, FactState::Kept);
    rig.stop().await;
}

#[tokio::test]
async fn on_its_own_keeps_ordins_conversation_fact_without_asking() {
    let rig = Rig::desktop().await;
    let id = rig.fact(rig.w1, "Board meets on Tuesdays.").await;
    assert_eq!(rig.state(id).await, FactState::Kept);
    assert!(rig
        .svc
        .held_writes(MemoryPlace::Workspace(rig.w1))
        .await
        .unwrap()
        .is_empty());
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-SET-3
#[tokio::test]
async fn conversations_turned_off_refuses_ordins_and_a_crew_members_fact() {
    let rig = Rig::desktop().await;
    rig.svc
        .set_settings(with(|s| s.learn_from_conversations = false))
        .expect("set");
    assert_eq!(
        code_of(
            rig.svc
                .remember_text(rig.ordin(rig.w1), input("Board meets on Tuesdays.", None))
                .await
        ),
        ("MEM_SOURCE_TURNED_OFF", false)
    );
    assert_eq!(
        code_of(
            rig.svc
                .remember_text(
                    rig.crew(rig.w1, AgentInstanceId::new()),
                    input("Board meets on Tuesdays.", None)
                )
                .await
        ),
        ("MEM_SOURCE_TURNED_OFF", false)
    );
    // A fact from a document is still kept.
    let mut from_doc = input("Revenue grew 4%.", Some("Q3"));
    from_doc.source = Some(document());
    rig.svc
        .remember_text(rig.ordin(rig.w1), from_doc)
        .await
        .expect("a document is still a source");
    rig.stop().await;
}

#[tokio::test]
async fn documents_turned_off_refuses_a_fact_from_a_document_only() {
    let rig = Rig::desktop().await;
    rig.svc
        .set_settings(with(|s| s.learn_from_documents = false))
        .expect("set");
    let mut from_doc = input("Revenue grew 4%.", Some("Q3"));
    from_doc.source = Some(document());
    assert_eq!(
        code_of(rig.svc.remember_text(rig.ordin(rig.w1), from_doc).await),
        ("MEM_SOURCE_TURNED_OFF", false)
    );
    let id = rig.fact(rig.w1, "Board meets on Tuesdays.").await;
    assert_eq!(rig.state(id).await, FactState::Kept);
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-SET-4
#[tokio::test]
async fn a_set_period_suggests_removal_after_that_many_unused_days_only() {
    let rig = Rig::desktop().await;
    let id = rig.fact(rig.w1, "Board meets on Tuesdays.").await;
    let mut fact = rig.stored(id).await.expect("stored");
    let now = chrono::Utc::now();
    // No period: the decay rule's answer stands, whichever it is.
    assert!(rig.svc.inner.fading_of(&fact, now, true));
    assert!(!rig.svc.inner.fading_of(&fact, now, false));
    rig.svc
        .set_settings(with(|s| s.suggest_removal_after_days = Some(30)))
        .expect("set");
    fact.last_recalled_at = None;
    fact.ingested_at = now - chrono::Duration::days(29);
    assert!(!rig.svc.inner.fading_of(&fact, now, true));
    fact.ingested_at = now - chrono::Duration::days(30);
    assert!(rig.svc.inner.fading_of(&fact, now, false));
    // A recent use counts, not the write.
    fact.last_recalled_at = Some(now - chrono::Duration::days(2));
    assert!(!rig.svc.inner.fading_of(&fact, now, false));
    // A suggestion only: the fact is still kept.
    assert_eq!(rig.state(id).await, FactState::Kept);
    rig.stop().await;
}

// spec: ordin-desktop/components/MemoryService#MEM-SET-5
#[tokio::test]
async fn everywhere_turned_off_keeps_facts_to_their_workspace() {
    let rig = Rig::desktop().await;
    let f = rig.fact(rig.w1, "Prefers pricing tables in euros").await;
    let view = rig.svc.remember_everywhere(f).await.expect("widen");
    rig.kept(view.id).await;
    rig.svc
        .set_settings(with(|s| s.use_everywhere = false))
        .expect("set");
    // Not used in another workspace…
    assert!(statements(&rig.recall_text(rig.w2, "pricing tables").await).is_empty());
    // …but still in its own.
    assert_eq!(
        statements(&rig.recall_text(rig.w1, "pricing tables").await),
        vec!["Prefers pricing tables in euros".to_owned()]
    );
    let other = rig.fact(rig.w1, "Board meets on Tuesdays.").await;
    assert_eq!(
        code_of(rig.svc.remember_everywhere(other).await),
        ("MEM_EVERYWHERE_TURNED_OFF", false)
    );
    // Turned on again, the fact is used everywhere as before.
    rig.svc
        .set_settings(MemorySettings::default())
        .expect("set");
    assert_eq!(
        statements(&rig.recall_text(rig.w2, "pricing tables").await),
        vec!["Prefers pricing tables in euros".to_owned()]
    );
    rig.stop().await;
}
