//! The computer's notifications (OrdinDesktop.md *Internal Logic* step 7, UX-A4; DSK-NTF-1): which
//! Service API event raises a notice, under which Settings › Notifications switch, and in which
//! words. The window shell (`window.rs`) reads the Command Bridge's merged stream, asks [`Notifier`]
//! what to show and shows it with the notification plug-in; this part holds no Tauri type, so it is
//! tested on every platform.
//!
//! Amended January 2026 (user decision; Approved by the user January 2026): the switches were saved
//! and never read. The notices raised are approvals, finished work and questions; the handover and
//! draft-review notices wait for their modules, and their switches are not listed until then.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use ordin_service_api::blocks::Block;
use ordin_service_api::{EventEnvelope, ServiceEvent};
use ordin_types::domain::{CrewRunState, RunState};
use ordin_types::{ApprovalId, BackendId};

use crate::prefs::NotificationPreferences;

/// The `notifications.*` keys whose notice the program raises, in the order Settings lists them.
// spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
pub const RAISED: [&str; 3] = [
    "notifications.approvals",
    "notifications.finished_work",
    "notifications.questions",
];

/// A question's text is cut to this many characters in its notice.
pub const QUESTION_CHARS: usize = 120;

/// How long an approval notice is held so that one decided at once is not shown (step 7.3), in ms.
pub const APPROVAL_HOLD_MS: u64 = 1_500;

/// At most this many decided approvals are remembered; the set is emptied when it is full.
const DECIDED_KEPT: usize = 1_000;

/// Which switch a notice belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Approval,
    FinishedWork,
    Question,
}

impl NoticeKind {
    /// Whether the person's switch for this kind is on.
    pub fn allowed(self, prefs: &NotificationPreferences) -> bool {
        match self {
            NoticeKind::Approval => prefs.approvals,
            NoticeKind::FinishedWork => prefs.finished_work,
            NoticeKind::Question => prefs.questions,
        }
    }

    /// The `kind` of the `notification` log line.
    pub fn log_name(self) -> &'static str {
        match self {
            NoticeKind::Approval => "approval",
            NoticeKind::FinishedWork => "finished_work",
            NoticeKind::Question => "question",
        }
    }
}

/// The words of one notice, before they are put in the person's language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Words {
    /// `notification-approval-title` / `notification-approval-body`.
    Approval { question: String },
    /// `notification-run-done-title-unnamed` / `notification-done-body`.
    RunDone,
    /// `notification-crew-done-title` / `notification-done-body`.
    CrewDone,
    /// `notification-question-title`; the body is the question itself.
    Question { question: String },
}

/// One notice to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: NoticeKind,
    pub words: Words,
    /// The server's display name for a server's event; `None` for this computer's.
    pub server: Option<String>,
    /// Set for an approval: the notice is held and dropped when it is decided meanwhile.
    pub approval: Option<ApprovalId>,
    /// What the notice opens: the `NotificationTarget` sent as `ordin://open-target`.
    pub target: serde_json::Value,
}

/// The notices of one program run: maps each event and remembers the approvals already decided.
pub struct Notifier {
    local: BackendId,
    decided: Mutex<HashSet<ApprovalId>>,
    /// The target of the last notice shown and when it was shown (step 7.5).
    pending: Mutex<Option<(serde_json::Value, Instant)>>,
}

/// A notice opens its item when the window gains focus within this time of it being shown (step 7.5).
pub const OPEN_WITHIN: Duration = Duration::from_secs(5 * 60);

impl Notifier {
    pub fn new(local: BackendId) -> Notifier {
        Notifier {
            local,
            decided: Mutex::new(HashSet::new()),
            pending: Mutex::new(None),
        }
    }

