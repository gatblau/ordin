//! Identifiers: one UUIDv7 newtype per entity (SA-ID-01), and the memory component's two string ids.

use serde::{Deserialize, Deserializer, Serialize};

use crate::TypesError;

/// Declares one UUIDv7 identifier newtype. The serde form is the 36-character lowercase
/// hyphenated string. Deserialising refuses any UUID whose version is not 7.
#[macro_export]
macro_rules! uuid_id {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[cfg_attr(feature = "schema", schemars(transparent))]
        pub struct $name(pub uuid::Uuid);

        impl $name {
            /// Mints a new identifier from the current time (UUIDv7).
            pub fn new() -> Self { Self(uuid::Uuid::now_v7()) }
            /// Parses the 36-character form; refuses anything but a lowercase hyphenated v7 UUID.
            pub fn parse(raw: &str) -> Result<Self, $crate::TypesError> {
                $crate::id::parse_v7(raw).map(Self)
            }
            pub fn as_uuid(&self) -> &uuid::Uuid { &self.0 }
        }
        impl Default for $name { fn default() -> Self { Self::new() } }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0.hyphenated())
            }
        }
        impl std::str::FromStr for $name {
            type Err = $crate::TypesError;
            fn from_str(s: &str) -> Result<Self, Self::Err> { Self::parse(s) }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(&self.0.hyphenated())
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
                Self::parse(&raw).map_err(serde::de::Error::custom)
            }
        }
    )*};
}

// The domain part's identifiers (2C part 1 conventions block, in that order, with its amendments) and
// the wire part's four (2C wire §0).
// SPEC-GAP-1: SharedTypes.md's copy of this list omits `KnowledgeSuggestionId`, the six Slack ids
// (ADR-102), `TeamShareId` (ADR-103) and `TeamFolderId` (ADR-110), which the 2C domain list it cites
// holds and the 2C entities use. The 2C list is compiled here, since SharedTypes.md names it as the source.
uuid_id!(
    BrandPackId,
    HarnessAttachmentId,
    OrganisationProfileId,
    AgentInstanceId,
    ExecutablePartDecisionId,
    PlaceId,
    FolderGrantId,
    WorkspaceId,
    ChangedMarkId,
    SourceLinkId,
    WorkingCopyItemId,
    EmailLinkId,
    EmailGrantId,
    EmailDraftId,
    SentEmailId,
    PendingPushId,
    MemoryFactId,
    KnowledgePageId,
    KnowledgeSuggestionId,
    ConfluenceLinkId,
    PublishedCopyId,
    FileVersionId,
    ProjectId,
    GithubOperationSettingId,
    LanguagePluginId,
    CodingTaskId,
    AttemptGroupId,
    ChangeSetId,
    ChangeSetEntryId,
    CraftInstallationId,
    FileTypeMapEntryId,
    RemoteAgentId,
    TaskId,
    RunId,
    ReusedAnswerId,
    ToolRequestId,
    ApprovalId,
    ExternalActionKey,
    ArtefactId,
    CrewCharterId,
    CrewRunId,
    WorkflowId,
    ReportId,
    DashboardId,
    RunStepId,
    StartupReportId,
    CredentialId,
    ChannelPairingId,
    DesktopPairingId,
    ServerConnectionId,
    CardReferenceId,
    TriggerId,
    TriggerEventId,
    TelegramLinkId,
    KeptTelegramMessageId,
    TelegramDraftId,
    AgentRepositoryId,
    AgentDraftId,
    RehearsalId,
    PartId,
    PartDraftId,
    WithdrawnListEntryId,
    LifecycleId,
    WorkspaceStageId,
    HandoverId,
    HandoverPlaceId,
    VocabularyTermId,
    ServerProfileId,
    AgentReleaseId,
    RunRecordId,
    ScopeId,
    TelegramPostRequestId,
    TelegramGrantId, // amended January 2026, decision 80: C-25
    SlackLinkId,
    KeptSlackMessageId,
    SlackDraftId,
    SlackRuleId,
    SlackPostId,
    SlackGrantId,   // amended January 2026 (ADR-102)
    TeamShareId,    // amended January 2026 (ADR-103)
    TeamFolderId,   // amended January 2026 (ADR-110)
    MemoryRecallId, // amended January 2026 (ADR-111)
    KnowledgeConnectionId,
    KnowledgeAnswerId, // amended January 2026 (ADR-113)
    BackendId,
    CommandRequestId,
    HookCallId,
    BlockId,
);

/// The length of the hyphenated UUID form.
const UUID_CHARS: usize = 36;

/// Shared parser behind every identifier (Internal Logic 1).
pub fn parse_v7(raw: &str) -> Result<uuid::Uuid, TypesError> {
    // (a) the exact length of the hyphenated form.
    if raw.len() != UUID_CHARS {
        return Err(TypesError::IdNotUuidV7);
    }
    // (b) lowercase is the only accepted form.
    if raw.bytes().any(|b| b.is_ascii_uppercase()) {
        return Err(TypesError::IdNotUuidV7);
    }
    // (c) the parse itself, then (d) the version.
    let id = uuid::Uuid::try_parse(raw).map_err(|_| TypesError::IdNotUuidV7)?;
    if id.get_version_num() != 7 {
        return Err(TypesError::IdNotUuidV7);
    }
    Ok(id)
}

/// The code both memory reference ids are refused with (amended January 2026, ADR-111).
const MEMORY_REF_INVALID: &str = "ID_MEMORY_REF_INVALID";

/// True when `raw` is `prefix` followed by a lowercase hyphenated UUID of any version.
fn is_memory_ref(raw: &str, prefix: &str) -> bool {
    let Some(rest) = raw.strip_prefix(prefix) else {
        return false;
    };
    rest.len() == UUID_CHARS
        && rest.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
}

/// The memory component's entity id, shown to the person as a "thing" (MemoryService.md, amendment January 2026).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct MemoryThingId(pub String); // ^entity:[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$

/// The memory component's relationship id, shown as a "link".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct MemoryLinkId(pub String); // ^relationship:<the same UUID form>$

impl MemoryThingId {
    /// `entity:` then a lowercase hyphenated UUID of any version, else `ID_MEMORY_REF_INVALID`.
    pub fn parse(raw: &str) -> Result<Self, TypesError> {
        if is_memory_ref(raw, "entity:") {
            Ok(Self(raw.to_owned()))
        } else {
            Err(TypesError::Rule {
                code: MEMORY_REF_INVALID,
            })
        }
    }
}

impl MemoryLinkId {
    /// `relationship:` then a lowercase hyphenated UUID of any version, else `ID_MEMORY_REF_INVALID`.
    pub fn parse(raw: &str) -> Result<Self, TypesError> {
        if is_memory_ref(raw, "relationship:") {
            Ok(Self(raw.to_owned()))
        } else {
            Err(TypesError::Rule {
                code: MEMORY_REF_INVALID,
            })
        }
    }
}

impl<'de> Deserialize<'de> for MemoryThingId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for MemoryLinkId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}
