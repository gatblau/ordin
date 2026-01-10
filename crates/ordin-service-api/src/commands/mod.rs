//! Command input and result types (ServiceApi.md *Command input and result types*).
//!
//! The types this spec owns — the program's commands, `workspaces.tree`, the diagnostics types, the
//! servers' types and the shared references — are defined here, at the module root. The owners' types
//! named in the catalogue are restated field for field, with the owner's `serde` attributes and limits,
//! in one submodule per command group (`commands::<module>`, SA-RECON-02): this crate is in build
//! phase 1 and depends only on `ordin-types` (2B-3), so it cannot import a module crate. 2C types are
//! imported from `ordin-types`, never restated; a submodule re-exports each one its catalogue rows name.

use ordin_types::domain::{
    AgentInstance, CrewRunState, Place, RunState, ServerConnectionState, Workspace,
};
use ordin_types::prelude::*;
use ordin_types::wire::ContentRef;
use serde::{Deserialize, Serialize};

use crate::api::{CommandInput, InputRejection};
use crate::wire::ServiceApiVersion;

pub(crate) mod rules;

pub mod agents;
pub mod approval;
pub mod artefacts;
pub mod authoring;
pub mod brand;
pub mod catalogue;
pub mod checker;
pub mod confluence;
pub mod craft;
pub mod crews;
pub mod dashboards;
pub mod dictation;
pub mod files;
pub mod handover;
pub mod history;
pub mod home;
pub mod hosted;
pub mod identity;
pub mod jira;
pub mod journal;
pub mod knowledge;
pub mod knowledge_agents;
pub mod localisation;
pub mod mail;
pub mod memory;
pub mod ordin;
pub mod plans;
pub mod profile;
pub mod projects;
pub mod push;
pub mod remote_agents;
pub mod reports;
pub mod runs;
pub mod search;
pub mod setup;
pub mod sharepoint;
pub mod slack;
pub mod team_knowledge;
pub mod telegram;
pub mod usage;
pub mod voice;
pub mod workflows;
pub mod workspaces;

/// Input limits of this spec's types (*each a constant in `commands::limits`*).
pub mod limits {
    /// `ChangeSetting.key`: 1–128 characters.
    pub const SETTING_KEY_MAX_CHARS: usize = 128;
    /// `ChangeSetting.value`: at most 64 KiB serialised.
    pub const SETTING_VALUE_MAX_BYTES: usize = 64 * 1024;
    /// `ContentRef::PlaceFile.relative_path`: 1–1,024 bytes.
    pub const RELATIVE_PATH_MAX_BYTES: usize = 1024;
    /// `ConnectServer::AddressAndCode.address`: a host name of at most 253 characters.
    pub const HOST_MAX_CHARS: usize = 253;
    /// The lowest and highest port of an address or a pairing line.
    pub const PORT_MIN: u16 = 1025;
    pub const PORT_MAX: u16 = 65535;
    /// `ConnectServer::PairingLine.line`: 1–400 characters.
    pub const PAIRING_LINE_MAX_CHARS: usize = 400;
    /// `SupportExport.run_ids`: at most 20.
    pub const SUPPORT_EXPORT_MAX_RUNS: usize = 20;
    /// Any `Cursor`: at most 1,024 characters.
    pub const CURSOR_MAX_CHARS: usize = 1024;
    /// `files.save_export.text`: at most 8 MiB of UTF-8.
    pub const SAVE_EXPORT_TEXT_MAX_BYTES: usize = 8 * 1024 * 1024;
}

// ---- references ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct WorkspaceRef {
    pub workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ServerRef {
    pub server_connection_id: ServerConnectionId,
}

// ---- backend ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ServiceHello {
    pub api_version: ServiceApiVersion,
    pub backend: BackendId,
    /// "This computer" for the desktop's own core; the server's display name for a server.
    pub backend_name: String,
    pub profile: BackendProfile,
    /// The last sequence number this backend's journal has assigned; 0 when none.
    pub last_seq: u64,
    /// The `CommandMeta.module` names whose commands this backend answers, sorted, no repeats
    /// (amended January 2026, ADR-114). The screens draw no entry whose commands belong to a group
    /// absent from this list.
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackendProfile {
    Desktop,
    PersonalServer,
}

