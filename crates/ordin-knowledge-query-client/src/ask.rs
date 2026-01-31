//! `ask`: validation (Internal Logic G), the parallel fan-out under one deadline with a backup
//! request (H), and the batches the Memory Service and the Knowledge Query Server read.

use std::sync::Arc;
use std::time::{Duration, Instant as StdInstant};

use futures::StreamExt;
use ordin_types::domain::agent_profile::KnowledgeProxy;
use ordin_types::domain::identity::CredentialKind;
use ordin_types::wire::knowledge_query::{
    KnowledgeHit, KnowledgeResult, UnansweredReason, UnansweredSource,
};
use ordin_types::{KnowledgeAnswerId, KnowledgeConnectionId};
use secrecy::SecretString;
use serde_json::{json, Map, Value};
use sse_stream::SseByteStream;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};
use tokio_util::sync::CancellationToken;

use crate::client::{code_of, Inner, KnowledgeQueryClient, Mode, COMPONENT};
use crate::error::KqError;
use crate::merge::merge_hits;
use crate::store;
use crate::transport::ServiceTokenFailure;
use crate::types::*;

/// What one member (a connection or a proxy) is, and where it is asked.
#[derive(Clone)]
struct Member {
    agent: String,
    base_url: String,
    kind: MemberKind,
}

#[derive(Clone)]
enum MemberKind {
    Connection {
        id: KnowledgeConnectionId,
        audience: String,
        sharepoint_scope: Option<String>,
    },
    Proxy(KnowledgeProxy),
}

/// One expected answer: a primary and, on the agent profile, an optional backup covering the same
/// repositories.
struct Slot {
    primary: usize,
    backup: Option<usize>,
}

