//! Email links and the email, Telegram and Slack grants (WorkspaceService.md Internal Logic 9 and
//! 10). The Mail, Telegram and Slack Services enforce them; this service only records them.

use ordin_types::domain::email::{
    EmailAccess, EmailGrant, EmailLink, EmailLinkKind, EmailLinkState, ImapFolderState,
    SlackAccess, SlackGrant, TelegramAccess, TelegramGrant,
};
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, EmailGrantId, EmailLinkId, SlackGrantId, TelegramGrantId, WorkspaceId,
};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::error::WorkspaceError;
use crate::links::check_display_name;
use crate::service::WorkspaceService;
use crate::store::{self, from_text, to_text};
use crate::types::NewEmailLink;

fn valid_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// RFC 5321 enough for a filter: at most 254 characters, one local part of at most 64 characters
/// and a DNS domain.
fn valid_address(address: &str) -> bool {
    if address.is_empty() || address.len() > 254 {
        return false;
    }
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c))
        && valid_dns_name(domain)
}

/// The 2C rules of an email link; returns the lower-cased, de-duplicated addresses and domains.
pub(crate) fn check_email_link(
    link: &NewEmailLink,
) -> Result<(Vec<String>, Vec<String>), WorkspaceError> {
    let mismatch = || WorkspaceError::Invalid("MAIL_LINK_FIELDS_MISMATCH");
    match link.kind {
        EmailLinkKind::FolderOrLabel => {
            let chars = link
                .folder_or_label
                .as_deref()
                .map_or(0, |f| f.chars().count());
            if chars == 0 || chars > 255 || !link.addresses.is_empty() || !link.domains.is_empty() {
                return Err(mismatch());
            }
        }
        EmailLinkKind::PeopleAndDomains => {
            if link.folder_or_label.is_some()
                || (link.addresses.is_empty() && link.domains.is_empty())
            {
                return Err(mismatch());
            }
        }
    }
    if link.addresses.len() > 200 || link.addresses.iter().any(|a| !valid_address(a.trim())) {
        return Err(WorkspaceError::Invalid("MAIL_ADDRESS_INVALID"));
    }
    if link.domains.len() > 100 || link.domains.iter().any(|d| !valid_dns_name(d.trim())) {
        return Err(WorkspaceError::Invalid("MAIL_DOMAIN_INVALID"));
    }
    Ok((normalise(&link.addresses), normalise(&link.domains)))
}

fn normalise(values: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for value in values {
        let lower = value.trim().to_lowercase();
        if !out.contains(&lower) {
            out.push(lower);
        }
    }
    out
}

/// 2C: the IMAP check state of a link holds 32-bit unsigned values (`MAIL_IMAP_STATE_INVALID`). The
/// Mail Service owns that state (C-26) and calls this on what it reads back from its own table.
pub fn check_imap_state(
    folders: &[(String, i64, i64)],
) -> Result<Vec<ImapFolderState>, WorkspaceError> {
    folders
        .iter()
        .map(|(folder, uid_validity, highest_uid)| {
            match (u32::try_from(*uid_validity), u32::try_from(*highest_uid)) {
                (Ok(uid_validity), Ok(highest_uid)) => Ok(ImapFolderState {
                    folder: folder.clone(),
                    uid_validity,
                    highest_uid,
                }),
                _ => Err(WorkspaceError::Invalid("MAIL_IMAP_STATE_INVALID")),
            }
        })
        .collect()
}

fn email_link_row(row: &SqliteRow) -> Result<EmailLink, WorkspaceError> {
    let list = |column: &str| -> Result<Vec<String>, WorkspaceError> {
        serde_json::from_str(&row.try_get::<String, _>(column)?)
            .map_err(|_| WorkspaceError::DatastoreUnavailable)
    };
    Ok(EmailLink {
        id: store::id_of(row, "id")?,
        workspace_id: store::id_of(row, "workspace_id")?,
        kind: from_text(&row.try_get::<String, _>("kind")?)?,
        folder_or_label: row.try_get("folder_or_label")?,
        addresses: list("addresses")?,
        domains: list("domains")?,
        display_name: row.try_get("display_name")?,
        state: from_text(&row.try_get::<String, _>("state")?)?,
        added_at: store::ts_req(row, "added_at")?,
        // The Mail Service owns the check state (2C C-26).
        last_checked_at: None,
        new_since_last_looked: 0,
        imap_folders: Vec::new(),
    })
}