    /// The notice `env` raises, if any (step 7.1). `server_name` names a connected server's backend.
    /// An `ApprovalDecided` raises none and is remembered for [`Notifier::still_pending`].
    pub fn notice(
        &self,
        env: &EventEnvelope,
        server_name: impl FnOnce(BackendId) -> Option<String>,
    ) -> Option<Notice> {
        let (kind, words, approval) = match &env.event {
            ServiceEvent::ApprovalRequested { card } => (
                NoticeKind::Approval,
                Words::Approval {
                    question: card.question.clone(),
                },
                Some(card.approval_id),
            ),
            ServiceEvent::ApprovalDecided { approval_id, .. } => {
                let mut decided = self.decided.lock().unwrap_or_else(|e| e.into_inner());
                if decided.len() >= DECIDED_KEPT {
                    decided.clear();
                }
                decided.insert(*approval_id);
                return None;
            }
            ServiceEvent::RunStateChanged {
                state: RunState::Completed,
                ..
            } => (NoticeKind::FinishedWork, Words::RunDone, None),
            ServiceEvent::CrewRoundChanged {
                state: CrewRunState::Finished,
                ..
            } => (NoticeKind::FinishedWork, Words::CrewDone, None),
            ServiceEvent::BlockShown {
                block: Block::Question(q),
                ..
            } => (
                NoticeKind::Question,
                Words::Question {
                    question: q.question.chars().take(QUESTION_CHARS).collect(),
                },
                None,
            ),
            _ => return None,
        };
        let server = if env.backend == self.local {
            None
        } else {
            Some(server_name(env.backend).unwrap_or_default())
        };
        Some(Notice {
            kind,
            words,
            server,
            approval,
            target: target_of(env)?,
        })
    }

    /// Step 7.4: a notice was shown; its target is opened at the next focus.
    pub fn shown(&self, notice: &Notice) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((notice.target.clone(), Instant::now()));
    }

    /// Step 7.5: the main window gained focus. The last notice's target, once, when it was shown
    /// within [`OPEN_WITHIN`]; clicking the notice brings the window forward, which is this focus.
    pub fn take_on_focus(&self) -> Option<serde_json::Value> {
        let (target, at) = self
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()?;
        (at.elapsed() <= OPEN_WITHIN).then_some(target)
    }

    /// Step 7.3: after the hold, an approval decided meanwhile is not shown (nothing is notified
    /// twice). Any other notice is always still pending.
    pub fn still_pending(&self, notice: &Notice) -> bool {
        match notice.approval {
            Some(id) => !self
                .decided
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&id),
            None => true,
        }
    }
}

/// The `NotificationTarget` (OrdinDesktop.md *Public Interface*) of an event that raises a notice.
// spec: ordin-desktop/components/OrdinDesktop#DSK-NTF-1
fn target_of(env: &EventEnvelope) -> Option<serde_json::Value> {
    let backend = env.backend.to_string();
    let target = match &env.event {
        ServiceEvent::ApprovalRequested { card } => serde_json::json!({
            "kind": "approval", "backend": backend,
            "workspace_id": card.workspace_id.to_string(), "approval_id": card.approval_id.to_string(),
        }),
        ServiceEvent::RunStateChanged { run_id, .. } => serde_json::json!({
            "kind": "run", "backend": backend, "run_id": run_id.to_string(),
        }),
        ServiceEvent::CrewRoundChanged { crew_run_id, .. } => serde_json::json!({
            "kind": "crew_run", "backend": backend, "crew_run_id": crew_run_id.to_string(),
        }),
        ServiceEvent::BlockShown {
            workspace_id,
            block_id,
            ..
        } => serde_json::json!({
            "kind": "question", "backend": backend,
            "workspace_id": workspace_id.to_string(), "block_id": block_id.to_string(),
        }),
        _ => return None,
    };
    Some(target)
}

