//! Internal journal entries, artefacts, tool requests, external actions and start-up reports
//! (JournalAndArtefactStore.md Internal Logic 7.1–7.3, 8, 9, 10, 11).

use ordin_types::domain::places::{Artefact, ArtefactLocation};
use ordin_types::domain::runs::{
    ExternalAction, ExternalActionKind, ExternalActionState, StartupReport, StartupSubject,
    ToolRequest, ToolRequestDecision, ToolRequestState,
};
use ordin_types::num::Sha256Hex;
use ordin_types::paging::PAGE_SIZE;
use ordin_types::text::{check_len, normalise_text};
use ordin_types::time::Timestamp;
use ordin_types::{
    AgentInstanceId, ApprovalId, ArtefactId, Cursor, ExternalActionKey, Page, PlaceId, RunId,
    StartupReportId, ToolRequestId,
};

use crate::codec;
use crate::db::{self, Arg, Conditions, DbRow, Tx};
use crate::error::JournalError;
use crate::journal::Journal;
use crate::types::{
    ArtefactFilter, DiagnosticEntry, EntryFilter, JournalEntry, JournalEntryId, NewArtefact,
    NewExternalAction, NewJournalEntry,
};
use crate::validate;

/// An entry's `data` is at most 65,536 bytes serialised (Internal Logic 7.1).
const MAX_ENTRY_DATA_BYTES: usize = 65_536;
/// `unsettled_external_actions` returns at most 1,000 rows (Internal Logic 9.6).
const MAX_UNSETTLED: i64 = 1_000;

pub(crate) const ENTRY_COLUMNS: &str = "{s:id}, kind, component, {s:run_id}, {s:crew_run_id}, \
    {s:workspace_id}, summary, {s:data}, {t:recorded_at}";

pub(crate) fn entry_from(row: &DbRow) -> Result<JournalEntry, JournalError> {
    Ok(JournalEntry {
        id: JournalEntryId(codec::uuid(&row.text(0)?)?),
        kind: row.text(1)?,
        component: row.text(2)?,
        run_id: codec::opt_uuid(row.opt_text(3)?)?.map(RunId),
        crew_run_id: codec::opt_uuid(row.opt_text(4)?)?.map(ordin_types::CrewRunId),
        workspace_id: codec::opt_uuid(row.opt_text(5)?)?.map(ordin_types::WorkspaceId),
        summary: row.text(6)?,
        data: codec::from_json(&row.text(7)?)?,
        recorded_at: codec::ts(&row.text(8)?)?,
    })
}

pub(crate) const TOOL_REQUEST_COLUMNS: &str =
    "{s:id}, {s:run_id}, tool, arguments_digest, decision, \
    deciding_rule, {s:approval_id}, state, {t:requested_at}";

pub(crate) fn tool_request_from(row: &DbRow) -> Result<ToolRequest, JournalError> {
    Ok(ToolRequest {
        id: ToolRequestId(codec::uuid(&row.text(0)?)?),
        run_id: RunId(codec::uuid(&row.text(1)?)?),
        tool: row.text(2)?,
        arguments_digest: Sha256Hex(row.text(3)?),
        decision: row
            .opt_text(4)?
            .as_deref()
            .map(codec::from_name::<ToolRequestDecision>)
            .transpose()?,
        deciding_rule: row.opt_text(5)?,
        approval_id: codec::opt_uuid(row.opt_text(6)?)?.map(ApprovalId),
        state: codec::from_name(&row.text(7)?)?,
        requested_at: codec::ts(&row.text(8)?)?,
    })
}

pub(crate) const ACTION_COLUMNS: &str =
    "{s:action_key}, {s:run_id}, kind, destination, state, {t:sent_at}, {t:confirmed_at}";

pub(crate) fn action_from(row: &DbRow) -> Result<ExternalAction, JournalError> {
    Ok(ExternalAction {
        action_key: ExternalActionKey(codec::uuid(&row.text(0)?)?),
        run_id: codec::opt_uuid(row.opt_text(1)?)?.map(RunId),
        kind: codec::from_name(&row.text(2)?)?,
        destination: row.text(3)?,
        state: codec::from_name(&row.text(4)?)?,
        sent_at: codec::ts(&row.text(5)?)?,
        confirmed_at: codec::opt_ts(row.opt_text(6)?)?,
    })
}

