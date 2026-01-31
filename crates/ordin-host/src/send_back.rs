//! OH-PF-1: a send-back that fails says so in the workspace's conversation, with **Try again** for the
//! items the Push Service may send again (PushService.md §C6, `try_again`).

use ordin_types::{PendingPushId, WorkingCopyItemId, WorkspaceId};

use crate::host::COMPONENT;
use crate::host::{Kept, OrdinHost};

/// The two choices of the failure question.
pub(crate) const TRY_AGAIN: &str = "Try again";
pub(crate) const NOT_NOW: &str = "Not now";

/// What the person is told for a Push Service code, and whether **Try again** may send it again.
/// `PUSH_APPROVED_VERSION_CHANGED` set the item back to changed (a new approval is needed) and
/// `PUSH_OUTCOME_UNKNOWN` is never resent automatically, so neither is tried again.
// spec: ordin-desktop/components/OrdinHost#OH-PF-1
pub(crate) fn reason(code: &str) -> (&'static str, bool) {
    match code {
        "PUSH_ORIGIN_UNREACHABLE" => ("the service could not be reached", true),
        "PUSH_SIGN_IN_NEEDED" => ("you need to sign in to the service again", true),
        "PUSH_FORBIDDEN" => ("you are not allowed to change it there", true),
        "PUSH_ORIGIN_NOT_FOUND" => ("it is no longer there", true),
        "PUSH_TOO_LARGE" => ("it is too large for the service", true),
        "PUSH_APPROVED_VERSION_CHANGED" => (
            "it changed after you approved it, so it needs a new approval",
            false,
        ),
        "PUSH_OUTCOME_UNKNOWN" => (
            "it is not known whether it arrived; look at the document before sending it again",
            false,
        ),
        _ => ("the service refused it", true),
    }
}

impl OrdinHost {
    /// On `PushFinished` with failed items: one message, or a question with **Try again** and
    /// **Not now** when at least one item may be sent again.
    // spec: ordin-desktop/components/OrdinHost#OH-PF-1
    pub(crate) async fn send_back_failed(
        &self,
        workspace_id: WorkspaceId,
        pending_push_id: PendingPushId,
        failed: &[(WorkingCopyItemId, &'static str)],
    ) {
        if failed.is_empty() {
            return;
        }
        let n = failed.len();
        let changes = if n == 1 {
            "1 change".to_owned()
        } else {
            format!("{n} changes")
        };
        let mut reasons: Vec<&str> = Vec::new();
        let mut again = Vec::new();
        for (item, code) in failed {
            let (why, retry) = reason(code);
            if !reasons.contains(&why) {
                reasons.push(why);
            }
            if retry {
                again.push(*item);
            }
        }
        let text = format!("Couldn't send {changes} back: {}.", reasons.join("; "));
        let shown = if again.is_empty() {
            self.show_message(workspace_id, &text).await.map(|_| ())
        } else {
            self.ask(
                workspace_id,
                &text,
                vec![TRY_AGAIN.to_owned(), NOT_NOW.to_owned()],
                Some(Kept::SendBack {
                    pending_push_id,
                    items: again,
                }),
            )
            .await
            .map(|_| ())
        };
        if let Err(e) = shown {
            tracing::warn!(
                component = COMPONENT,
                code = e.code().as_str(),
                "ordin_host.send_back_failure_not_shown"
            );
        }
    }

    /// **Try again** sends each kept item again; **Not now** does nothing.
    pub(crate) async fn send_back_answered(
        &self,
        workspace_id: WorkspaceId,
        pending_push_id: PendingPushId,
        items: &[WorkingCopyItemId],
        answer: &str,
    ) {
        if answer != TRY_AGAIN {
            return;
        }
        for item in items {
            if let Err(e) = self.ports.push.try_again(pending_push_id, *item).await {
                tracing::warn!(
                    component = COMPONENT,
                    code = e.code.as_str(),
                    "ordin_host.send_back_not_tried_again"
                );
                let _ = self
                    .show_message(workspace_id, "That did not work; try again later.")
                    .await;
                return;
            }
        }
    }
}