/// The fields of `KnowledgeAsk` that leave the computer, plus the two deadlines.
struct Plan {
    request_id: uuid::Uuid,
    query: Option<String>,
    searches: Option<Vec<ordin_types::wire::knowledge_query::KnowledgeSearch>>,
    limit: u8,
    access_group: Option<String>,
    hop_budget: u8,
    visited: Vec<String>,
    first_deadline: Instant,
    more_until: Instant,
    asker: Option<AskerStatement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fail {
    Refused,
    Unreachable,
}

enum Terminal {
    Ended,
    Failed(Fail),
    Dropped,
    Cancelled,
}

enum Msg {
    Result {
        member: usize,
        event: KnowledgeResult,
    },
    More {
        member: usize,
        event: MoreFoundEvent,
    },
    Done {
        member: usize,
        event: DoneEvent,
    },
    Failed {
        member: usize,
        reason: Fail,
    },
    Dropped {
        member: usize,
    },
    Ended {
        member: usize,
    },
}

#[derive(Default)]
struct MemberState {
    started: bool,
    ended: bool,
    answered: bool,
    cancelled: bool,
    dropped: bool,
    failure: Option<Fail>,
    cancel: Option<CancellationToken>,
}

impl MemberState {
    fn terminal(&self) -> bool {
        self.ended || self.cancelled || self.dropped || self.failure.is_some()
    }
}

fn reason_rank(reason: UnansweredReason) -> u8 {
    match reason {
        UnansweredReason::NotInTime => 0,
        UnansweredReason::Refused => 1,
        UnansweredReason::Unreachable => 2,
    }
}

fn validate(ask: &KnowledgeAsk) -> Result<(), KqError> {
    let chars = |s: &str| s.chars().count();
    match (&ask.query, &ask.searches) {
        (Some(query), None) if (1..=4_096).contains(&chars(query)) => {}
        (None, Some(searches))
            if (1..=4).contains(&searches.len())
                && searches
                    .iter()
                    .all(|s| (1..=500).contains(&chars(&s.query))) => {}
        _ => return Err(KqError::BadQuery),
    }
    if !(1..=50).contains(&ask.limit)
        || ask.hop_budget > MAX_HOP_BUDGET
        || ask.visited.len() > MAX_VISITED
        || ask.visited.iter().any(|v| !(1..=80).contains(&chars(v)))
    {
        return Err(KqError::BadQuery);
    }
    Ok(())
}

impl KnowledgeQueryClient {
    /// Asks every target in parallel and returns the stream of batches (Internal Logic G and H).
    pub async fn ask(&self, ask: KnowledgeAsk) -> Result<KnowledgeAnswerStream, KqError> {
        validate(&ask)?;
        let inner = &self.inner;
        let (members, slots) = inner.resolve_targets(&ask).await?;
        let plan = Plan {
            request_id: ask.request_id,
            query: ask.query,
            searches: ask.searches,
            limit: ask.limit,
            access_group: ask.access_group,
            hop_budget: ask.hop_budget,
            visited: ask.visited,
            first_deadline: ask.first_deadline,
            more_until: ask.more_until.max(ask.first_deadline),
            asker: ask.asker,
        };
        let (tx, rx) = mpsc::channel(32);
        let cancel = CancellationToken::new();
        if members.is_empty() {
            let _ = tx.try_send(KnowledgeBatch::empty_final());
            return Ok(KnowledgeAnswerStream {
                rx,
                done: false,
                _guard: cancel.drop_guard(),
            });
        }
        let driver = Driver::new(Arc::clone(inner), plan, members, slots, tx, cancel.clone());
        tokio::spawn(driver.run());
        Ok(KnowledgeAnswerStream {
            rx,
            done: false,
            _guard: cancel.drop_guard(),
        })
    }
}

impl Inner {
    /// Internal Logic G.2.
    async fn resolve_targets(
        &self,
        ask: &KnowledgeAsk,
    ) -> Result<(Vec<Member>, Vec<Slot>), KqError> {
        let mut members: Vec<Member> = Vec::new();
        let mut slots: Vec<Slot> = Vec::new();
        match &ask.target {
            AskTarget::Workspace(ws) => {
                if !matches!(self.mode, Mode::Person { .. }) {
                    return Err(KqError::NoConnections);
                }
                let pool = self.pool()?;
                let rows = store::live_of(&pool, *ws).await?;
                for row in rows.into_iter().filter(|r| {
                    matches!(
                        r.state,
                        ConnectionState::Ready | ConnectionState::Unreachable
                    )
                }) {
                    let scope = self.about_cache.lock().ok().and_then(|c| {
                        c.get(&row.id)
                            .and_then(|e| e.sharepoint_check_scope.clone())
                    });
                    slots.push(Slot {
                        primary: members.len(),
                        backup: None,
                    });
                    members.push(Member {
                        agent: row.name,
                        base_url: row.url,
                        kind: MemberKind::Connection {
                            id: row.id,
                            audience: row.token_audience,
                            sharepoint_scope: scope,
                        },
                    });
                }
                if members.is_empty() {
                    return Err(KqError::NoConnections);
                }
            }
            AskTarget::Proxies(list) => {
                let Mode::Agent { proxies, .. } = &self.mode else {
                    return Err(KqError::NotSupported);
                };
                let find = |name: &str| proxies.iter().find(|p| p.name == name).cloned();
                for pair in list {
                    let primary = find(&pair.primary).ok_or(KqError::UnknownProxy)?;
                    let backup = match &pair.backup {
                        Some(name) => Some(find(name).ok_or(KqError::UnknownProxy)?),
                        None => None,
                    };
                    // A visited agent has already answered this request: skipped, reported nowhere.
                    if ask.visited.contains(&primary.name) {
                        continue;
                    }
                    let backup = backup.filter(|b| !ask.visited.contains(&b.name));
                    let primary_at = members.len();
                    members.push(proxy_member(primary));
                    let backup_at = backup.map(|b| {
                        members.push(proxy_member(b));
                        members.len() - 1
                    });
                    slots.push(Slot {
                        primary: primary_at,
                        backup: backup_at,
                    });
                }
            }
        }
        Ok((members, slots))
    }

    async fn mark_answered(&self, id: KnowledgeConnectionId) {
        if let Ok(pool) = self.pool() {
            let _ = store::set_answered(&pool, id, ordin_types::time::Timestamp::now()).await;
        }
    }