// ---- this program (OrdinDesktop.md) ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AppQuitResult {
    pub stopping_sessions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct AppPickFolderResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SettingsSectionRef {
    pub section: SettingsSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSection {
    SignIns,
    AgentRepositories,
    Email,
    Telegram,
    Servers,
    Dictation,
    HowOrdinTalks,
    UsingYourPlan,
    Notifications,
    History,
    Display,
    Code,
    WhatOrdinRemembers,
    Diagnostics,
    /// The version running, **Check for updates** and **What's new** (amended January 2026, ADR-114).
    About,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SettingsView {
    pub section: SettingsSection,
    pub settings: Vec<SettingView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SettingView {
    /// The 2D-2 / 2D-3 key, for example `usage.plan_mode`.
    pub key: String,
    pub value: serde_json::Value,
    pub set_by: SetBy,
    /// False when the organisation profile locks it ("Set by your company").
    pub person_may_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ChangeSetting {
    pub key: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SaveCopy {
    pub backend: BackendId,
    pub workspace_id: WorkspaceId,
    pub item: ContentRef,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SavedCopy {
    /// The path the person chose; shown only on this computer.
    pub saved_path: String,
}

/// `files.save_export` (amended January 2026, ADR-109): the save dialog, then a local write of `text`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FilesSaveExportInput {
    pub file_name: String,
    pub text: String,
}

/// `app.open_slack_link` (ADR-102): opens a Slack permalink the core produced.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AppOpenSlackLinkInput {
    pub url: String,
}

// ---- composed by the program, or served by the Journal over this spec's types ----

/// `workspaces.tree`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct WorkspaceTree {
    pub workspace: Workspace,
    pub places: Vec<Place>,
    pub agents: Vec<AgentInstance>,
    pub workflows: Vec<WorkflowId>,
    pub dashboards: Vec<DashboardId>,
    pub running: Vec<RunningItem>,
    pub knowledge_pages: u32,
    pub memory_facts: u32,
    pub waiting_for_person: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunningItem {
    Run {
        workspace_id: WorkspaceId,
        run_id: RunId,
        title: String,
        state: RunState,
    },
    CrewRun {
        workspace_id: WorkspaceId,
        crew_run_id: CrewRunId,
        title: String,
        state: CrewRunState,
    },
}

/// `diagnostics.lines`, `diagnostics.support_export`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticLine {
    pub reference: String,
    pub at: Timestamp,
    pub level: String,
    pub code: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct SupportExport {
    pub run_ids: Vec<RunId>,
}

// ---- servers (ServerConnector.md uses these types unchanged) ----

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ServerView {
    pub server_connection_id: ServerConnectionId,
    pub display_name: String,
    pub state: ServerConnectionState,
    /// "Not connected · as of 10:42" (11 §5.31).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<Timestamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendId>,
    /// Present only when `state` is `refused_for_version`: which side must be updated, so the screens
    /// pick one of the two 11 §5.31 sentences (reconciliation row 144; filled by the Server Connector
    /// from `RefusedForVersion { server_older }` and the stored server version).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub older_side: Option<OlderSide>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OlderSide {
    Desktop,
    Server,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectServer {
    AddressAndCode { address: String, code: String },
    PairingLine { line: String },
}

// ---- checks of this spec's input types (the *Input limits* table) ----

/// `diagnostics.lines`: a cursor of at most 1,024 characters.
impl CommandInput for ordin_types::PageRequest {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::cursor(&self.cursor, "/cursor")
    }
}

impl CommandInput for WorkspaceRef {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for ServerRef {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

impl CommandInput for SettingsSectionRef {
    fn check(&mut self) -> Result<(), InputRejection> {
        Ok(())
    }
}

/// True when `key` matches `^[a-z][a-z0-9_]*(\.[a-z0-9_<>]+)*$`.
fn is_setting_key(key: &str) -> bool {
    let mut parts = key.split('.');
    let head_ok = parts.next().is_some_and(|head| {
        let mut chars = head.chars();
        chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    });
    head_ok
        && parts.all(|part| {
            !part.is_empty()
                && part.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '<' | '>')
                })
        })
}

impl CommandInput for ChangeSetting {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::text(&self.key, 1, limits::SETTING_KEY_MAX_CHARS, "/key")?;
        if !is_setting_key(&self.key) {
            return Err(rules::reject("/key"));
        }
        let size = serde_json::to_vec(&self.value)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if size > limits::SETTING_VALUE_MAX_BYTES {
            return Err(rules::reject("/value"));
        }
        Ok(())
    }
}

/// The `ContentRef::PlaceFile.relative_path` row, shared by every input that carries a `ContentRef`.
pub(crate) fn check_content_ref(item: &ContentRef, at: &str) -> Result<(), InputRejection> {
    match item {
        ContentRef::Artefact { .. } => Ok(()),
        ContentRef::PlaceFile { relative_path, .. } => {
            rules::relative_path(relative_path, &format!("{at}/relative_path"))
        }
    }
}

impl CommandInput for SaveCopy {
    fn check(&mut self) -> Result<(), InputRejection> {
        check_content_ref(&self.item, "/item")
    }
}

impl CommandInput for FilesSaveExportInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::file_name(&self.file_name, "/file_name")?;
        rules::bytes(&self.text, 0, limits::SAVE_EXPORT_TEXT_MAX_BYTES, "/text")
    }
}

