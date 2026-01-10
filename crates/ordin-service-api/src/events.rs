//! The event catalogue, its decoding from the Journal's records and its transport form
//! (ServiceApi.md *Event catalogue*; Internal Logic 6–8).

use ordin_types::domain::{
    ApprovalState, BrandPackSource, CredentialKind, CrewRunState, RunState, RunStep,
    ServerConnectionState,
};
use ordin_types::prelude::*;
use ordin_types::wire::knowledge_query::UnansweredSource;
use ordin_types::wire::{ApprovalCard, StartUpReportItem, StartUpSubject};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blocks::Block;
use crate::commands::identity::SignInState;
use crate::commands::localisation::{ActiveLocale, LanguageChangeKind};
use crate::commands::setup::SetupProgress;
use crate::wire::{EventEnvelope, ServiceApiVersion};

/// The largest serialised event `data` (LW-6).
pub const MAX_EVENT_DATA_BYTES: usize = 256 * 1024;

const EVENT_INVALID: &str = "SVC_EVENT_INVALID";
const EVENT_TOO_LARGE: &str = "SVC_EVENT_TOO_LARGE";

// The variants are the spec's; a block or a card is carried inline, as 2C wire §1 writes them.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceEvent {
    // ---- 2C wire §1, unchanged ----
    /// A `Run`'s state changed (04 — Run lifecycle).
    RunStateChanged {
        run_id: RunId,
        state: RunState,
    },
    /// One observable progress line, or a tool count (ADR-068 "What happened").
    RunProgress {
        run_id: RunId,
        progress: RunProgress,
    },
    /// A stated or changed plan (ADR-068); `steps` are `RunStep`s of the new plan version.
    PlanChanged {
        run_id: RunId,
        plan_version: u32,
        steps: Vec<RunStep>,
    },
    /// One step's mark changed (ADR-068).
    StepMarked {
        run_id: RunId,
        plan_version: u32,
        position: u8,
        mark: StepMark,
    },
    /// The start-up report of a run or crew run is ready (G9, P5).
    StartUpReportReady {
        subject: StartUpSubject,
        items: Vec<StartUpReportItem>,
    },
    ApprovalRequested {
        card: ApprovalCard,
    },
    ApprovalDecided {
        approval_id: ApprovalId,
        state: ApprovalState,
        decided_on: String,
    },
    CrewRoundChanged {
        crew_run_id: CrewRunId,
        round: u32,
        state: CrewRunState,
    },
    /// A block Ordin or an agent showed, after it passed the block check.
    BlockShown {
        workspace_id: WorkspaceId,
        block_id: BlockId,
        block: Block,
    },
    /// Remote API only: `Last-Event-ID` is older than the journal keeps; reload by commands.
    Reset,
    // ---- added in 1.0 by this spec (chosen here) ----
    /// Desktop only, on the desktop's own backend: a paired server's link state changed (02:589).
    /// Emitted by the Server Connector.
    ServerStateChanged {
        server_connection_id: ServerConnectionId,
        state: ServerConnectionState,
        #[serde(skip_serializing_if = "Option::is_none")]
        as_of: Option<Timestamp>,
    },
    /// Something in a workspace's tree changed (a place, an agent, a workflow, a dashboard); the
    /// screens reload it with `workspaces.tree`.
    WorkspaceChanged {
        workspace_id: WorkspaceId,
    },
    /// An item was added to or settled in *Needs you*; reload with `home.needs_you`.
    NeedsYouChanged {
        workspace_id: WorkspaceId,
    },
    /// A shown block whose event would exceed 256 KiB; fetch it with `ordin.block`.
    BlockAvailable {
        workspace_id: WorkspaceId,
        block_id: BlockId,
    },
    /// The active locale changed (the person's choice or a newer profile). Emitted by the program
    /// from Localisation; `ActiveLocale` restated from Localisation.md (SA-RECON-04).
    LocaleChanged {
        active: ActiveLocale,
    },
    /// A language's wording changed (edited, restored, checked, confirmed, imported, removed, or
    /// drafted by Ordin). Emitted by the program from Localisation's `language_changes()`; payload
    /// restated from Localisation.md (SA-RECON-04; amended January 2026, ADR-109).
    LanguageChanged {
        locale: String,
        change: LanguageChangeKind,
    },
    /// Desktop only: the active brand pack changed (Brand.md's payload).
    BrandChanged {
        brand_identifier: String,
        version: String,
        source: BrandPackSource,
    },
    /// Getting ready progressed (Setup Assistant's payload); reload with `setup.status`.
    SetupProgress {
        progress: SetupProgress,
    },
    /// A sign-in's state changed (Identity and Credentials' payload); reload with `identity.list`.
    /// `credential_kind`, not `kind`: `kind` is the event's own tag (SA-CLOSE-02).
    SignInChanged {
        credential_id: CredentialId,
        credential_kind: CredentialKind,
        state: SignInState,
    },
    /// The workspace's front doors changed (added, removed, a company entry applied or withdrawn,
    /// or a front door's `about` read anew). Reload with `knowledge_agents.list`.
    /// Emitted by the Knowledge Query Client (amended January 2026, ADR-113).
    KnowledgeAgentsChanged {
        workspace_id: WorkspaceId,
    },
    /// Late results arrived from knowledge agents for one recall, after the first deadline and
    /// before the second. Emitted by the Memory Service (amended January 2026, ADR-113).
    MoreFound {
        workspace_id: WorkspaceId,
        recall_id: MemoryRecallId,
        added: u16,
        unanswered: Vec<UnansweredSource>,
    },
}