pub(crate) const STARTUP_COLUMNS: &str = "{s:id}, {s:subject}, {s:enforced_rules}, \
    {s:rules_not_enforced}, {s:unavailable_parts}, {s:available_tools}, \
    {s:adapter_verbs_not_offered}, {t:generated_at}";

pub(crate) fn startup_from(row: &DbRow) -> Result<StartupReport, JournalError> {
    Ok(StartupReport {
        id: StartupReportId(codec::uuid(&row.text(0)?)?),
        subject: codec::from_json(&row.text(1)?)?,
        enforced_rules: codec::from_json(&row.text(2)?)?,
        rules_not_enforced: codec::from_json(&row.text(3)?)?,
        unavailable_parts: codec::from_json(&row.text(4)?)?,
        available_tools: codec::from_json(&row.text(5)?)?,
        adapter_verbs_not_offered: codec::from_json(&row.text(6)?)?,
        generated_at: codec::ts(&row.text(7)?)?,
    })
}

const ARTEFACT_COLUMNS: &str = "{s:id}, {s:location}, path, version, hash, {s:producing_run_id}, \
    media_type, {t:created_at}";

fn artefact_from(row: &DbRow) -> Result<Artefact, JournalError> {
    Ok(Artefact {
        id: ArtefactId(codec::uuid(&row.text(0)?)?),
        location: codec::from_json(&row.text(1)?)?,
        path: row.text(2)?,
        version: codec::u32_of(row.int(3)?)?,
        hash: Sha256Hex(row.text(4)?),
        producing_run_id: codec::opt_uuid(row.opt_text(5)?)?.map(RunId),
        media_type: row.text(6)?,
        created_at: codec::ts(&row.text(7)?)?,
    })
}

/// `agent_folder:<uuid>` or `place:<uuid>` (Data Model, `artefacts.location_key`).
pub(crate) fn location_key(location: &ArtefactLocation) -> String {
    match location {
        ArtefactLocation::AgentFolder {
            agent_instance_id: AgentInstanceId(id),
        } => format!("agent_folder:{id}"),
        ArtefactLocation::Place {
            place_id: PlaceId(id),
        } => format!("place:{id}"),
    }
}

/// `run:<id>`, `crew_run:<id>` or `agent_release:<id>` (Internal Logic 11.2).
pub(crate) fn subject_key(subject: &StartupSubject) -> String {
    match subject {
        StartupSubject::Run { run_id } => format!("run:{run_id}"),
        StartupSubject::CrewRun { crew_run_id } => format!("crew_run:{crew_run_id}"),
        StartupSubject::AgentRelease { agent_release_id } => {
            format!("agent_release:{agent_release_id}")
        }
    }
}

/// The keyset condition of a newest-first page over `(time_column, id_column)`.
pub(crate) fn add_keyset(
    conditions: &mut Conditions,
    cursor: Option<&Cursor>,
    time_column: &str,
    id_column: &str,
    id_marker: &str,
) -> Result<(), JournalError> {
    if let Some(cursor) = cursor {
        let (at, id): (Timestamp, uuid::Uuid) = cursor.decode()?;
        conditions.add(
            &format!(
                "({time_column} < #1{{ts}} OR ({time_column} = #1{{ts}} AND {id_column} < #2{id_marker}))"
            ),
            vec![Arg::Text(at.to_wire()), Arg::Text(id.to_string())],
        );
    }
    Ok(())
}

/// One page of 100 items from `limit + 1` rows; the cursor is the last item's sort key.
pub(crate) fn page_of<T>(
    mut items: Vec<T>,
    key: impl Fn(&T) -> (Timestamp, uuid::Uuid),
) -> Page<T> {
    let next_cursor = if items.len() > PAGE_SIZE {
        items.truncate(PAGE_SIZE);
        items.last().map(|last| Cursor::encode(&key(last)))
    } else {
        None
    };
    Page { items, next_cursor }
}

/// `PAGE_SIZE + 1`, to know whether a next page exists.
pub(crate) fn page_probe() -> i64 {
    i64::try_from(PAGE_SIZE + 1).unwrap_or(101)
}

impl Journal {
    // ---- internal journal entries

