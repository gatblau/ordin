//! The two traits this crate owns and the program implements: the agent's sandbox capabilities
//! (`SandboxGrants`, SA-RECON-05) and its place files (`PlaceFiles`, ADR-100 item 1 as amended).

use async_trait::async_trait;
use ordin_types::domain::identity::CredentialKind;
use ordin_types::num::Sha256Hex;
use ordin_types::{AgentInstanceId, CredentialId, PlaceId, WorkspaceId};

/// The company systems an agent instance's sandboxed code may call. Implemented by the program.
// spec: ordin-desktop/components/Sandbox#SA-RECON-05
#[async_trait]
pub trait SandboxGrants: Send + Sync {
    /// The company systems this agent instance's sandboxed code may call in this workspace:
    /// its package's declared connections, narrowed by the organisation profile and the
    /// agent's own tool policy (ADR-090). Empty = no system. Errors fail closed.
    async fn sandbox_capabilities(
        &self,
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<SandboxCapability>, GrantsUnavailable>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxCapability {
    /// The guest's name for it: `api.<name>` / `mcp.<name>`. `^[a-z][a-z0-9_]{0,39}$`; never "ordin_tools".
    pub name: String,
    pub kind: SandboxCapabilityKind,
    /// Write methods (POST, PUT, PATCH, DELETE) allowed; false = GET only.
    pub allow_write: bool,
    /// Private-network destinations (RFC 1918, CGNAT, IPv6 ULA) allowed.
    pub allow_private_network: bool,
    pub credential: SandboxCredential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCapabilityKind {
    Rest {
        host: String,
        /// Regular expressions.
        path_allow: Vec<String>,
        methods: Vec<String>,
    },
    Mcp {
        /// An https origin.
        server_url: String,
        tool_allow: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxCredential {
    /// No credential is sent.
    None,
    /// The person's own token of this kind, forwarded as a bearer token (sandboxd `passthrough`).
    PersonToken {
        credential_id: CredentialId,
        kind: CredentialKind,
    },
    /// A key the person entered once for this agent's connection (SA-CRED-01), applied by the broker.
    AgentKey {
        key_name: String,
        placement: KeyPlacementSpec,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyPlacementSpec {
    Header {
        name: String,
        scheme: Option<String>,
    },
    Query {
        param: String,
    },
}

/// The grants could not be read; the Sandbox fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantsUnavailable;

/// Place reads and agent-folder writes, implemented by the program over the File Service and the
/// Agent Folder Manager.
#[async_trait]
pub trait PlaceFiles: Send + Sync {
    /// Reads one file of a place for this agent instance in this workspace. The implementer
    /// refuses a place the agent has no folder grant for (`NotGranted`), a path outside the
    /// place (`NotFound`), and a file over `max_bytes` (`TooLarge`).
    async fn read_for_agent(
        &self,
        agent_instance_id: AgentInstanceId,
        workspace_id: WorkspaceId,
        place_id: PlaceId,
        relative_path: &str,
        max_bytes: u64,
    ) -> Result<Vec<u8>, PlaceFileError>;

    /// Writes one output file into the agent's own folder at `relative_path` ('/'-separated, no
    /// `..`), replacing an earlier file at that path; returns its SHA-256. `Unreadable` when the
    /// folder cannot be written.
    async fn write_agent_output(
        &self,
        agent_instance_id: AgentInstanceId,
        relative_path: &str,
        bytes: &[u8],
    ) -> Result<Sha256Hex, PlaceFileError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceFileError {
    NotGranted,
    NotFound,
    TooLarge,
    Unreadable,
}