    async fn mark_state(&self, id: KnowledgeConnectionId, state: ConnectionState) {
        if let Ok(pool) = self.pool() {
            let _ = store::set_state(&pool, id, state, None).await;
        }
    }
}

fn proxy_member(proxy: KnowledgeProxy) -> Member {
    Member {
        agent: proxy.name.clone(),
        base_url: proxy.url.trim_end_matches('/').to_owned(),
        kind: MemberKind::Proxy(proxy),
    }
}

/// Step G.3: the body is built from `ask`'s fields only. `deadline_ms` is the time left to the first
/// deadline less the network allowance, clamped to 100..=5,000; `more_until_ms` likewise to
/// `deadline_ms..=10,000`.
fn build_body(
    plan: &Plan,
    allowance: Duration,
    delegation: Option<Delegation>,
    send_asker: bool,
) -> String {
    let now = Instant::now();
    let left = |until: Instant| {
        plan.first_deadline
            .max(until)
            .checked_duration_since(now)
            .unwrap_or_default()
    };
    let deadline_ms = (plan
        .first_deadline
        .checked_duration_since(now)
        .unwrap_or_default()
        .saturating_sub(allowance)
        .as_millis() as u64)
        .clamp(100, 5_000);
    let more_ms = (left(plan.more_until).saturating_sub(allowance).as_millis() as u64)
        .clamp(deadline_ms, 10_000);
    let mut body = Map::new();
    body.insert("request_id".into(), json!(plan.request_id));
    if let Some(query) = &plan.query {
        body.insert("query".into(), json!(query));
    }
    if let Some(searches) = &plan.searches {
        body.insert("searches".into(), json!(searches));
    }
    body.insert("limit".into(), json!(plan.limit));
    if let Some(group) = &plan.access_group {
        body.insert("access_group".into(), json!(group));
    }
    body.insert("hop_budget".into(), json!(plan.hop_budget));
    body.insert("visited".into(), json!(plan.visited));
    body.insert("deadline_ms".into(), json!(deadline_ms));
    body.insert("more_until_ms".into(), json!(more_ms));
    if send_asker {
        if let Some(asker) = &plan.asker {
            body.insert("asker".into(), json!(asker));
        }
    }
    if let Some(delegation) = delegation {
        body.insert("delegation".into(), json!(delegation));
    }
    Value::Object(body).to_string()
}

/// Everything one member's task needs.
struct TaskCtx {
    inner: Arc<Inner>,
    plan: Arc<Plan>,
    member: Member,
    index: usize,
    tx: mpsc::Sender<Msg>,
    cancel: CancellationToken,
}

impl TaskCtx {
    async fn run(self) {
        let terminal = self.attempt().await;
        let member = self.index;
        let msg = match terminal {
            Terminal::Ended => Msg::Ended { member },
            Terminal::Failed(reason) => Msg::Failed { member, reason },
            Terminal::Dropped => Msg::Dropped { member },
            Terminal::Cancelled => return,
        };
        let _ = self.tx.send(msg).await;
    }

    /// The bearer token for this member, or why there is none (H.1).
    async fn token(&self) -> Result<String, Fail> {
        let inner = &self.inner;
        match (&self.member.kind, &inner.mode) {
            (MemberKind::Connection { id, audience, .. }, Mode::Person { sign_in }) => {
                match inner.person_token(sign_in, audience).await {
                    Ok(token) => Ok(token),
                    Err(KqError::SignInNeeded) => {
                        inner.mark_state(*id, ConnectionState::SignInNeeded).await;
                        Err(Fail::Refused)
                    }
                    Err(_) => Err(Fail::Unreachable),
                }
            }
            (
                MemberKind::Proxy(proxy),
                Mode::Agent {
                    service: Some(service),
                    ..
                },
            ) => match service.token_for(&inner.transport, proxy).await {
                Ok(token) => Ok(token.expose().to_owned()),
                Err(ServiceTokenFailure::Refused) => {
                    tracing::error!(
                        component = COMPONENT,
                        agent = proxy.name.as_str(),
                        "kq_service_token_refused"
                    );
                    Err(Fail::Refused)
                }
                Err(ServiceTokenFailure::Unreachable) => Err(Fail::Unreachable),
            },
            _ => Err(Fail::Unreachable),
        }
    }

    async fn invalidate(&self) {
        let inner = &self.inner;
        match (&self.member.kind, &inner.mode) {
            (MemberKind::Connection { audience, .. }, Mode::Person { sign_in }) => {
                if let Ok(tokens) = inner.token_source() {
                    tokens
                        .invalidate_audience(&sign_in.issuer, &sign_in.client_id, audience)
                        .await;
                }
            }
            (
                MemberKind::Proxy(proxy),
                Mode::Agent {
                    service: Some(service),
                    ..
                },
            ) => {
                service.invalidate(&proxy.name).await;
            }
            _ => {}
        }
    }