/// Step 7.2: a notice is shown only when its switch is on and the person is not already looking at
/// the window.
pub fn should_show(
    kind: NoticeKind,
    prefs: &NotificationPreferences,
    window_in_front: bool,
) -> bool {
    kind.allowed(prefs) && !window_in_front
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::DesktopPreferences;
    use ordin_service_api::blocks::QuestionBlock;
    use ordin_service_api::ServiceApiVersion;
    use ordin_types::time::Timestamp;
    use ordin_types::{BlockId, CrewRunId, RunId, WorkspaceId};

    fn backend(n: u128) -> BackendId {
        BackendId(uuid::Uuid::from_u128(n))
    }

    fn env(backend: BackendId, event: ServiceEvent) -> EventEnvelope {
        EventEnvelope {
            api_version: ServiceApiVersion { major: 1, minor: 0 },
            backend,
            seq: 1,
            at: Timestamp::now(),
            event,
        }
    }

    #[test]
    fn a_finished_run_and_crew_raise_finished_work_and_other_states_nothing() {
        let n = Notifier::new(backend(1));
        let done = n
            .notice(
                &env(
                    backend(1),
                    ServiceEvent::RunStateChanged {
                        run_id: RunId(uuid::Uuid::from_u128(5)),
                        state: RunState::Completed,
                    },
                ),
                |_| None,
            )
            .expect("notice");
        assert_eq!(done.kind, NoticeKind::FinishedWork);
        assert_eq!(done.words, Words::RunDone);
        assert_eq!(done.server, None);
        assert!(n
            .notice(
                &env(
                    backend(1),
                    ServiceEvent::RunStateChanged {
                        run_id: RunId(uuid::Uuid::from_u128(5)),
                        state: RunState::Running,
                    },
                ),
                |_| None,
            )
            .is_none());
        let crew = n
            .notice(
                &env(
                    backend(2),
                    ServiceEvent::CrewRoundChanged {
                        crew_run_id: CrewRunId(uuid::Uuid::from_u128(6)),
                        round: 2,
                        state: CrewRunState::Finished,
                    },
                ),
                |_| Some("Northwind server".to_owned()),
            )
            .expect("notice");
        assert_eq!(crew.words, Words::CrewDone);
        assert_eq!(crew.server.as_deref(), Some("Northwind server"));
    }

    #[test]
    fn a_question_is_cut_to_its_first_characters() {
        let n = Notifier::new(backend(1));
        let long = "q".repeat(400);
        let notice = n
            .notice(
                &env(
                    backend(1),
                    ServiceEvent::BlockShown {
                        workspace_id: WorkspaceId(uuid::Uuid::from_u128(7)),
                        block_id: BlockId(uuid::Uuid::from_u128(8)),
                        block: Block::Question(QuestionBlock {
                            question: long,
                            choices: Vec::new(),
                            allow_other: true,
                        }),
                        author: None,
                    },
                ),
                |_| None,
            )
            .expect("notice");
        assert_eq!(notice.kind, NoticeKind::Question);
        let Words::Question { question } = notice.words else {
            panic!("question words")
        };
        assert_eq!(question.chars().count(), QUESTION_CHARS);
    }

    #[test]
    fn an_approval_decided_during_the_hold_is_not_shown() {
        let n = Notifier::new(backend(1));
        let id = ApprovalId(uuid::Uuid::from_u128(9));
        let notice = Notice {
            kind: NoticeKind::Approval,
            words: Words::Approval {
                question: "Publish?".to_owned(),
            },
            server: None,
            approval: Some(id),
            target: serde_json::json!({ "kind": "approval" }),
        };
        assert!(n.still_pending(&notice));
        assert!(n
            .notice(
                &env(
                    backend(1),
                    ServiceEvent::ApprovalDecided {
                        approval_id: id,
                        state: ordin_types::domain::ApprovalState::Approved,
                        decided_on: "This computer".to_owned(),
                    },
                ),
                |_| None,
            )
            .is_none());
        assert!(!n.still_pending(&notice));
    }

    #[test]
    fn each_switch_turns_its_own_kind_off_and_the_window_in_front_shows_none() {
        let mut prefs = DesktopPreferences::default().notifications;
        assert!(should_show(NoticeKind::Approval, &prefs, false));
        assert!(!should_show(NoticeKind::Approval, &prefs, true));
        prefs.approvals = false;
        assert!(!should_show(NoticeKind::Approval, &prefs, false));
        assert!(should_show(NoticeKind::Question, &prefs, false));
        prefs.questions = false;
        prefs.finished_work = false;
        assert!(!should_show(NoticeKind::Question, &prefs, false));
        assert!(!should_show(NoticeKind::FinishedWork, &prefs, false));
    }

    #[test]
    fn the_shown_notice_opens_its_item_once_at_the_next_focus() {
        let n = Notifier::new(backend(1));
        assert_eq!(n.take_on_focus(), None);
        let run = RunId(uuid::Uuid::from_u128(5));
        let notice = n
            .notice(
                &env(
                    backend(1),
                    ServiceEvent::RunStateChanged {
                        run_id: run,
                        state: RunState::Completed,
                    },
                ),
                |_| None,
            )
            .expect("notice");
        assert_eq!(notice.target["kind"], "run");
        assert_eq!(notice.target["run_id"], run.to_string());
        assert_eq!(notice.target["backend"], backend(1).to_string());
        n.shown(&notice);
        assert_eq!(n.take_on_focus(), Some(notice.target.clone()));
        assert_eq!(n.take_on_focus(), None);
    }
}