/// An event refused before it is sent (`SVC_EVENT_INVALID`, `SVC_EVENT_TOO_LARGE`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}")]
pub struct EventError {
    pub code: &'static str,
}

fn invalid() -> EventError {
    EventError {
        code: EVENT_INVALID,
    }
}

/// True for a BCP 47 tag of 2–35 characters (the `locale_changed` and `language_changed` limits).
fn is_locale(tag: &str) -> bool {
    (2..=35).contains(&tag.chars().count())
        && tag.starts_with(|c: char| c.is_ascii_alphabetic())
        && tag
            .split('-')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

impl ServiceEvent {
    pub fn kind(&self) -> EventKind {
        match self {
            ServiceEvent::RunStateChanged { .. } => EventKind::RunStateChanged,
            ServiceEvent::RunProgress { .. } => EventKind::RunProgress,
            ServiceEvent::PlanChanged { .. } => EventKind::PlanChanged,
            ServiceEvent::StepMarked { .. } => EventKind::StepMarked,
            ServiceEvent::StartUpReportReady { .. } => EventKind::StartUpReportReady,
            ServiceEvent::ApprovalRequested { .. } => EventKind::ApprovalRequested,
            ServiceEvent::ApprovalDecided { .. } => EventKind::ApprovalDecided,
            ServiceEvent::CrewRoundChanged { .. } => EventKind::CrewRoundChanged,
            ServiceEvent::BlockShown { .. } => EventKind::BlockShown,
            ServiceEvent::Reset => EventKind::Reset,
            ServiceEvent::ServerStateChanged { .. } => EventKind::ServerStateChanged,
            ServiceEvent::WorkspaceChanged { .. } => EventKind::WorkspaceChanged,
            ServiceEvent::NeedsYouChanged { .. } => EventKind::NeedsYouChanged,
            ServiceEvent::BlockAvailable { .. } => EventKind::BlockAvailable,
            ServiceEvent::LocaleChanged { .. } => EventKind::LocaleChanged,
            ServiceEvent::LanguageChanged { .. } => EventKind::LanguageChanged,
            ServiceEvent::BrandChanged { .. } => EventKind::BrandChanged,
            ServiceEvent::SetupProgress { .. } => EventKind::SetupProgress,
            ServiceEvent::SignInChanged { .. } => EventKind::SignInChanged,
            ServiceEvent::KnowledgeAgentsChanged { .. } => EventKind::KnowledgeAgentsChanged,
            ServiceEvent::MoreFound { .. } => EventKind::MoreFound,
        }
    }

    /// Decodes a Journal record's `data` into the variant `kind` names, then runs `check`. An
    /// unknown kind string → `SVC_EVENT_INVALID`.
    pub fn from_journal(kind: &str, data: Value) -> Result<ServiceEvent, EventError> {
        let parsed = EventKind::parse(kind).ok_or_else(invalid)?;
        // Only the Remote API makes `reset`; a Journal record of that kind is a defect.
        if parsed == EventKind::Reset {
            return Err(invalid());
        }
        let Value::Object(mut map) = data else {
            return Err(invalid());
        };
        map.insert("kind".to_owned(), Value::String(kind.to_owned()));
        let event: ServiceEvent =
            serde_json::from_value(Value::Object(map)).map_err(|_| invalid())?;
        event.check()?;
        Ok(event)
    }

    /// The JSON object of the variant without its `kind` key (what an emitter writes as `data`).
    pub fn to_journal_data(&self) -> Value {
        match serde_json::to_value(self) {
            Ok(Value::Object(mut map)) => {
                map.remove("kind");
                Value::Object(map)
            }
            // A `ServiceEvent` always serialises to an object; an empty object stands in otherwise.
            _ => Value::Object(serde_json::Map::new()),
        }
    }

    /// The validation rules for events (*Validation rules* table and the event table's data limits).
    pub fn check(&self) -> Result<(), EventError> {
        let ok = match self {
            ServiceEvent::RunProgress { progress, .. } => {
                (1..=160).contains(&progress.line.chars().count())
                    && progress
                        .count
                        .is_none_or(|c| c.total.is_none_or(|total| c.done <= total))
            }
            ServiceEvent::StepMarked { position, .. } => (1..=7).contains(position),
            ServiceEvent::PlanChanged { steps, .. } => (3..=7).contains(&steps.len()),
            ServiceEvent::ApprovalDecided { decided_on, .. } => {
                (1..=80).contains(&decided_on.chars().count())
            }
            ServiceEvent::StartUpReportReady { items, .. } => items.len() <= 200,
            ServiceEvent::LocaleChanged { active } => is_locale(&active.locale),
            ServiceEvent::LanguageChanged { locale, .. } => is_locale(locale),
            ServiceEvent::BrandChanged { version, .. } => {
                (1..=64).contains(&version.chars().count())
            }
            ServiceEvent::MoreFound {
                added, unanswered, ..
            } => {
                (1..=50).contains(added)
                    && unanswered.len() <= 8
                    && unanswered
                        .iter()
                        .all(|u| (1..=200).contains(&u.agent.chars().count()))
            }
            _ => true,
        };
        if ok {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

impl EventEnvelope {
    /// Builds the envelope of one Journal record: `api_version` = CURRENT, then `from_journal`.
    pub fn new(
        backend: BackendId,
        seq: u64,
        at: Timestamp,
        kind: &str,
        data: Value,
    ) -> Result<EventEnvelope, EventError> {
        Ok(EventEnvelope {
            api_version: ServiceApiVersion::CURRENT,
            backend,
            seq,
            at,
            event: ServiceEvent::from_journal(kind, data)?,
        })
    }
}

/// The SSE frame or IPC payload of one event, with the 256 KiB rule applied.
#[derive(Debug, Clone)]
pub struct TransportEvent {
    pub seq: u64,
    pub kind: EventKind,
    pub json: String,
}

/// Serialises the envelope; a `block_shown` over 256 KiB becomes `block_available` with the same
/// backend, seq and time; any other kind over 256 KiB → `SVC_EVENT_TOO_LARGE`.
pub fn encode_for_transport(envelope: &EventEnvelope) -> Result<TransportEvent, EventError> {
    let too_large = || EventError {
        code: EVENT_TOO_LARGE,
    };
    let json = serde_json::to_string(envelope).map_err(|_| too_large())?;
    // The data part is the event without the envelope's own four keys (Internal Logic 8).
    let data_len = serde_json::to_vec(&envelope.event)
        .map(|v| v.len())
        .unwrap_or(usize::MAX);
    match &envelope.event {
        // A block's frame is also replaced when the envelope overhead pushes it past the limit.
        ServiceEvent::BlockShown {
            workspace_id,
            block_id,
            ..
        } if data_len > MAX_EVENT_DATA_BYTES || json.len() > MAX_EVENT_DATA_BYTES => {
            let substitute = EventEnvelope {
                api_version: envelope.api_version,
                backend: envelope.backend,
                seq: envelope.seq,
                at: envelope.at,
                event: ServiceEvent::BlockAvailable {
                    workspace_id: *workspace_id,
                    block_id: *block_id,
                },
            };
            let json = serde_json::to_string(&substitute).map_err(|_| too_large())?;
            Ok(TransportEvent {
                seq: envelope.seq,
                kind: EventKind::BlockAvailable,
                json,
            })
        }
        _ if data_len > MAX_EVENT_DATA_BYTES => Err(too_large()),
        event => Ok(TransportEvent {
            seq: envelope.seq,
            kind: event.kind(),
            json,
        }),
    }
}