    /// PD-B: a short-lived Microsoft token for this front door's app, only when its `about` names a
    /// scope; any error means no delegation is sent.
    async fn delegation(&self) -> Option<Delegation> {
        let MemberKind::Connection {
            id,
            sharepoint_scope: Some(scope),
            ..
        } = &self.member.kind
        else {
            return None;
        };
        let tokens = self.inner.token_source().ok()?;
        match tokens.scope_token(CredentialKind::Microsoft, scope).await {
            Ok(token) => Some(Delegation {
                sharepoint_assertion: SecretString::from(token.token.expose()),
            }),
            Err(error) => {
                tracing::debug!(
                    component = COMPONENT,
                    connection_id = %id,
                    code = ordin_types::CodedError::code(&error),
                    "kq_delegation_unavailable"
                );
                None
            }
        }
    }

    async fn fail(&self, reason: Fail, state: Option<ConnectionState>) -> Terminal {
        if let (MemberKind::Connection { id, .. }, Some(state)) = (&self.member.kind, state) {
            self.inner.mark_state(*id, state).await;
        }
        Terminal::Failed(reason)
    }

    async fn attempt(&self) -> Terminal {
        let mut retried = false;
        loop {
            let token = tokio::select! {
                _ = self.cancel.cancelled() => return Terminal::Cancelled,
                token = self.token() => match token {
                    Ok(token) => token,
                    Err(reason) => return Terminal::Failed(reason),
                },
            };
            let delegation = self.delegation().await;
            let send_asker = matches!(self.inner.mode, Mode::Agent { .. });
            let body = build_body(
                &self.plan,
                self.inner.timings.network_allowance,
                delegation,
                send_asker,
            );
            let url = format!("{}/v1/knowledge/query", self.member.base_url);
            let allow = self
                .plan
                .more_until
                .checked_duration_since(Instant::now())
                .unwrap_or_default()
                + Duration::from_secs(1);
            let request = self
                .inner
                .transport
                .client(&self.member.base_url)
                .post(self.inner.transport.address(url))
                .bearer_auth(token)
                .header(reqwest::header::ACCEPT, "text/event-stream")
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .timeout(allow)
                .body(body);
            let response = tokio::select! {
                _ = self.cancel.cancelled() => return Terminal::Cancelled,
                response = request.send() => response,
            };
            let Ok(response) = response else {
                return self
                    .fail(Fail::Unreachable, Some(ConnectionState::Unreachable))
                    .await;
            };
            let status = response.status().as_u16();
            match status {
                200..=299 => return self.read_events(response).await,
                401 if !retried => {
                    retried = true;
                    self.invalidate().await;
                }
                401 => return self.fail(Fail::Refused, None).await,
                403 => {
                    return self
                        .fail(Fail::Refused, Some(ConnectionState::NotInAudience))
                        .await
                }
                409 => {
                    let bytes = response.bytes().await.unwrap_or_default();
                    return if code_of(&bytes) == Some("KQ_LOOP") {
                        Terminal::Dropped
                    } else {
                        Terminal::Failed(Fail::Unreachable)
                    };
                }
                _ => {
                    return self
                        .fail(Fail::Unreachable, Some(ConnectionState::Unreachable))
                        .await
                }
            }
        }
    }

