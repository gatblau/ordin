//! The event-order guard every adapter's stream passes through (Internal Logic 7).

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::{BoxStream, Stream, StreamExt};
use ordin_types::RunId;

use crate::adapter::{PauseReason, RunEnd, RunEvent};
use crate::check::is_local;
use crate::error::CheckError;
use crate::{AdapterKind, RunEndReason};

/// Shortest and longest harness plan, in steps (C-41).
const PLAN_STEPS: std::ops::RangeInclusive<usize> = 3..=7;
/// Longest harness plan step, in scalar values.
const MAX_PLAN_STEP: usize = 80;
/// Longest progress line, in scalar values.
const MAX_PROGRESS_LINE: usize = 160;
/// Longest `Person.question`, in scalar values.
const MAX_QUESTION: usize = 500;
/// Longest final message, in scalar values.
const MAX_FINAL_MESSAGE: usize = 32_768;

/// One item of a guarded event stream.
#[derive(Debug, Clone)]
pub enum Guarded {
    /// An event that passed every check, in stream order.
    Event(RunEvent),
    /// An event dropped, or a synthetic end added, with the reason; the caller journals it.
    Violation(CheckError),
}

/// Wraps an adapter's event stream so it yields only events in the order 2C fixes.
///
/// The guard never reorders events and holds at most one event: an `Ended` waiting behind the
/// violations that corrected it.
pub fn guard_events(
    kind: AdapterKind,
    run_id: RunId,
    inner: BoxStream<'static, RunEvent>,
) -> BoxStream<'static, Guarded> {
    Box::pin(Guard {
        kind,
        run_id: run_id.to_string(),
        inner: Some(inner),
        state: State::BeforeStart,
        ready: VecDeque::with_capacity(3),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    BeforeStart,
    Running,
    Ended,
}

struct Guard {
    kind: AdapterKind,
    /// The run identifier in its wire form, for the violation log line.
    run_id: String,
    /// `None` once the inner stream has finished.
    inner: Option<BoxStream<'static, RunEvent>>,
    state: State,
    /// Items decided but not yet yielded: violations, then at most one event.
    ready: VecDeque<Guarded>,
}

/// The wire name of `kind`, for the log line.
fn kind_name(kind: AdapterKind) -> &'static str {
    match kind {
        AdapterKind::ClaudeLocal => "claude_local",
        AdapterKind::CopilotLocal => "copilot_local",
        AdapterKind::HostedAgent => "hosted_agent",
        AdapterKind::SlackThread => "slack_thread",
    }
}

fn chars_within(text: &str, min: usize, max: usize) -> bool {
    let count = text.chars().count();
    (min..=max).contains(&count)
}

impl Guard {
    /// Queues a violation and logs it once. The line holds the run, the adapter and the code,
    /// never the event's text (*Security*).
    fn violation(&mut self, code: &'static str, field: &'static str) {
        tracing::warn!(
            run_id = self.run_id.as_str(),
            adapter = kind_name(self.kind),
            code = code,
            "ra.event_violation"
        );
        self.ready
            .push_back(Guarded::Violation(CheckError::new(code, field)));
    }

    fn pass(&mut self, event: RunEvent) {
        self.ready.push_back(Guarded::Event(event));
    }

    /// Decides one event from the inner stream; queues at least one item.
    fn accept(&mut self, event: RunEvent) {
        match (self.state, event) {
            (State::BeforeStart, event @ RunEvent::Started { .. }) => {
                self.state = State::Running;
                self.pass(event);
            }
            (State::BeforeStart, _)
            | (State::Running, RunEvent::Started { .. })
            | (State::Ended, _) => {
                self.violation("RA_EVENT_ORDER", "events");
            }
            // C-41 fallback: a plan outside the bounds is dropped and the run continues with
            // events only.
            // spec: ordin-desktop/components/RuntimeAdapterInterface#C-41
            (State::Running, RunEvent::HarnessPlan { steps }) => {
                if PLAN_STEPS.contains(&steps.len())
                    && steps.iter().all(|s| chars_within(s, 1, MAX_PLAN_STEP))
                {
                    self.pass(RunEvent::HarnessPlan { steps });
                } else {
                    self.violation("RA_PLAN_INVALID", "steps");
                }
            }
            (State::Running, RunEvent::Progress { progress }) => {
                if !chars_within(&progress.line, 1, MAX_PROGRESS_LINE) {
                    self.violation("RA_EVENT_INVALID", "progress.line");
                } else if progress
                    .count
                    .is_some_and(|c| c.total.is_some_and(|total| total < c.done))
                {
                    self.violation("RA_EVENT_INVALID", "progress.count");
                } else {
                    self.pass(RunEvent::Progress { progress });
                }
            }
            (State::Running, RunEvent::Paused { reason }) => match &reason {
                PauseReason::Person { question } if !chars_within(question, 1, MAX_QUESTION) => {
                    self.violation("RA_EVENT_INVALID", "question");
                }
                PauseReason::RemoteApprovers if self.kind != AdapterKind::HostedAgent => {
                    self.violation("RA_EVENT_INVALID", "reason");
                }
                PauseReason::DecisionElsewhere { .. } if self.kind != AdapterKind::SlackThread => {
                    self.violation("RA_EVENT_INVALID", "reason");
                }
                _ => self.pass(RunEvent::Paused { reason }),
            },
            (State::Running, RunEvent::Ended { end }) => {
                let end = self.correct_end(end);
                self.state = State::Ended;
                self.pass(RunEvent::Ended { end });
            }
            (State::Running, event @ (RunEvent::Resumed | RunEvent::Usage { .. })) => {
                self.pass(event);
            }
        }
    }

    /// Replaces an end reason the route cannot report with `Failed`, and cuts an over-long final
    /// message, queueing one violation for each correction.
    fn correct_end(&mut self, mut end: RunEnd) -> RunEnd {
        let wrong_reason = match end.reason {
            RunEndReason::NoReply => self.kind != AdapterKind::SlackThread,
            RunEndReason::Abandoned => is_local(self.kind),
            _ => false,
        };
        if wrong_reason {
            end.reason = RunEndReason::Failed;
            self.violation("RA_EVENT_INVALID", "reason");
        }
        if let Some(message) = end.final_message.as_mut() {
            if let Some((cut, _)) = message.char_indices().nth(MAX_FINAL_MESSAGE) {
                message.truncate(cut);
                self.violation("RA_EVENT_INVALID", "final_message");
            }
        }
        end
    }

    /// The inner stream finished: a run that never ended gets a violation and a synthetic end.
    fn finish(&mut self) {
        self.inner = None;
        if self.state != State::Ended {
            self.violation("RA_EVENT_ORDER", "events");
            self.state = State::Ended;
            self.pass(RunEvent::Ended {
                end: RunEnd {
                    reason: RunEndReason::Interrupted,
                    final_message: None,
                    artefacts: vec![],
                },
            });
        }
    }
}

impl Stream for Guard {
    type Item = Guarded;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Guarded>> {
        let this = self.get_mut();
        if let Some(item) = this.ready.pop_front() {
            return Poll::Ready(Some(item));
        }
        let Some(inner) = this.inner.as_mut() else {
            return Poll::Ready(None);
        };
        match inner.poll_next_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(event)) => {
                this.accept(event);
                Poll::Ready(this.ready.pop_front())
            }
            Poll::Ready(None) => {
                this.finish();
                Poll::Ready(this.ready.pop_front())
            }
        }
    }
}