/// The three agent grant tables. Their statements are literals, never built from input.
#[derive(Clone, Copy)]
pub(crate) enum Table {
    Email,
    Telegram,
    Slack,
}

/// `concat!` of a statement around the table's literal name: a `&'static str` per table.
macro_rules! by_table {
    ($table:expr, $before:literal, $after:literal) => {
        match $table {
            $crate::mail::Table::Email => concat!($before, "email_grants", $after),
            $crate::mail::Table::Telegram => concat!($before, "telegram_grants", $after),
            $crate::mail::Table::Slack => concat!($before, "slack_grants", $after),
        }
    };
}
pub(crate) use by_table;

/// One row of an agent grant table.
struct SimpleGrant {
    id: String,
    workspace_id: WorkspaceId,
    agent: AgentInstanceId,
    access: String,
    granted_at: Timestamp,
}

fn simple_row(row: &SqliteRow) -> Result<SimpleGrant, WorkspaceError> {
    Ok(SimpleGrant {
        id: row.try_get("id")?,
        workspace_id: store::id_of(row, "workspace_id")?,
        agent: store::id_of(row, "agent_instance_id")?,
        access: row.try_get("access")?,
        granted_at: store::ts_req(row, "granted_at")?,
    })
}

fn parse_id<T: std::str::FromStr>(text: &str) -> Result<T, WorkspaceError> {
    text.parse::<T>()
        .map_err(|_| WorkspaceError::DatastoreUnavailable)
}

impl WorkspaceService {
    // ---- 9. Email links

    /// Internal Logic 9.1.
    pub async fn propose_email_link(
        &self,
        id: WorkspaceId,
        link: NewEmailLink,
    ) -> Result<EmailLink, WorkspaceError> {
        self.ensure_active(id).await?;
        let (addresses, domains) = check_email_link(&link)?;
        let display_name = check_display_name(&link.display_name)?;
        let new_id = EmailLinkId::new();
        let now = Timestamp::now();
        sqlx::query(
            "INSERT INTO email_links (id, workspace_id, kind, folder_or_label, addresses, domains, display_name, state, added_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'proposed', ?8)",
        )
        .bind(new_id.to_string())
        .bind(id.to_string())
        .bind(to_text(&link.kind))
        .bind(link.folder_or_label.as_deref())
        .bind(serde_json::to_string(&addresses).unwrap_or_default())
        .bind(serde_json::to_string(&domains).unwrap_or_default())
        .bind(&display_name)
        .bind(now.to_wire())
        .execute(&self.pool)
        .await?;
        self.email_link(new_id)
            .await?
            .ok_or(WorkspaceError::LinkNotFound)
    }

    pub async fn confirm_email_link(&self, link: EmailLinkId) -> Result<EmailLink, WorkspaceError> {
        let held = self
            .email_link(link)
            .await?
            .ok_or(WorkspaceError::LinkNotFound)?;
        if held.state != EmailLinkState::Proposed {
            return Err(WorkspaceError::LinkStateInvalid);
        }
        sqlx::query("UPDATE email_links SET state = 'active' WHERE id = ?1")
            .bind(link.to_string())
            .execute(&self.pool)
            .await?;
        // The kind only: no addresses in the journal entry (Internal Logic 9.1).
        self.record(
            "email_link_added",
            Some(held.workspace_id),
            "An email source was added to a workspace.",
            serde_json::json!({ "kind": to_text(&held.kind) }),
        )
        .await;
        self.changed(held.workspace_id).await;
        self.email_link(link)
            .await?
            .ok_or(WorkspaceError::LinkNotFound)
    }

    pub async fn remove_email_link(&self, link: EmailLinkId) -> Result<(), WorkspaceError> {
        let held = self
            .email_link(link)
            .await?
            .ok_or(WorkspaceError::LinkNotFound)?;
        sqlx::query("DELETE FROM email_links WHERE id = ?1")
            .bind(link.to_string())
            .execute(&self.pool)
            .await?;
        self.record(
            "email_link_removed",
            Some(held.workspace_id),
            "An email source was removed from a workspace.",
            serde_json::json!({ "kind": to_text(&held.kind) }),
        )
        .await;
        self.changed(held.workspace_id).await;
        Ok(())
    }