    /// H.2: the events of one agent, each sent to the driver tagged with the member.
    async fn read_events(&self, response: reqwest::Response) -> Terminal {
        let mut stream = Box::pin(SseByteStream::new(response.bytes_stream()));
        let mut answer_id: Option<KnowledgeAnswerId> = None;
        let agent = self.member.agent.as_str();
        loop {
            let next = tokio::select! {
                _ = self.cancel.cancelled() => return Terminal::Cancelled,
                next = stream.next() => next,
            };
            let event = match next {
                None => return Terminal::Ended,
                Some(Err(_)) => return Terminal::Failed(Fail::Unreachable),
                Some(Ok(event)) => event,
            };
            let data = event.data.as_deref().unwrap_or_default();
            let msg = match event.event.as_deref() {
                Some("result") => match serde_json::from_str::<KnowledgeResult>(data) {
                    Ok(mut parsed) => {
                        answer_id = parsed.answer_id;
                        stamp(&mut parsed.hits, answer_id);
                        Msg::Result {
                            member: self.index,
                            event: parsed,
                        }
                    }
                    Err(_) => return self.bad_event(agent),
                },
                Some("more_found") => match serde_json::from_str::<MoreFoundEvent>(data) {
                    Ok(mut parsed) => {
                        stamp(&mut parsed.hits, answer_id);
                        Msg::More {
                            member: self.index,
                            event: parsed,
                        }
                    }
                    Err(_) => return self.bad_event(agent),
                },
                Some("done") => match serde_json::from_str::<DoneEvent>(data) {
                    Ok(parsed) => Msg::Done {
                        member: self.index,
                        event: parsed,
                    },
                    Err(_) => return self.bad_event(agent),
                },
                // Any other event name is ignored.
                _ => continue,
            };
            if matches!(msg, Msg::Result { .. }) {
                if let MemberKind::Connection { id, .. } = &self.member.kind {
                    self.inner.mark_answered(*id).await;
                }
            }
            if self.tx.send(msg).await.is_err() {
                return Terminal::Cancelled;
            }
        }
    }

    fn bad_event(&self, agent: &str) -> Terminal {
        tracing::warn!(component = COMPONENT, agent, "kq_bad_event");
        Terminal::Failed(Fail::Unreachable)
    }
}

/// Every hit gets the answer id of the agent this client asked; any value the hit carried is
/// replaced (SA-KQC-08).
// spec: ordin-desktop/components/KnowledgeQueryClient#SA-KQC-08
fn stamp(hits: &mut [KnowledgeHit], answer_id: Option<KnowledgeAnswerId>) {
    for hit in hits {
        hit.answer_id = answer_id;
    }
}

/// The task that owns the receiver, the state of every member, and the deadlines.
struct Driver {
    inner: Arc<Inner>,
    plan: Arc<Plan>,
    members: Vec<Member>,
    slots: Vec<Slot>,
    states: Vec<MemberState>,
    lists: Vec<(usize, String, Vec<KnowledgeHit>)>,
    reported: Vec<UnansweredSource>,
    pending: Vec<(String, PendingHit)>,
    unchecked: bool,
    out: mpsc::Sender<KnowledgeBatch>,
    cancel: CancellationToken,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    started_at: StdInstant,
}

impl Driver {
    fn new(
        inner: Arc<Inner>,
        plan: Plan,
        members: Vec<Member>,
        slots: Vec<Slot>,
        out: mpsc::Sender<KnowledgeBatch>,
        cancel: CancellationToken,
    ) -> Driver {
        let (tx, rx) = mpsc::channel(64);
        let states = members.iter().map(|_| MemberState::default()).collect();
        Driver {
            inner,
            plan: Arc::new(plan),
            members,
            slots,
            states,
            lists: Vec::new(),
            reported: Vec::new(),
            pending: Vec::new(),
            unchecked: false,
            out,
            cancel,
            tx,
            rx,
            started_at: StdInstant::now(),
        }
    }

    fn start(&mut self, index: usize) {
        let token = self.cancel.child_token();
        self.states[index].started = true;
        self.states[index].cancel = Some(token.clone());
        let ctx = TaskCtx {
            inner: Arc::clone(&self.inner),
            plan: Arc::clone(&self.plan),
            member: self.members[index].clone(),
            index,
            tx: self.tx.clone(),
            cancel: token,
        };
        tokio::spawn(ctx.run());
    }

    fn slot_answered(&self, slot: &Slot) -> bool {
        self.states[slot.primary].answered || slot.backup.is_some_and(|b| self.states[b].answered)
    }

    /// A slot is resolved once it has an answer, or every member of it has been tried and failed.
    fn slot_resolved(&self, slot: &Slot) -> bool {
        if self.slot_answered(slot) {
            return true;
        }
        let tried = |i: usize| self.states[i].started && self.states[i].terminal();
        tried(slot.primary) && slot.backup.is_none_or(tried)
    }

    fn all_resolved(&self) -> bool {
        self.slots.iter().all(|s| self.slot_resolved(s))
    }

    /// No task is open and none is still to start.
    fn all_ended(&self) -> bool {
        self.slots.iter().all(|slot| {
            let answered = self.slot_answered(slot);
            let done = |i: usize| {
                let state = &self.states[i];
                state.terminal() || (!state.started && answered)
            };
            done(slot.primary) && slot.backup.is_none_or(done)
        })
    }