impl CommandInput for AppOpenSlackLinkInput {
    fn check(&mut self) -> Result<(), InputRejection> {
        // OrdinDesktop.md: refused unless `https://<name>.slack.com/archives/…`.
        let ok = url::Url::parse(&self.url).is_ok_and(|u| {
            u.scheme() == "https"
                && u.host_str().is_some_and(|h| {
                    h.strip_suffix(".slack.com")
                        .is_some_and(|name| !name.is_empty() && !name.contains('.'))
                })
                && u.path().starts_with("/archives/")
        });
        if !ok {
            return Err(rules::reject("/url"));
        }
        Ok(())
    }
}

impl CommandInput for SupportExport {
    fn check(&mut self) -> Result<(), InputRejection> {
        rules::count(
            &self.run_ids,
            0,
            limits::SUPPORT_EXPORT_MAX_RUNS,
            "/run_ids",
        )
    }
}

impl CommandInput for ConnectServer {
    fn check(&mut self) -> Result<(), InputRejection> {
        match self {
            ConnectServer::AddressAndCode { address, code } => {
                if !is_address(address) {
                    return Err(rules::reject("/address"));
                }
                // 8 symbols of the alphabet, with one optional hyphen after the fourth.
                let hyphen_ok = match code.find('-') {
                    None => code.len() == 8,
                    Some(at) => at == 4 && code.len() == 9 && code.matches('-').count() == 1,
                };
                let symbols_ok = code
                    .chars()
                    .filter(|c| *c != '-')
                    .all(|c| crate::pairing::CODE_ALPHABET.contains(c));
                if !(hyphen_ok && symbols_ok) {
                    return Err(rules::reject("/code"));
                }
                Ok(())
            }
            ConnectServer::PairingLine { line } => {
                rules::text(line, 1, limits::PAIRING_LINE_MAX_CHARS, "/line")?;
                crate::pairing::PairingLine::parse(line).map_err(|_| rules::reject("/line"))?;
                Ok(())
            }
        }
    }
}

