//! The service's public data (MinutesService.md *Public Interface*; 2C wire §23, WIRE-MIN-3 and
//! WIRE-MIN-4).

use ordin_types::id::{MinutesId, MinutesJobId, RecordingId, WorkspaceId};
use ordin_types::wire::minutes::MinutesSource;
use tokio_util::sync::CancellationToken;

use crate::error::MinutesError;

/// Defined once in the shared types (2C wire §23, WIRE-MIN-3 and WIRE-MIN-4).
pub use ordin_types::wire::minutes::{
    MinutesJob, MinutesJobPurpose, MinutesJobState, MinutesSettings, MinutesSettingsChange,
    MinutesStatus, RecordingView, SpeechModelSetting,
};

/// What Ordin is asked to write minutes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteMinutesRequest {
    pub workspace_id: WorkspaceId,
    pub job_id: MinutesJobId,
    pub source: MinutesSource,
    pub source_name: String,
    pub recording: Option<RecordingId>,
    /// Rendered by Internal Logic B5, at most 400,000 characters.
    pub transcript_text: String,
}

/// Ordin's minutes request, implemented by the desktop program over `OrdinHost::write_minutes`.
#[async_trait::async_trait]
pub trait MinutesWriter: Send + Sync {
    /// The name shown in the one-time notice: the AI service Ordin's runs use.
    fn writer_name(&self) -> String;
    /// Runs Ordin's minutes request and waits for its end; Ok with the id `keep_minutes` returned.
    async fn write(
        &self,
        req: WriteMinutesRequest,
        cancel: CancellationToken,
    ) -> Result<MinutesId, MinutesError>;
}