    fn hedge_due(&self) -> bool {
        self.slots
            .iter()
            .any(|s| s.backup.is_some_and(|b| !self.states[b].started) && !self.slot_answered(s))
    }

    /// H.3: the primary sent no `result` within the hedge delay, so each such slot's backup starts.
    fn start_backups(&mut self) {
        if !self.hedge_due() {
            return;
        }
        let backups: Vec<usize> = self
            .slots
            .iter()
            .filter(|s| !self.slot_answered(s))
            .filter_map(|s| s.backup)
            .collect();
        for backup in backups {
            self.start(backup);
        }
    }

    fn handle(&mut self, msg: Msg) -> bool {
        let member = match &msg {
            Msg::Result { member, .. }
            | Msg::More { member, .. }
            | Msg::Done { member, .. }
            | Msg::Failed { member, .. }
            | Msg::Dropped { member }
            | Msg::Ended { member } => *member,
        };
        if self.states[member].cancelled {
            return false;
        }
        let agent = self.members[member].agent.clone();
        match msg {
            Msg::Result { event, .. } => {
                if self.states[member].answered {
                    return false;
                }
                let more_expected = event.more_expected;
                self.states[member].answered = true;
                self.unchecked |= event.unchecked_sources;
                self.reported.extend(event.unanswered);
                for hit in event.pending {
                    self.pending.push((agent.clone(), hit));
                }
                self.lists.push((member, agent, event.hits));
                if !more_expected {
                    self.end_member(member);
                }
                self.cancel_partner(member);
                true
            }
            Msg::More { event, .. } => {
                self.unchecked |= event.unchecked_sources;
                self.reported.extend(event.unanswered);
                for hit in event.pending {
                    self.pending.push((agent.clone(), hit));
                }
                self.lists.push((member, agent, event.hits));
                true
            }
            Msg::Done { event, .. } => {
                self.unchecked |= event.unchecked_sources;
                self.reported.extend(event.unanswered);
                self.end_member(member);
                false
            }
            Msg::Failed { reason, .. } => {
                self.states[member].failure = Some(reason);
                false
            }
            Msg::Dropped { .. } => {
                self.states[member].dropped = true;
                false
            }
            Msg::Ended { .. } => {
                self.states[member].ended = true;
                false
            }
        }
    }

    fn end_member(&mut self, member: usize) {
        self.states[member].ended = true;
        if let Some(token) = &self.states[member].cancel {
            token.cancel();
        }
    }

    /// The first `result` of a primary and backup pair is kept; the other task is cancelled and its
    /// later events ignored (H.3).
    fn cancel_partner(&mut self, winner: usize) {
        let partner = self.slots.iter().find_map(|s| match s.backup {
            Some(b) if s.primary == winner => Some(b),
            Some(b) if b == winner => Some(s.primary),
            _ => None,
        });
        if let Some(partner) = partner {
            if !self.states[partner].answered {
                self.states[partner].cancelled = true;
                if let Some(token) = &self.states[partner].cancel {
                    token.cancel();
                }
            }
        }
    }

    fn batch(&self, final_batch: bool) -> KnowledgeBatch {
        let mut ordered: Vec<&(usize, String, Vec<KnowledgeHit>)> = self.lists.iter().collect();
        ordered.sort_by_key(|(member, _, _)| *member);
        let lists: Vec<(String, Vec<KnowledgeHit>)> = ordered
            .into_iter()
            .map(|(_, agent, hits)| (agent.clone(), hits.clone()))
            .collect();
        let hits = merge_hits(lists, self.plan.limit);
        let mut relied_on: Vec<(String, String, String)> = Vec::new();
        for hit in &hits {
            let item = (
                hit.agent.clone(),
                hit.repository.clone(),
                hit.commit.clone(),
            );
            if !relied_on.contains(&item) {
                relied_on.push(item);
            }
        }
        let mut answered = Vec::new();
        let mut unanswered: Vec<UnansweredSource> = Vec::new();
        let mut add = |source: UnansweredSource| {
            let seen = unanswered.iter().any(|u| {
                u.agent == source.agent && reason_rank(u.reason) == reason_rank(source.reason)
            });
            if !seen {
                unanswered.push(source);
            }
        };
        for slot in &self.slots {
            let answered_slot = self.slot_answered(slot);
            for index in std::iter::once(slot.primary).chain(slot.backup) {
                let state = &self.states[index];
                let agent = self.members[index].agent.clone();
                if state.answered {
                    answered.push(agent);
                } else if answered_slot || !state.started || state.dropped || state.cancelled {
                    // Answered by the other of the pair, not asked, or dropped: reported nowhere.
                } else {
                    let reason = match state.failure {
                        Some(Fail::Refused) => UnansweredReason::Refused,
                        Some(Fail::Unreachable) => UnansweredReason::Unreachable,
                        None => UnansweredReason::NotInTime,
                    };
                    add(UnansweredSource { agent, reason });
                }
            }
        }
        for source in &self.reported {
            add(source.clone());
        }
        KnowledgeBatch {
            hits,
            answered,
            unanswered,
            relied_on,
            final_batch,
            unchecked_sources: self.unchecked,
            pending: self.pending.clone(),
        }
    }