/// A host name (≤ 253 characters) or IP literal, with an optional `:port` 1025–65535; an IPv6
/// literal with a port is written in brackets.
fn is_address(raw: &str) -> bool {
    if raw.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let (host, port) = if let Some(rest) = raw.strip_prefix('[') {
        match rest.split_once(']') {
            Some((h, tail)) => match tail.strip_prefix(':') {
                Some(p) => (h, Some(p)),
                None if tail.is_empty() => (h, None),
                None => return false,
            },
            None => return false,
        }
    } else {
        match raw.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (raw, None),
        }
    };
    let port_ok = match port {
        None => true,
        Some(p) => p
            .parse::<u16>()
            .is_ok_and(|n| (limits::PORT_MIN..=limits::PORT_MAX).contains(&n)),
    };
    port_ok && host.chars().count() <= limits::HOST_MAX_CHARS && rules::is_host(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_keys() {
        for good in ["usage.plan_mode", "a", "mail.accounts.<id>.port"] {
            assert!(is_setting_key(good), "{good}");
        }
        for bad in ["", "Usage.x", "a..b", "1a", "a.b-c"] {
            assert!(!is_setting_key(bad), "{bad}");
        }
        let mut ok = ChangeSetting {
            key: "usage.plan_mode".into(),
            value: serde_json::json!("pro"),
        };
        assert!(ok.check().is_ok());
        let mut big = ChangeSetting {
            key: "a".into(),
            value: serde_json::json!("x".repeat(70_000)),
        };
        assert_eq!(big.check().unwrap_err().at, "/input/value");
        let mut bad = ChangeSetting {
            key: "A".into(),
            value: serde_json::Value::Null,
        };
        assert_eq!(bad.check().unwrap_err().at, "/input/key");
    }

    #[test]
    fn addresses_and_codes() {
        for good in ["10.0.0.1", "ordin.example.org:8443", "[::1]:8443", "::1"] {
            assert!(is_address(good), "{good}");
        }
        for bad in ["ordin.example.org:80", "[::1", "[::1]x", "a:b", ""] {
            assert!(!is_address(bad), "{bad}");
        }
        let mut ok = ConnectServer::AddressAndCode {
            address: "10.0.0.1:8443".into(),
            code: "K7QM-2XPA".into(),
        };
        assert!(ok.check().is_ok());
        let mut bad_code = ConnectServer::AddressAndCode {
            address: "10.0.0.1".into(),
            code: "K7Q-M2XPA".into(),
        };
        assert_eq!(bad_code.check().unwrap_err().at, "/input/code");
        let mut bad_line = ConnectServer::PairingLine {
            line: "nope".into(),
        };
        assert_eq!(bad_line.check().unwrap_err().at, "/input/line");
    }

    #[test]
    fn program_inputs() {
        let mut export = FilesSaveExportInput {
            file_name: "a/b".into(),
            text: String::new(),
        };
        assert_eq!(export.check().unwrap_err().at, "/input/file_name");
        let mut link = AppOpenSlackLinkInput {
            url: "https://acme.slack.com/archives/C1/p2".into(),
        };
        assert!(link.check().is_ok());
        for bad in [
            "https://slack.com/archives/x",
            "http://acme.slack.com/archives/x",
            "https://acme.slack.com/messages/x",
            "https://a.b.slack.com/archives/x",
        ] {
            let mut link = AppOpenSlackLinkInput { url: bad.into() };
            assert!(link.check().is_err(), "{bad}");
        }
        let mut export_runs = SupportExport {
            run_ids: (0..21).map(|_| RunId::new()).collect(),
        };
        assert!(export_runs.check().is_err());
        let mut copy = SaveCopy {
            backend: BackendId::new(),
            workspace_id: WorkspaceId::new(),
            item: ContentRef::PlaceFile {
                place_id: PlaceId::new(),
                relative_path: "../x".into(),
            },
        };
        assert_eq!(copy.check().unwrap_err().at, "/input/item/relative_path");
        assert!(WorkspaceRef {
            workspace_id: WorkspaceId::new()
        }
        .check()
        .is_ok());
        assert!(ServerRef {
            server_connection_id: ServerConnectionId::new()
        }
        .check()
        .is_ok());
        assert!(SettingsSectionRef {
            section: SettingsSection::About
        }
        .check()
        .is_ok());
    }
}