    /// Every email link of the workspace, proposed and active; the check fields come back empty.
    pub async fn email_links(&self, id: WorkspaceId) -> Result<Vec<EmailLink>, WorkspaceError> {
        self.workspace(id).await?;
        let rows =
            sqlx::query("SELECT * FROM email_links WHERE workspace_id = ?1 ORDER BY added_at, id")
                .bind(id.to_string())
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(email_link_row).collect()
    }

    pub async fn email_link(&self, link: EmailLinkId) -> Result<Option<EmailLink>, WorkspaceError> {
        let row = sqlx::query("SELECT * FROM email_links WHERE id = ?1")
            .bind(link.to_string())
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(email_link_row).transpose()
    }

    /// Active workspaces holding at least one active email link.
    pub async fn active_workspaces_with_email(&self) -> Result<Vec<WorkspaceId>, WorkspaceError> {
        let rows = sqlx::query(
            "SELECT DISTINCT l.workspace_id AS workspace_id FROM email_links l \
             JOIN workspaces w ON w.id = l.workspace_id \
             WHERE l.state = 'active' AND w.state = 'active' ORDER BY l.workspace_id",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|r| store::id_of(r, "workspace_id"))
            .collect()
    }

    // ---- the three agent grant tables

    /// Upserts (`Some`) or deletes (`None`) the row of `(workspace, agent)`; the id is kept and
    /// `granted_at` is set on each change.
    async fn set_simple(
        &self,
        table: Table,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        access: Option<String>,
    ) -> Result<Option<SimpleGrant>, WorkspaceError> {
        self.not_in_bin(ws).await?;
        match access {
            None => {
                sqlx::query(by_table!(
                    table,
                    "DELETE FROM ",
                    " WHERE workspace_id = ?1 AND agent_instance_id = ?2"
                ))
                .bind(ws.to_string())
                .bind(agent.to_string())
                .execute(&self.pool)
                .await?;
                Ok(None)
            }
            Some(access) => {
                // The unique key makes a repeat an update, never a duplicate towards the caller.
                match self.insert_simple(table, ws, agent, &access).await {
                    Ok(()) => {}
                    Err(WorkspaceError::Invalid("MAIL_GRANT_DUPLICATE" | "GRANT_DUPLICATE")) => {
                        sqlx::query(by_table!(
                            table,
                            "UPDATE ",
                            " SET access = ?3, granted_at = ?4 WHERE workspace_id = ?1 AND agent_instance_id = ?2"
                        ))
                        .bind(ws.to_string())
                        .bind(agent.to_string())
                        .bind(&access)
                        .bind(Timestamp::now().to_wire())
                        .execute(&self.pool)
                        .await?;
                    }
                    Err(error) => return Err(error),
                }
                self.simple(table, ws, agent).await
            }
        }
    }