    async fn emit(&self, final_batch: bool) -> bool {
        self.out.send(self.batch(final_batch)).await.is_ok()
    }

    fn log_summary(&self, first_ms: u128) {
        let batch = self.batch(true);
        let count = |reason: UnansweredReason| {
            batch
                .unanswered
                .iter()
                .filter(|u| reason_rank(u.reason) == reason_rank(reason))
                .count()
        };
        tracing::debug!(
            component = COMPONENT,
            request_id = %self.plan.request_id,
            targets = self.members.len(),
            answered = batch.answered.len(),
            not_in_time = count(UnansweredReason::NotInTime),
            refused = count(UnansweredReason::Refused),
            unreachable = count(UnansweredReason::Unreachable),
            first_ms = first_ms as u64,
            "kq_ask"
        );
    }

    /// Cancels every open task: the stream was dropped or the client closed.
    fn stop_all(&self) {
        self.cancel.cancel();
    }

    async fn run(mut self) {
        let primaries: Vec<usize> = self.slots.iter().map(|s| s.primary).collect();
        for index in primaries {
            self.start(index);
        }
        let shutdown = self.inner.shutdown.clone();
        let first_deadline = self.plan.first_deadline;
        let more_until = self.plan.more_until;
        let mut hedge_at = Some(Instant::now() + self.inner.timings.hedge_delay);

        // Phase 1: until every slot is resolved, or the first deadline.
        loop {
            if self.all_resolved() {
                break;
            }
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                _ = shutdown.cancelled() => {
                    self.fail_open();
                    let _ = self.emit(true).await;
                    self.stop_all();
                    return;
                }
                msg = self.rx.recv() => {
                    if let Some(msg) = msg {
                        self.handle(msg);
                    }
                }
                _ = sleep_until(hedge_at.unwrap_or(first_deadline)), if hedge_at.is_some() => {
                    hedge_at = None;
                    self.start_backups();
                }
                _ = sleep_until(first_deadline) => break,
            }
        }
        let first_ms = self.started_at.elapsed().as_millis();
        let final_first = self.all_ended();
        if !self.emit(final_first).await {
            self.stop_all();
            return;
        }
        if final_first {
            self.log_summary(first_ms);
            self.stop_all();
            return;
        }

        // Phase 2: More found, until `more_until` or until every task has ended.
        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => return,
                _ = shutdown.cancelled() => {
                    self.fail_open();
                    let _ = self.emit(true).await;
                    self.stop_all();
                    return;
                }
                msg = self.rx.recv() => {
                    let Some(msg) = msg else { break };
                    let changed = self.handle(msg);
                    if changed && !self.emit(false).await {
                        self.stop_all();
                        return;
                    }
                    if self.all_ended() {
                        break;
                    }
                }
                _ = sleep_until(hedge_at.unwrap_or(more_until)), if hedge_at.is_some() => {
                    hedge_at = None;
                    self.start_backups();
                }
                _ = sleep_until(more_until) => break,
            }
        }
        let _ = self.emit(true).await;
        self.log_summary(first_ms);
        self.stop_all();
    }

    /// The client is closing: every open source that has not answered is listed `Unreachable`.
    fn fail_open(&mut self) {
        for state in &mut self.states {
            if !state.answered && state.failure.is_none() {
                state.failure = Some(Fail::Unreachable);
                state.started = true;
            }
        }
    }
}