    pub async fn append_entry(&self, entry: NewJournalEntry) -> Result<JournalEntry, JournalError> {
        let summary = normalise_text(&entry.summary).map_err(|_| JournalError::EntryInvalid)?;
        let data = serde_json::to_string(&entry.data).map_err(|_| JournalError::EntryInvalid)?;
        let valid = validate::is_kind(&entry.kind)
            && validate::is_component(&entry.component)
            && check_len(&summary, 1, 300, "JRNL_ENTRY_INVALID").is_ok()
            && data.len() <= MAX_ENTRY_DATA_BYTES;
        if !valid {
            return Err(JournalError::EntryInvalid);
        }
        let stored = JournalEntry {
            id: JournalEntryId(uuid::Uuid::now_v7()),
            kind: entry.kind,
            component: entry.component,
            run_id: entry.run_id,
            crew_run_id: entry.crew_run_id,
            workspace_id: entry.workspace_id,
            summary,
            data: entry.data,
            recorded_at: self.now(),
        };
        let _writer = self.inner.write_lock.lock().await;
        db::execute(
            &self.inner.store,
            "INSERT INTO journal_entries (id, kind, component, run_id, crew_run_id, workspace_id, \
             summary, data, recorded_at) \
             VALUES ($1{uuid}, $2, $3, $4{uuid}, $5{uuid}, $6{uuid}, $7, $8{json}, $9{ts})",
            &[
                Arg::Text(stored.id.0.to_string()),
                Arg::Text(stored.kind.clone()),
                Arg::Text(stored.component.clone()),
                Arg::OptText(stored.run_id.map(|r| r.to_string())),
                Arg::OptText(stored.crew_run_id.map(|r| r.to_string())),
                Arg::OptText(stored.workspace_id.map(|r| r.to_string())),
                Arg::Text(stored.summary.clone()),
                Arg::Text(data),
                Arg::Text(stored.recorded_at.to_wire()),
            ],
        )
        .await?;
        Ok(stored)
    }

