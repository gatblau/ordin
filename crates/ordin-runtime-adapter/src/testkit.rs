//! The scripted adapter for other crates' tests (feature `testkit`, *chosen here* in the spec).
//!
//! Enabled only through a crate's `[dev-dependencies]`; never in a program's normal build.

use futures::stream::{self, BoxStream, StreamExt};

use crate::adapter::{
    AdapterContext, ResumeDecision, RunEvent, RunHandle, RunSnapshot, RuntimeAdapter, StartRun,
};
use crate::check::check_verb;
use crate::error::AdapterError;
use crate::{AdapterKind, AdapterVerb};

/// A scripted adapter for other crates' tests: `start` returns a handle, `events` replays
/// `script` in order, `read` returns `snapshot`, unsupported verbs return VerbNotOffered.
pub struct ScriptedAdapter {
    pub kind: AdapterKind,
    pub unsupported: &'static [AdapterVerb],
    pub script: Vec<RunEvent>,
    pub snapshot: Option<RunSnapshot>,
    pub start_error: Option<AdapterError>,
}

#[async_trait::async_trait]
impl RuntimeAdapter for ScriptedAdapter {
    fn kind(&self) -> AdapterKind {
        self.kind
    }

    fn unsupported_verbs(&self) -> &'static [AdapterVerb] {
        self.unsupported
    }

    /// Returns `start_error` when set. Otherwise the handle carries the request's run and the
    /// identifiers of the script's first `Started` event (*chosen here*: the spec says only that
    /// `start` returns a handle).
    async fn start(&self, _ctx: &AdapterContext, req: StartRun) -> Result<RunHandle, AdapterError> {
        check_verb(self, AdapterVerb::Start)?;
        if let Some(error) = &self.start_error {
            return Err(error.clone());
        }
        let (runtime_session_id, remote_thread_id) = self
            .script
            .iter()
            .find_map(|event| match event {
                RunEvent::Started {
                    runtime_session_id,
                    remote_thread_id,
                } => Some((runtime_session_id.clone(), remote_thread_id.clone())),
                _ => None,
            })
            .unwrap_or((None, None));
        Ok(RunHandle {
            run_id: req.run_id,
            adapter: self.kind,
            runtime_session_id,
            remote_thread_id,
        })
    }

    /// Replays `script` in order, whatever the handle.
    fn events(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
    ) -> Result<BoxStream<'static, RunEvent>, AdapterError> {
        Ok(stream::iter(self.script.clone()).boxed())
    }

    async fn resume(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
        _decision: ResumeDecision,
    ) -> Result<(), AdapterError> {
        check_verb(self, AdapterVerb::Resume)
    }

    /// Returns `snapshot`; with none scripted, `UnknownRun` (*chosen here*).
    async fn read(
        &self,
        _ctx: &AdapterContext,
        _run: &RunHandle,
    ) -> Result<RunSnapshot, AdapterError> {
        check_verb(self, AdapterVerb::Read)?;
        self.snapshot.clone().ok_or(AdapterError::UnknownRun)
    }

    async fn cancel(&self, _ctx: &AdapterContext, _run: &RunHandle) -> Result<(), AdapterError> {
        check_verb(self, AdapterVerb::Cancel)
    }
}