    /// Inserts a new row; `MAIL_GRANT_DUPLICATE` (email) or `GRANT_DUPLICATE` when the
    /// `(workspace_id, agent_instance_id)` pair exists.
    pub(crate) async fn insert_simple(
        &self,
        table: Table,
        ws: WorkspaceId,
        agent: AgentInstanceId,
        access: &str,
    ) -> Result<(), WorkspaceError> {
        sqlx::query(by_table!(
            table,
            "INSERT INTO ",
            " (id, workspace_id, agent_instance_id, access, granted_at) VALUES (?1, ?2, ?3, ?4, ?5)"
        ))
        .bind(uuid::Uuid::now_v7().hyphenated().to_string())
        .bind(ws.to_string())
        .bind(agent.to_string())
        .bind(access)
        .bind(Timestamp::now().to_wire())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| {
            if crate::error::is_unique_violation(&error) {
                WorkspaceError::Invalid(if matches!(table, Table::Email) {
                    "MAIL_GRANT_DUPLICATE"
                } else {
                    "GRANT_DUPLICATE"
                })
            } else {
                error.into()
            }
        })
    }

    async fn simple(
        &self,
        table: Table,
        ws: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Option<SimpleGrant>, WorkspaceError> {
        let row = sqlx::query(by_table!(
            table,
            "SELECT * FROM ",
            " WHERE workspace_id = ?1 AND agent_instance_id = ?2"
        ))
        .bind(ws.to_string())
        .bind(agent.to_string())
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref().map(simple_row).transpose()
    }

    async fn simple_list(
        &self,
        table: Table,
        ws: WorkspaceId,
    ) -> Result<Vec<SimpleGrant>, WorkspaceError> {
        self.workspace(ws).await?;
        let rows = sqlx::query(by_table!(
            table,
            "SELECT * FROM ",
            " WHERE workspace_id = ?1 ORDER BY granted_at, id"
        ))
        .bind(ws.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(simple_row).collect()
    }

    async fn announce(&self, kind: &str, ws: WorkspaceId, access: Option<String>) {
        self.record(
            kind,
            Some(ws),
            "A grant was changed in a workspace.",
            serde_json::json!({ "workspace_id": ws, "access": access }),
        )
        .await;
        self.changed(ws).await;
    }

    // ---- email grants (9.3)

    pub async fn set_email_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
        access: Option<EmailAccess>,
    ) -> Result<Option<EmailGrant>, WorkspaceError> {
        let text = access.map(|a| to_text(&a));
        let stored = self
            .set_simple(Table::Email, id, agent, text.clone())
            .await?;
        self.announce("email_grant_set", id, text).await;
        stored.map(email_grant_of).transpose()
    }

    pub async fn email_grants(&self, id: WorkspaceId) -> Result<Vec<EmailGrant>, WorkspaceError> {
        self.simple_list(Table::Email, id)
            .await?
            .into_iter()
            .map(email_grant_of)
            .collect()
    }

    pub async fn email_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Option<EmailGrant>, WorkspaceError> {
        self.simple(Table::Email, id, agent)
            .await?
            .map(email_grant_of)
            .transpose()
    }

    // ---- Telegram grants (10.1)

    pub async fn set_telegram_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
        access: Option<TelegramAccess>,
    ) -> Result<Option<TelegramGrant>, WorkspaceError> {
        let text = access.map(|a| to_text(&a));
        let stored = self
            .set_simple(Table::Telegram, id, agent, text.clone())
            .await?;
        self.announce("telegram_grant_set", id, text).await;
        stored.map(telegram_grant_of).transpose()
    }

    pub async fn telegram_grants(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<TelegramGrant>, WorkspaceError> {
        self.simple_list(Table::Telegram, id)
            .await?
            .into_iter()
            .map(telegram_grant_of)
            .collect()
    }

    pub async fn telegram_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Option<TelegramGrant>, WorkspaceError> {
        self.simple(Table::Telegram, id, agent)
            .await?
            .map(telegram_grant_of)
            .transpose()
    }

    // ---- Slack grants (10.1a)

    pub async fn set_slack_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
        access: Option<SlackAccess>,
    ) -> Result<Option<SlackGrant>, WorkspaceError> {
        let text = access.map(|a| to_text(&a));
        let stored = self
            .set_simple(Table::Slack, id, agent, text.clone())
            .await?;
        self.announce("slack_grant_set", id, text).await;
        stored.map(slack_grant_of).transpose()
    }

    pub async fn slack_grants(&self, id: WorkspaceId) -> Result<Vec<SlackGrant>, WorkspaceError> {
        self.simple_list(Table::Slack, id)
            .await?
            .into_iter()
            .map(slack_grant_of)
            .collect()
    }

    pub async fn slack_grant(
        &self,
        id: WorkspaceId,
        agent: AgentInstanceId,
    ) -> Result<Option<SlackGrant>, WorkspaceError> {
        self.simple(Table::Slack, id, agent)
            .await?
            .map(slack_grant_of)
            .transpose()
    }
}

fn email_grant_of(row: SimpleGrant) -> Result<EmailGrant, WorkspaceError> {
    Ok(EmailGrant {
        id: parse_id::<EmailGrantId>(&row.id)?,
        workspace_id: row.workspace_id,
        agent_instance_id: row.agent,
        access: from_text(&row.access)?,
        granted_at: row.granted_at,
    })
}

fn telegram_grant_of(row: SimpleGrant) -> Result<TelegramGrant, WorkspaceError> {
    Ok(TelegramGrant {
        id: parse_id::<TelegramGrantId>(&row.id)?,
        workspace_id: row.workspace_id,
        agent_instance_id: row.agent,
        access: from_text(&row.access)?,
        granted_at: row.granted_at,
    })
}

fn slack_grant_of(row: SimpleGrant) -> Result<SlackGrant, WorkspaceError> {
    Ok(SlackGrant {
        id: parse_id::<SlackGrantId>(&row.id)?,
        workspace_id: row.workspace_id,
        agent_instance_id: row.agent,
        access: from_text(&row.access)?,
        granted_at: row.granted_at,
    })
}