    pub async fn list_entries(
        &self,
        filter: EntryFilter,
        cursor: Option<Cursor>,
    ) -> Result<Page<JournalEntry>, JournalError> {
        let mut conditions = Conditions::default();
        if let Some(run_id) = filter.run_id {
            conditions.add("run_id = #1{uuid}", vec![Arg::Text(run_id.to_string())]);
        }
        if let Some(kind) = filter.kind {
            conditions.add("kind = #1", vec![Arg::Text(kind)]);
        }
        add_keyset(
            &mut conditions,
            cursor.as_ref(),
            "recorded_at",
            "id",
            "{uuid}",
        )?;
        let sql = format!(
            "SELECT {ENTRY_COLUMNS} FROM journal_entries{} \
             ORDER BY recorded_at DESC, id DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(page_probe()));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let items = rows.iter().map(entry_from).collect::<Result<Vec<_>, _>>()?;
        Ok(page_of(items, |e| (e.recorded_at, e.id.0)))
    }

    /// Newest first; the entries as Diagnostics lines (step 7.3).
    pub async fn diagnostic_lines(
        &self,
        cursor: Option<Cursor>,
    ) -> Result<Page<DiagnosticEntry>, JournalError> {
        let page = self.list_entries(EntryFilter::default(), cursor).await?;
        let items = page
            .items
            .into_iter()
            .map(|entry| {
                let level = match entry.data.get("level").and_then(|v| v.as_str()) {
                    Some(level @ ("info" | "warning" | "error")) => level.to_owned(),
                    _ => "info".to_owned(),
                };
                let code = match entry.data.get("code").and_then(|v| v.as_str()) {
                    Some(code) if validate::is_code(code) => code.to_owned(),
                    _ => String::new(),
                };
                DiagnosticEntry {
                    reference: entry.id.0.to_string().chars().take(8).collect(),
                    at: entry.recorded_at,
                    level,
                    code,
                    summary: entry.summary,
                }
            })
            .collect();
        Ok(Page {
            items,
            next_cursor: page.next_cursor,
        })
    }

    // ---- artefacts (02:677 "the artefact index")

    /// Indexes content that the caller has already written; never writes or reads bytes.
    pub async fn record_artefact(&self, new: NewArtefact) -> Result<Artefact, JournalError> {
        validate::artefact_path(&new.path)?;
        validate::media_type(&new.media_type)?;
        Sha256Hex::parse(&new.hash.0)?;
        let key = location_key(&new.location);
        let _writer = self.inner.write_lock.lock().await;
        let mut tx = Tx::begin(&self.inner.store).await?;
        let latest = tx
            .fetch_optional(
                &format!(
                    "SELECT {ARTEFACT_COLUMNS} FROM artefacts WHERE location_key = $1 AND path = $2 \
                     ORDER BY version DESC LIMIT 1"
                ),
                &[Arg::Text(key.clone()), Arg::Text(new.path.clone())],
            )
            .await?
            .map(|row| artefact_from(&row))
            .transpose()?;
        // Step 8.2: unchanged content gets no new version.
        if let Some(latest) = latest.as_ref().filter(|a| a.hash == new.hash) {
            return Ok(latest.clone());
        }
        let artefact = Artefact {
            id: ArtefactId(uuid::Uuid::now_v7()),
            location: new.location,
            path: new.path,
            version: latest.map_or(1, |a| a.version + 1),
            hash: new.hash,
            producing_run_id: new.producing_run_id,
            media_type: new.media_type,
            created_at: self.now(),
        };
        tx.execute(
            "INSERT INTO artefacts (id, location, location_key, path, version, hash, \
             producing_run_id, media_type, created_at) \
             VALUES ($1{uuid}, $2{json}, $3, $4, $5, $6, $7{uuid}, $8, $9{ts})",
            &[
                Arg::Text(artefact.id.0.to_string()),
                Arg::Text(codec::to_json(&artefact.location)?),
                Arg::Text(key),
                Arg::Text(artefact.path.clone()),
                Arg::Int(i64::from(artefact.version)),
                Arg::Text(artefact.hash.0.clone()),
                Arg::OptText(artefact.producing_run_id.map(|r| r.to_string())),
                Arg::Text(artefact.media_type.clone()),
                Arg::Text(artefact.created_at.to_wire()),
            ],
        )
        .await?;
        tx.commit().await?;
        Ok(artefact)
    }

    /// Every version of one artefact, oldest first.
    pub async fn artefact_versions(
        &self,
        location: ArtefactLocation,
        path: &str,
    ) -> Result<Vec<Artefact>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            &format!(
                "SELECT {ARTEFACT_COLUMNS} FROM artefacts WHERE location_key = $1 AND path = $2 \
                 ORDER BY version"
            ),
            &[
                Arg::Text(location_key(&location)),
                Arg::Text(path.to_owned()),
            ],
        )
        .await?;
        rows.iter().map(artefact_from).collect()
    }

    pub async fn list_artefacts(
        &self,
        filter: ArtefactFilter,
        cursor: Option<Cursor>,
    ) -> Result<Page<Artefact>, JournalError> {
        let mut conditions = Conditions::default();
        if let Some(location) = &filter.location {
            conditions.add("location_key = #1", vec![Arg::Text(location_key(location))]);
        }
        if let Some(run_id) = filter.producing_run_id {
            conditions.add(
                "producing_run_id = #1{uuid}",
                vec![Arg::Text(run_id.to_string())],
            );
        }
        add_keyset(
            &mut conditions,
            cursor.as_ref(),
            "created_at",
            "id",
            "{uuid}",
        )?;
        let sql = format!(
            "SELECT {ARTEFACT_COLUMNS} FROM artefacts{} ORDER BY created_at DESC, id DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(page_probe()));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let items = rows
            .iter()
            .map(artefact_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(page_of(items, |a| (a.created_at, a.id.0)))
    }

    // ---- tool requests (02:674)

    /// The Journal mints the `ToolRequestId`; there is no insert-or-replace.
    pub async fn record_tool_request(
        &self,
        run_id: RunId,
        tool: &str,
        arguments_digest: Sha256Hex,
    ) -> Result<ToolRequest, JournalError> {
        check_len(tool, 1, 200, "TREQ_NAME_INVALID")?;
        Sha256Hex::parse(&arguments_digest.0)?;
        let request = ToolRequest {
            id: ToolRequestId(uuid::Uuid::now_v7()),
            run_id,
            tool: tool.to_owned(),
            arguments_digest,
            decision: None,
            deciding_rule: None,
            approval_id: None,
            state: ToolRequestState::Requested,
            requested_at: self.now(),
        };
        let _writer = self.inner.write_lock.lock().await;
        db::execute(
            &self.inner.store,
            "INSERT INTO tool_requests (id, run_id, tool, arguments_digest, state, requested_at) \
             VALUES ($1{uuid}, $2{uuid}, $3, $4, 'requested', $5{ts})",
            &[
                Arg::Text(request.id.0.to_string()),
                Arg::Text(run_id.to_string()),
                Arg::Text(request.tool.clone()),
                Arg::Text(request.arguments_digest.0.clone()),
                Arg::Text(request.requested_at.to_wire()),
            ],
        )
        .await?;
        Ok(request)
    }

    pub async fn decide_tool_request(
        &self,
        id: ToolRequestId,
        decision: ToolRequestDecision,
        deciding_rule: &str,
        approval_id: Option<ApprovalId>,
    ) -> Result<ToolRequest, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut request = self.tool_request(id).await?.ok_or(JournalError::NotFound)?;
        match request.state {
            ToolRequestState::Requested => {}
            // SPEC-GAP-4: Internal Logic 10.2 names no code for deciding a request that is already
            // `decided`; a second decision changes a decided row, so it is refused as immutable.
            ToolRequestState::Decided | ToolRequestState::Executed | ToolRequestState::Refused => {
                return Err(JournalError::invalid("TREQ_IMMUTABLE"))
            }
        }
        // SPEC-GAP-5: `deciding_rule` 1–128 characters (LW-20) has no code of its own; an empty or
        // over-long rule leaves the decision without a valid deciding rule, `TREQ_UNDECIDED`.
        check_len(deciding_rule, 1, 128, "TREQ_UNDECIDED")?;
        if approval_id.is_some() && decision != ToolRequestDecision::Ask {
            return Err(JournalError::invalid("TREQ_APPROVAL_MISMATCH"));
        }
        db::execute(
            &self.inner.store,
            "UPDATE tool_requests SET decision = $1, deciding_rule = $2, approval_id = $3{uuid}, \
             state = 'decided' WHERE id = $4{uuid}",
            &[
                Arg::Text(codec::name_of(&decision)?),
                Arg::Text(deciding_rule.to_owned()),
                Arg::OptText(approval_id.map(|a| a.to_string())),
                Arg::Text(id.to_string()),
            ],
        )
        .await?;
        request.decision = Some(decision);
        request.deciding_rule = Some(deciding_rule.to_owned());
        request.approval_id = approval_id;
        request.state = ToolRequestState::Decided;
        Ok(request)
    }

    pub async fn finish_tool_request(
        &self,
        id: ToolRequestId,
        executed: bool,
    ) -> Result<ToolRequest, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut request = self.tool_request(id).await?.ok_or(JournalError::NotFound)?;
        match request.state {
            ToolRequestState::Decided => {}
            ToolRequestState::Requested => return Err(JournalError::invalid("TREQ_UNDECIDED")),
            ToolRequestState::Executed | ToolRequestState::Refused => {
                return Err(JournalError::invalid("TREQ_IMMUTABLE"))
            }
        }
        let state = if executed {
            ToolRequestState::Executed
        } else {
            ToolRequestState::Refused
        };
        db::execute(
            &self.inner.store,
            "UPDATE tool_requests SET state = $1 WHERE id = $2{uuid}",
            &[
                Arg::Text(codec::name_of(&state)?),
                Arg::Text(id.to_string()),
            ],
        )
        .await?;
        request.state = state;
        Ok(request)
    }

    pub async fn tool_request(
        &self,
        id: ToolRequestId,
    ) -> Result<Option<ToolRequest>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {TOOL_REQUEST_COLUMNS} FROM tool_requests WHERE id = $1{{uuid}}"),
            &[Arg::Text(id.to_string())],
        )
        .await?
        .map(|row| tool_request_from(&row))
        .transpose()
    }

    // ---- external actions (02:675; G8)

    // spec: ordin-desktop/components/JournalAndArtefactStore#SA-RECON-04
    pub async fn begin_external_action(
        &self,
        new: NewExternalAction,
    ) -> Result<ExternalAction, JournalError> {
        check_len(&new.destination, 1, 2048, "EXT_DESTINATION_INVALID")?;
        let action = ExternalAction {
            action_key: new.action_key,
            run_id: new.run_id,
            kind: new.kind,
            destination: new.destination,
            state: ExternalActionState::Sent,
            sent_at: self.now(),
            confirmed_at: None,
        };
        let _writer = self.inner.write_lock.lock().await;
        let inserted = db::execute(
            &self.inner.store,
            "INSERT INTO external_actions (action_key, run_id, kind, destination, state, sent_at) \
             VALUES ($1{uuid}, $2{uuid}, $3, $4, 'sent', $5{ts})",
            &[
                Arg::Text(action.action_key.to_string()),
                Arg::OptText(action.run_id.map(|r| r.to_string())),
                Arg::Text(codec::name_of(&action.kind)?),
                Arg::Text(action.destination.clone()),
                Arg::Text(action.sent_at.to_wire()),
            ],
        )
        .await;
        match inserted {
            Ok(_) => Ok(action),
            Err(e) if db::is_unique_violation(&e) => {
                Err(JournalError::invalid("EXT_KEY_DUPLICATE"))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Sets `confirmed_at` from the Journal's clock; the caller passes no time.
    pub async fn confirm_external_action(
        &self,
        key: ExternalActionKey,
    ) -> Result<ExternalAction, JournalError> {
        self.settle_external_action(key, &[ExternalActionState::Sent])
            .await
    }

    /// The person's "It arrived — mark as done" on an unknown outcome (03:441).
    pub async fn mark_external_action_arrived(
        &self,
        key: ExternalActionKey,
    ) -> Result<ExternalAction, JournalError> {
        self.settle_external_action(key, &[ExternalActionState::UnknownOutcome])
            .await
    }

    /// `from` → `confirmed` with `confirmed_at = now`; already `confirmed` is returned unchanged
    /// by a confirmation (idempotent); any other state is `JRNL_EXT_STATE_INVALID`.
    async fn settle_external_action(
        &self,
        key: ExternalActionKey,
        from: &[ExternalActionState],
    ) -> Result<ExternalAction, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let mut action = self
            .external_action(key)
            .await?
            .ok_or(JournalError::NotFound)?;
        let idempotent = action.state == ExternalActionState::Confirmed
            && from.contains(&ExternalActionState::Sent);
        if idempotent {
            return Ok(action);
        }
        if !from.contains(&action.state) {
            return Err(JournalError::ExternalActionState);
        }
        let now = self.now();
        db::execute(
            &self.inner.store,
            "UPDATE external_actions SET state = 'confirmed', confirmed_at = $1{ts} \
             WHERE action_key = $2{uuid}",
            &[Arg::Text(now.to_wire()), Arg::Text(key.to_string())],
        )
        .await?;
        action.state = ExternalActionState::Confirmed;
        action.confirmed_at = Some(now);
        Ok(action)
    }

    /// One action by its key; `None` when no row has it (step 9.5).
    pub async fn external_action(
        &self,
        key: ExternalActionKey,
    ) -> Result<Option<ExternalAction>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {ACTION_COLUMNS} FROM external_actions WHERE action_key = $1{{uuid}}"),
            &[Arg::Text(key.to_string())],
        )
        .await?
        .map(|row| action_from(&row))
        .transpose()
    }

    pub async fn external_actions_for_run(
        &self,
        run_id: RunId,
    ) -> Result<Vec<ExternalAction>, JournalError> {
        let rows = db::fetch_all(
            &self.inner.store,
            &format!(
                "SELECT {ACTION_COLUMNS} FROM external_actions WHERE run_id = $1{{uuid}} \
                 ORDER BY sent_at, action_key"
            ),
            &[Arg::Text(run_id.to_string())],
        )
        .await?;
        rows.iter().map(action_from).collect()
    }

    /// Every action of the given kinds still `sent` or `unknown_outcome`, oldest first,
    /// at most 1,000 (step 9.6).
    pub async fn unsettled_external_actions(
        &self,
        kinds: &[ExternalActionKind],
    ) -> Result<Vec<ExternalAction>, JournalError> {
        if kinds.is_empty() {
            return Ok(Vec::new());
        }
        let mut args = kinds
            .iter()
            .map(|k| codec::name_of(k).map(Arg::Text))
            .collect::<Result<Vec<_>, _>>()?;
        let sql = format!(
            "SELECT {ACTION_COLUMNS} FROM external_actions \
             WHERE state IN ('sent', 'unknown_outcome') AND kind IN ({}) \
             ORDER BY sent_at, action_key LIMIT ${}",
            db::placeholders(1, kinds.len(), ""),
            kinds.len() + 1
        );
        args.push(Arg::Int(MAX_UNSETTLED));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        rows.iter().map(action_from).collect()
    }

    /// Newest first, keyset over `(sent_at DESC, action_key DESC)` *(order chosen here; Pagination.md
    /// lists no order for this listing)*.
    pub async fn list_external_actions(
        &self,
        state: Option<ExternalActionState>,
        cursor: Option<Cursor>,
    ) -> Result<Page<ExternalAction>, JournalError> {
        let mut conditions = Conditions::default();
        if let Some(state) = state {
            conditions.add("state = #1", vec![Arg::Text(codec::name_of(&state)?)]);
        }
        add_keyset(
            &mut conditions,
            cursor.as_ref(),
            "sent_at",
            "action_key",
            "{uuid}",
        )?;
        let sql = format!(
            "SELECT {ACTION_COLUMNS} FROM external_actions{} \
             ORDER BY sent_at DESC, action_key DESC LIMIT ${}",
            conditions.sql(),
            conditions.next()
        );
        let mut args = conditions.args;
        args.push(Arg::Int(page_probe()));
        let rows = db::fetch_all(&self.inner.store, &sql, &args).await?;
        let items = rows
            .iter()
            .map(action_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(page_of(items, |a| (a.sent_at, a.action_key.0)))
    }

    /// Called once at start, before any session resumes (step 9).
    pub async fn recover_external_actions(&self) -> Result<u32, JournalError> {
        let _writer = self.inner.write_lock.lock().await;
        let count = db::execute(
            &self.inner.store,
            "UPDATE external_actions SET state = 'unknown_outcome' WHERE state = 'sent'",
            &[],
        )
        .await?;
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        tracing::info!(
            component = "journal",
            count,
            "journal.recovery.unknown_outcomes"
        );
        Ok(count)
    }

    // ---- start-up reports (2C: the Journal holds them; no other table keeps them)

    // spec: ordin-desktop/components/JournalAndArtefactStore#SA-RECON-06
    pub async fn store_startup_report(
        &self,
        report: StartupReport,
        replace: bool,
    ) -> Result<(), JournalError> {
        validate::startup_report(&report)?;
        let key = subject_key(&report.subject);
        let args = [
            Arg::Text(report.id.0.to_string()),
            Arg::Text(codec::to_json(&report.subject)?),
            Arg::Text(key),
            Arg::Text(codec::to_json(&report.enforced_rules)?),
            Arg::Text(codec::to_json(&report.rules_not_enforced)?),
            Arg::Text(codec::to_json(&report.unavailable_parts)?),
            Arg::Text(codec::to_json(&report.available_tools)?),
            Arg::Text(codec::to_json(&report.adapter_verbs_not_offered)?),
            Arg::Text(report.generated_at.to_wire()),
        ];
        let insert = "INSERT INTO startup_reports (id, subject, subject_key, enforced_rules, \
             rules_not_enforced, unavailable_parts, available_tools, adapter_verbs_not_offered, \
             generated_at) VALUES ($1{uuid}, $2{json}, $3, $4{json}, $5{json}, $6{json}, $7{json}, \
             $8{json}, $9{ts})";
        let _writer = self.inner.write_lock.lock().await;
        if replace {
            let mut tx = Tx::begin(&self.inner.store).await?;
            tx.execute(
                "DELETE FROM startup_reports WHERE subject_key = $1",
                &[args[2].clone()],
            )
            .await?;
            tx.execute(insert, &args).await?;
            tx.commit().await?;
            return Ok(());
        }
        match db::execute(&self.inner.store, insert, &args).await {
            Ok(_) => Ok(()),
            Err(e) if db::is_unique_violation(&e) => Err(JournalError::invalid("START_DUPLICATE")),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn startup_report(
        &self,
        subject: StartupSubject,
    ) -> Result<Option<StartupReport>, JournalError> {
        db::fetch_optional(
            &self.inner.store,
            &format!("SELECT {STARTUP_COLUMNS} FROM startup_reports WHERE subject_key = $1"),
            &[Arg::Text(subject_key(&subject))],
        )
        .await?
        .map(|row| startup_from(&row))
        .transpose()
    }
}
