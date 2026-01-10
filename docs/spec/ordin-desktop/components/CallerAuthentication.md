# SPEC: Caller Authentication

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-111; promoted to Approved by the user January 2026) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-caller-auth` · **Build phase:** 2 · **Programs:** A
> **Dependencies:** Agent Configuration

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance / Security / Observability](#performance--security--observability)
- [Gaps](#gaps)
- [Amendment January 2026 — Agentic memory as a core capability (ADR-111)](#amendment-january-2026--agentic-memory-as-a-core-capability-adr-111)

## Purpose

Caller Authentication verifies the identity-provider token of every request to a hosted agent's HTTP routes except health, metrics and a signed intake, in ordin's own code: signature against the provider's published keys, then issuer, audience and expiry, then the role from the one claim the agent configuration names (02:797–799, ADR-098 item 6 *Authentication and roles*, wrightrun `AuthenticationCheck.md`). It owns the table that gives every route its least role; a route the table does not hold is refused, and the Caller API refuses to start with one (wrightrun R-11). A caller starts runs and reads, follows, resumes and cancels its own; an operator may do all that and use the operator and work-folder routes. It also tells whether the token's holder is one of the configuration's approvers (ADR-097 item 4) and checks the company sign-in of an ordin desktop that reaches the agent by role, with the same verification (ADR-097 item 9; 03 *A desktop uses a hosted agent by role*).

## Approach

One `Verifier`, built once per instance from the configuration's `auth` section, with an in-memory key-set cache whose timing rules are wrightrun's (300 s lifetime, 30 s cooldown on an unknown `kid`, 5 s fetch limit, 10 s back-off after a failed fetch, 30 s clock leeway), plus pure functions over a fixed route table. JWT decoding and signature checks use the `jsonwebtoken` crate, adopted for this component in 05 under decision 80 (SA-RECON-01) and already in the workspace (`memory/Cargo.toml:47`; its JWK decoding is used at `memory/src/auth/jwks.rs:11-12,106`), and the key set is fetched with `reqwest` (SA-HTTP-02). Error prefix: `AUTH_` (wrightrun's codes, kept unchanged because callers parse them, SA-ENV-02). Rejected: an HTTP middleware that decides everything — the own-runs rules need the run's record, which only the route reads. Rejected: caching keys without expiry — a key the provider removed would stay accepted until a restart.

### Reconciliation assumptions

- **SA-RECON-01** `jsonwebtoken` is adopted for Caller Authentication; 05 carries its row (ruling R22).
- **SA-RECON-02** Caller Authentication does not implement the Memory Service's `CallerAuthenticator` (reconciliation row 432): the Memory Service's own `InProcessAuthenticator` (MemoryService.md Internal Logic, steps 1–2) implements it, and an identity-provider token never reaches the memory component.
- **SA-RECON-03** The approver list is the Agent Configuration's `ApproversConfig` (ruling R6): a token is an approver by `users[].subject` or by a group under `group_claim`, which may differ from `role_claim`; the configuration's `approvers` key is optional.

### Closing assumptions

- **SA-CLOSE-01** *(chosen here)* `Principal` carries `groups: Vec<String>`, the string values of the claim named by `approvers.group_claim`, read as the role claim is read (Internal Logic step 11); with `approvers = None` the list is empty. The Caller API passes it to the Approval Service as `PressOrigin::Approver.groups`, so an approver named only by group is recognised there.
- **SA-CLOSE-02** *(chosen here)* The route table holds the Caller API's approver-card routes 31 `GET /v1/approvals` and 32 `POST /v1/approvals/{approval_id}/decision` (CallerApi.md SA-CLOSE-01) with least role `caller`. Being an approver is not a role of this table: the Caller API refuses a principal whose `approver` is `false` on those two routes with `AUTH_FORBIDDEN`. The table key is the path without its query string, so `?state=pending` is not part of it.

## Shared Context

### From Agent Configuration (types in `ordin_types::domain::agent_profile`, copied from AgentConfiguration.md; reconciliation rows 589, 591, N60, N125)

wrightrun's `auth` section (YAML keys with hyphens) and ADR-097's optional approver list:

```rust
// Copied from AgentConfiguration.md (`ordin_types::domain::agent_profile`); the derive and serde attributes are the ones
// that spec states for every configuration struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct AuthConfig {
    pub issuer: String,                 // https URL
    pub audience: String,
    pub jwks_url: String,               // https URL (http allowed only for localhost / 127.0.0.1)
    pub role_claim: String,             // a top-level claim name, e.g. "groups"
    pub operator_value: String,
    pub caller_value: String,
    #[serde(default = "rs256")] pub algorithms: Vec<String>,   // default ["RS256"]
}

/// ADR-097 item 4: users, or groups the identity provider asserts.
pub struct ApproversConfig {
    #[serde(default)] pub users: Vec<ApproverUser>,
    /// Identity-provider group names; checked only where a token is presented.
    #[serde(default)] pub groups: Vec<String>,
    #[serde(default = "groups_claim")] pub group_claim: String,  // default "groups"
    #[serde(default)] pub asker_may_approve_internal: bool,      // default false
}
pub struct ApproverUser {
    #[serde(default)] pub chat_user: Option<String>,   // matched by the Channel Gateway, not here
    #[serde(default)] pub subject: Option<String>,     // the identity provider's `sub`, 1..=256
}
```

`AgentConfig.approvers` is `Option<ApproversConfig>`; `None` → nobody is an approver (operators resume through the caller interface, ADR-097 item 4). This crate reads `users[].subject`, `groups` and `group_claim`; `chat_user` and `asker_may_approve_internal` are for the Channel Gateway and the Approval Service.

### Constants

| Constant | Value | Source |
|---|---|---|
| `KEY_SET_SECONDS` | 300 | wrightrun `AuthenticationCheck.md` |
| `KEY_COOLDOWN_SECONDS` | 30 | wrightrun |
| `KEY_FETCH_TIMEOUT_SECONDS` | 5 | wrightrun |
| `KEY_RETRY_SECONDS` | 10 | wrightrun |
| `LEEWAY_SECONDS` | 30 | wrightrun |
| `MAX_SUBJECT_CHARS` | 255 | wrightrun |
| `MAX_TOKEN_BYTES` | 16,384 | *chosen here*: a bearer token longer than this is refused before decoding |
| Accepted algorithms | `RS256`, `RS384`, `RS512`, `ES256`, `ES384`, `PS256`, `PS384`, `PS512` | wrightrun's nine less `ES512`, which `jsonwebtoken::Algorithm` does not offer *(assumed here: the crate's enum has no ES512 variant; a configuration naming it is refused at build)* |

No environment variable is read by this crate.

## Public Interface

```rust
// crate ordin-caller-auth
use ordin_types::domain::agent_profile::{ApproversConfig, AuthConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role { Caller, Operator }          // Operator > Caller: an operator passes a caller check

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeastRole { None, Caller, Operator }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub subject: String,        // the token's "sub", 1–255 characters
    pub role: Role,
    pub approver: bool,         // ADR-097 item 4
    pub groups: Vec<String>,    // values of claims[approvers.group_claim]; empty when approvers = None (SA-CLOSE-01)
}

/// One route as it is declared: method in upper case, path with placeholders.
pub type RouteKey = (&'static str, &'static str);

/// The complete table of routes and their least role (see the table below).
pub const ROUTE_ROLES: &[(&str, &str, LeastRole)];

pub fn required_role(method: &str, route_path: &str) -> Result<LeastRole, AuthError>;
pub fn require_role(principal: &Principal, role: Role) -> Result<(), AuthError>;
/// The own-runs rule: an operator, or the caller that started the run or thread.
pub fn may_act(principal: &Principal, started_by: &ordin_types::RunStarter) -> bool;

pub struct Verifier { /* config, approvers, http client, key cache */ }
impl Verifier {
    /// No network call; fails with AUTH_ALGORITHM for an algorithm outside the accepted list.
    pub fn new(auth: AuthConfig, approvers: Option<ApproversConfig>) -> Result<Self, AuthError>;
    pub async fn verify(&self, authorization_header: Option<&str>) -> Result<Principal, AuthError>;
}

/// required_role, then verify, then require_role; Ok(None) for a route whose least role is None.
pub async fn check_request(verifier: &Verifier, method: &str, route_path: &str,
                           authorization_header: Option<&str>) -> Result<Option<Principal>, AuthError>;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("a bearer token is required")]                           Missing,
    #[error("the request's token was not accepted")]                 Invalid,
    #[error("the token carries no role of this agent")]              NoRole,
    #[error("the token's role does not permit this request")]        Forbidden,
    #[error("no role is stated for this route")]                     RouteUnmapped { route: String },
    #[error("the identity provider's keys cannot be read; try again later")] KeysUnavailable,
    #[error("the signing algorithm {0} is not accepted")]            Algorithm(String),
}
impl AuthError {
    pub fn code(&self) -> &'static str;          // see Error Table
    pub fn http_status(&self) -> u16;            // 401, 401, 403, 403, 403, 503, — (start-up)
    pub fn retryable(&self) -> bool;             // true for KeysUnavailable only
}
```

**Called by.**

| Public item | Called by |
|---|---|
| `Verifier::new` | the server program (OrdinServer, agent profile, `requests` role and single shape), from the Agent Configuration's `auth` and `approvers` |
| `check_request`, `may_act`, `required_role`, `require_role`, `ROUTE_ROLES`, `AuthError::{code, http_status}` | Caller API (every route but intakes, health and metrics; `build_router` checks every route against `ROUTE_ROLES`) |
| `Verifier::verify` | Caller API (intake token form, through `check_request`) |
| `Principal.approver` | Caller API, routes 31–32 (an approver's open cards and decision); the resume route does not consult it (CallerApi SA-RECON-01) |
| `Principal.groups` | Caller API, route 32, passed as `PressOrigin::Approver.groups` to the Approval Service (SA-CLOSE-01) |

Not offered here: the Memory Service's `CallerAuthenticator` (`validate(bearer, correlation_id) -> Result<ScopeContext, AuthError>`). The Memory Service implements it itself with its `InProcessAuthenticator`, which mints a per-call bearer for a context it builds; no identity-provider token reaches it, so this crate has no adapter for it (reconciliation row 432, SA-RECON-02).

**The route table, complete** (wrightrun's 28 entries, the two routes ADR-098 item 2 adds, and the Caller API's two approver-card routes; the added paths are *chosen here*):

| Method | Route path | Least role |
|---|---|---|
| `POST` | `/v1/runs` | caller |
| `POST` | `/v1/threads/{thread_id}/runs` | caller |
| `GET` | `/v1/runs/{run_id}` | caller |
| `POST` | `/v1/runs/{run_id}/resume` | caller |
| `GET` | `/v1/runs/{run_id}/events` | caller |
| `POST` | `/v1/runs/{run_id}/cancel` | caller |
| `GET` | `/work-folder/files` | operator |
| `GET` | `/work-folder/files/{*path}` | operator |
| `PUT` | `/work-folder/files/{*path}` | operator |
| `DELETE` | `/work-folder/files/{*path}` | operator |
| `POST` | `/intakes/{name}` | none (the Caller API verifies each event itself) |
| `GET` | `/operator/runs` | operator |
| `GET` | `/operator/scopes/{scope}/settings` | operator |
| `GET` | `/operator/scopes/{scope}/settings/{name}` | operator |
| `PUT` | `/operator/scopes/{scope}/settings/{name}` | operator |
| `GET` | `/operator/jobs` | operator |
| `POST` | `/operator/jobs` | operator |
| `GET` | `/operator/scopes/{scope}/notes` | operator |
| `GET` | `/operator/scopes/{scope}/notes/{note_id}` | operator |
| `POST` | `/operator/scopes/{scope}/notes/{note_id}/retire` | operator |
| `GET` | `/operator/scopes/{scope}/entries` | operator |
| `POST` | `/operator/scopes/{scope}/entries` | operator |
| `POST` | `/operator/scopes/{scope}/entries/{entry_id}/verify` | operator |
| `DELETE` | `/operator/scopes/{scope}/entries/{entry_id}` | operator |
| `POST` | `/operator/erase` | operator |
| `GET` | `/operator/duties` | operator |
| `GET` | `/operator/duties/{duty}/lock` | operator |
| `GET` | `/healthz` | none |
| `GET` | `/readyz` | none |
| `GET` | `/metrics` | none |
| `GET` | `/v1/approvals` | caller (the Caller API also requires `approver`; SA-CLOSE-02) |
| `POST` | `/v1/approvals/{approval_id}/decision` | caller (the Caller API also requires `approver`; SA-CLOSE-02) |

Thirty-two entries. `{*path}` is axum 0.8's wildcard form (SA-HTTP-01) of wrightrun's `{path:path}`. `HEAD` is looked up as `GET`.

**Which component makes which check.** This crate: the token's signature, issuer, audience, expiry, `sub`, the role, the approver flag, the route's least role, and that every route of the application is in the table. The Caller API: the own-runs rules (with `may_act`), the intake's signature, and whether the run's resume is open to callers when approvers are configured.

### Example

```yaml
auth:
  issuer: https://id.example.org/oauth2/default
  audience: ordin-support-agent
  jwks-url: https://id.example.org/oauth2/default/v1/keys
  role-claim: groups
  operator-value: support-agent-operators
  caller-value: support-agent-callers
approvers:
  groups: [support-leads]
```

Token header `{"alg":"RS256","kid":"k-2026-10"}`, payload `{"iss":"https://id.example.org/oauth2/default","aud":"ordin-support-agent","sub":"00u8x2asha","exp":1791369000,"groups":["Everyone","support-agent-callers","support-leads"]}`:

```rust
let verifier = Verifier::new(config.auth.clone(), config.approvers.clone())?;
let p = verifier.verify(Some("Bearer eyJhbGciOiJSUzI1NiIsImtpZCI6ImstMjAyNi0xMCJ9.…")).await?;
// Principal { subject: "00u8x2asha", role: Role::Caller, approver: true,
//             groups: ["Everyone", "support-agent-callers", "support-leads"] }
required_role("POST", "/v1/runs")?;              // LeastRole::Caller
required_role("HEAD", "/operator/runs")?;        // LeastRole::Operator
required_role("GET", "/operator/secrets");       // Err(AuthError::RouteUnmapped) — AUTH_ROUTE_UNMAPPED
require_role(&p, Role::Operator);                // Err(AuthError::Forbidden) — AUTH_FORBIDDEN, 403
```

## Internal Logic

**`Verifier::new(auth, approvers)`**

1. Each item of `auth.algorithms` must be one of the accepted eight; otherwise `Algorithm(item)` (code `AUTH_ALGORITHM`), which stops the start. The Agent Configuration refuses these first; this guard keeps `none` and `HS*` out of decoding for a config built in code.
2. Build a `reqwest::Client` with a 5 s total timeout, redirects disabled (`redirect::Policy::none()`), and the platform verifier (SA-HTTP-02). No network call. Key cache empty; `unavailable_until = None`. Nothing logged.

**`Verifier::verify(header)`**

3. `None` or empty → refuse `Missing`, reason `no-header`.
4. Split at the first space; first part not `Bearer` (case-insensitive), second part empty, longer than 16,384 bytes, or holding a space → refuse `Missing`, reason `bad-scheme`.
5. `now < unavailable_until` → refuse `KeysUnavailable` without fetching.
6. `jsonwebtoken::decode_header(token)`: error → refuse `Invalid`, reason `malformed`. Header `alg` not in `auth.algorithms` → refuse `Invalid`, reason `algorithm`. No `kid` → use the only key of the set when the set holds exactly one key, else refuse `Invalid`, reason `unknown-key`.
7. **Key lookup.** The cache holds `(keys: HashMap<kid, DecodingKey>, fetched_at)`. If the cache is empty or older than 300 s → fetch. Else if `kid` is absent from it and the last successful fetch was more than 30 s ago → fetch once. A fetch is one `GET jwks_url` (5 s limit) under a `tokio::sync::Mutex` so concurrent requests share one fetch; the body is parsed as `jsonwebtoken::jwk::JwkSet` and each JWK becomes a `DecodingKey::from_jwk` (keys that fail are skipped). Fetch failure (network, timeout, HTTP status ≥ 400, unparsable body, zero usable keys): set `unavailable_until = now + 10 s`, log `auth_keys_unavailable` (ERROR; `error` = the error's kind name only), and — when the cache still holds a set younger than 300 s that has the `kid` — continue with it; otherwise refuse `KeysUnavailable`. `kid` still absent after a successful fetch → refuse `Invalid`, reason `unknown-key`.
8. `jsonwebtoken::decode::<serde_json::Map>(token, key, &validation)` with `Validation { algorithms: auth.algorithms, iss: {issuer}, aud: {audience}, leeway: 30, required_spec_claims: {"exp","iss","aud","sub"}, validate_exp: true, validate_nbf: true }`. Error kinds map to `Invalid` with reasons: `ExpiredSignature` → `expired`; `InvalidAudience` → `audience`; `InvalidIssuer` → `issuer`; `MissingRequiredClaim` → `claim-missing`; `InvalidAlgorithm` → `algorithm`; `InvalidSignature` → `signature`; `ImmatureSignature` → `not-yet-valid`; any other → `malformed`.
9. `sub` must be a string of 1–255 characters → else refuse `Invalid`, reason `subject`.
10. **Role.** `raw = claims[auth.role_claim]` (a top-level claim; a dot has no special meaning). A string is one value compared whole (not split at spaces); an array contributes its string items; anything else contributes nothing. `operator_value` among the values → `Operator`; else `caller_value` among them → `Caller`; else refuse `NoRole`, reason `no-role` (the log line carries `subject`).
11. **Approver.** With `approvers = None` → `false`. Else `gvalues` = the values of `claims[approvers.group_claim]` read as step 10 reads the role claim; `approver = approvers.users.any(|u| u.subject == Some(sub)) || gvalues ∩ approvers.groups ≠ ∅`. A user given only by `chat_user` never makes a token an approver. *(SA-CLOSE-01)* `groups` = `gvalues` in claim order with duplicates removed, at most 256 values of at most 255 characters each (longer values are dropped, values past the 256th are ignored); with `approvers = None`, `groups` is empty. A `group_claim` absent from the token gives an empty list, never a refusal.
12. Return `Principal`. Nothing is logged for an accepted token.

"Refuse `<variant>`, reason `<r>`" means: log `auth_refused` (WARN; `code`, `reason`; `subject` only for `no-role`), then return the error. The message is the same for every reason of one code, so a response never says which check failed.

**`require_role(principal, role)`** — `role == Operator && principal.role != Operator` → log `auth_forbidden` (WARN; `subject`, `required`) and `Forbidden`. Otherwise `Ok`.

**`required_role(method, path)`** — `HEAD` → `GET`; upper-case; drop a query string (`?` and what follows); look up `(method, path)` in `ROUTE_ROLES`; absent → `RouteUnmapped { route: "<METHOD> <path>" }` and log `auth_route_unmapped` (ERROR; `method`, `route`).

**`check_request`** — `required_role`; `LeastRole::None` → `Ok(None)`; else `verify`, `require_role(principal, Caller | Operator)`, `Ok(Some(principal))`.

**`may_act(principal, started_by)`** — `principal.role == Operator || matches!(started_by, RunStarter::Caller { caller_id } if caller_id == &principal.subject)`. A run or thread a schedule, an intake or a chat started is reached by an operator only.

**Key rotation, as configured:** a known key set is reused for 300 s with no network call; an unknown `kid` causes at most one fetch per 30 s, so a key the provider adds is accepted within 30 s of the last fetch with no restart; a removed key is accepted until the kept set expires (at most 300 s); during a provider outage one fetch attempt per 10 s. Each instance keeps its own set; nothing is stored.

## Data Model

N/A — keys and tokens are held in memory only, for the times stated; this crate owns no table.

## Error Table

Statuses are those the Caller API answers (wrightrun registry, unchanged). The `AUTH_` prefix is carried from wrightrun and belongs to this crate alone (Error Handling, EH-D1): the Authoring Service's codes use `AUTHR_`.

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No `Authorization` header, or not `Bearer <token>`, or a token over 16,384 bytes | 401 | `AUTH_MISSING` | `{"error": "a bearer token is required", "code": "AUTH_MISSING"}` |
| Not a token; unknown `kid`; signature, expiry, not-before, issuer, audience, required claim, algorithm or `sub` fails | 401 | `AUTH_INVALID` | `{"error": "the request's token was not accepted", "code": "AUTH_INVALID"}` |
| Valid token whose role claim holds neither value | 403 | `AUTH_NO_ROLE` | `{"error": "the token carries no role of this agent", "code": "AUTH_NO_ROLE"}` |
| A caller token on an operator route | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| A principal whose `approver` is `false` on route 31 or 32 (the Caller API applies the check with this crate's error; SA-CLOSE-02) | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| A route not in `ROUTE_ROLES`, at a request | 403 | `AUTH_ROUTE_UNMAPPED` | `{"error": "no role is stated for this route", "code": "AUTH_ROUTE_UNMAPPED"}` |
| The same, when the Caller API checks its routes at start | exit 1 | `AUTH_ROUTE_UNMAPPED` | `error[AUTH_ROUTE_UNMAPPED]: no role is stated for this route (GET /operator/secrets)` |
| Keys unreadable within 5 s or unusable, and no kept set younger than 300 s holds the `kid` | 503, `Err(retryable)` | `AUTH_KEYS_UNAVAILABLE` | `{"error": "the identity provider's keys cannot be read; try again later", "code": "AUTH_KEYS_UNAVAILABLE"}` |
| `auth.algorithms` holds a value outside the accepted eight, at `Verifier::new` | exit 1 | `AUTH_ALGORITHM` | `error[AUTH_ALGORITHM]: the signing algorithm HS256 is not accepted` |

The person never sees a code (07). On the desktop, the Hosted Agent Adapter shows `AUTH_FORBIDDEN` / `AUTH_NO_ROLE` as "You don't have access to [agent]. Ask IT if you need it." (03 *A desktop uses a hosted agent by role*, Error posture); that text is the adapter's, not this crate's.

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Caller Authentication

  Background:
    Given an identity provider stub serving a key set with the RSA key "k1"
    And an AuthConfig with issuer "https://id.test/", audience "ordin-test", role-claim "groups", operator-value "ops", caller-value "callers", jwks-url the stub's address
    And an ApproversConfig with groups ["leads"], group-claim "groups" and users [{subject: "u-approver"}, {chat-user: "U07LEAD"}]
    And a Verifier built from them

  Scenario: Happy path — a caller token gives a caller principal
    Given a token signed with "k1", right issuer and audience, sub "pipeline-7", groups ["callers"], expiry in 300 s
    When verify receives "Bearer <token>"
    Then it returns Principal { subject "pipeline-7", role Caller, approver false }
    And the stub recorded 1 key-set request

  Scenario: Happy path — an approver group sets the approver flag
    Given a valid token with groups ["callers", "leads"]
    When verify receives it
    Then the principal has role Caller and approver true

  Scenario: Edge case — a token with both role values is an operator
    Given a valid token with groups ["callers", "ops"]
    Then verify returns role Operator

  Scenario: Edge case — approver groups come from the configured group claim
    Given an ApproversConfig with groups ["leads"] and group-claim "approver_groups"
    And a valid token with groups ["callers", "leads"] and approver_groups ["staff"]
    When verify is called
    Then the principal has role Caller and approver false

  Scenario: Happy path — the principal carries the group-claim values (SA-CLOSE-01)
    Given an ApproversConfig with groups ["leads"] and group-claim "approver_groups"
    And a valid token with groups ["callers"] and approver_groups ["staff", "leads", "staff"]
    When verify is called
    Then the principal has approver true and groups ["staff", "leads"]

  Scenario: Edge case — no approvers configured gives no groups
    Given a Verifier built with approvers None
    And a valid token with groups ["callers", "leads"]
    When verify is called
    Then the principal has approver false and groups []

  Scenario: Happy path — the approver-card routes are mapped (SA-CLOSE-02)
    When required_role("GET", "/v1/approvals?state=pending") and required_role("POST", "/v1/approvals/{approval_id}/decision") are called
    Then both return LeastRole::Caller

  Scenario: Edge case — a role claim given as one string is compared whole
    Given a valid token whose "groups" is the string "callers ops"
    Then verify fails with code AUTH_NO_ROLE

  Scenario: Error path — no header
    When verify receives None
    Then it fails with code AUTH_MISSING and http_status 401

  Scenario Outline: Error path — a token that fails one check
    Given a token signed with "<key>" whose <claim> is <value>
    When verify receives it
    Then it fails with code AUTH_INVALID and message "the request's token was not accepted"
    And the auth_refused log line carries reason "<reason>" and no part of the token

    Examples:
      | key       | claim | value                 | reason        |
      | k1        | exp   | 120 seconds ago       | expired       |
      | k1        | aud   | "another-service"     | audience      |
      | k1        | iss   | "https://other.test/" | issuer        |
      | k1        | sub   | absent                | claim-missing |
      | k1        | sub   | ""                    | subject       |
      | not-known | exp   | 300 seconds ahead     | unknown-key   |

  Scenario: Edge case — an expiry 20 seconds past is inside the leeway
    Given a token signed with "k1" whose exp is 20 seconds ago
    Then verify returns a Principal

  Scenario: Error path — an HS256 token signed with the public key text is refused
    Given a token with alg "HS256" signed with the text of "k1"
    Then verify fails with code AUTH_INVALID

  Scenario: Error path — an intake's shared secret is not a token
    When verify receives "Bearer 0123456789abcdef0123456789abcdef01234567"
    Then it fails with code AUTH_INVALID with reason "malformed"

  Scenario: Error path — a configuration naming HS256
    When Verifier::new is called with algorithms ["HS256"]
    Then it fails with code AUTH_ALGORITHM

  Scenario: Edge case — 50 tokens within 10 seconds cause one fetch
    Given 50 valid tokens signed with "k1"
    When verify receives each within 10 seconds
    Then the stub recorded 1 key-set request

  Scenario: Edge case — a key added by the provider is accepted without a restart
    Given verify accepted a "k1" token 31 seconds ago
    And the stub now serves "k1" and "k2"
    When verify receives a valid "k2" token
    Then it returns a Principal and the stub recorded 2 key-set requests

  Scenario: Edge case — a removed key is refused once the kept set expired
    Given verify accepted a "k1" token and the stub now serves only "k2"
    When 301 seconds pass and verify receives a valid "k1" token
    Then it fails with code AUTH_INVALID

  Scenario: Error path — the provider is unreachable
    Given the stub refuses connections and no key set is kept
    When verify receives a well-formed token
    Then it fails with code AUTH_KEYS_UNAVAILABLE, http_status 503, retryable true
    And a second token 2 seconds later fails the same way with no new connection attempt

  Scenario: Error path — a caller on an operator route
    Given a valid caller token
    When check_request("GET", "/operator/runs", "Bearer <token>") is awaited
    Then it fails with code AUTH_FORBIDDEN

  Scenario: Error path — an unmapped route
    When required_role("GET", "/operator/secrets") is called
    Then it fails with code AUTH_ROUTE_UNMAPPED and route "GET /operator/secrets"

  Scenario: Edge case — open routes need no token
    When check_request("GET", "/readyz", None) and check_request("POST", "/intakes/{name}", None) are awaited
    Then both return Ok(None) and the stub recorded 0 requests

  Scenario: The own-runs rule
    Given a Principal { subject "pipeline-7", role Caller }
    Then may_act with Caller "pipeline-7" is true, with Caller "pipeline-8" is false, with ScheduleEntry "daily/run-check/0 6 * * *" is false
    And for an operator principal may_act with ScheduleEntry is true
```

## Performance / Security / Observability

- **Performance targets:** with the key set kept, `verify` adds under 5 ms at p95 (one RSA or ECDSA check); `required_role` under 10 µs. A key fetch is bounded by 5 s and happens at most once per 300 s for known keys and once per 30 s for an unknown `kid`; during an outage one attempt per 10 s. Memory: one key set (a few kilobytes) per instance.
- **Security:** only asymmetric algorithms; `none` and `HS*` cannot be configured and the algorithm list is passed to every decode. `exp`, `iss`, `aud`, `sub` required; leeway 30 s. No private key and no shared secret is held. The key address is read with no redirect and no credential. Every refusal of one code has one message. The token, its claims and the key set never appear in a log line or error; `subject` appears only in `auth_forbidden` and the `no-role` refusal, after verification. A 401 carries no `WWW-Authenticate` header. No rate limit (07 — Rate limiting: limits are declared controls).
- **Observability:** agent profile only. Log events (target `ordin_caller_auth`): `auth_refused` (WARN; `code`, `reason`, `subject` for `no-role`), `auth_forbidden` (WARN; `subject`, `required`), `auth_keys_unavailable` (ERROR; `error`), `auth_route_unmapped` (ERROR; `method`, `route`). No counter (wrightrun lists none for this unit, and this crate does not depend on Telemetry). No trace span.

## Gaps

None.

---

## Amendment January 2026 — Agentic memory as a core capability (ADR-111)

> Source: decision 91 (the user's decisions of January 2026); HLD revision 0.60.0, ADR-111 (open question 6: a hosted agent's owners see its memory scope by scope). Counterpart: `components/CallerApi.md` (amendment January 2026, route 33).
> Every rule below adds to the sections above.

### Purpose (addition)

The route table gains one operator route, through which a hosted agent's owners — its operators, the role that already owns every record of the agent (CallerApi.md *Routes*, "Owner") — read and act on the agent's memory scope by scope.

### Approach (addition)

The owners of open question 6 are the agent's operators: the agent profile already names one role that may act on every scope (settings, notes, entries, erasure), so a second notion of owner is not added. Rejected: *a new `memory_owner` role claim* — a third role would need an identity-provider change on every deployment for no difference in who may already erase the same memory with route 25.

### Shared Context, Data Model

N/A — no type or table changes.

### Public Interface (changes)

**The route table** gains one row (*chosen here*), so it holds 33 entries:

| Method | Route path | Least role |
|---|---|---|
| `POST` | `/operator/memory/{verb}` | operator |

### Internal Logic

N/A — `required_role` and `check_request` read the table as before.

### Error Table (rows added)

N/A — a caller on the new route is refused with the existing `AUTH_FORBIDDEN`.

### Acceptance Criteria (Gherkin, added)

```gherkin
  Scenario: The memory route needs an operator
    When required_role("POST", "/operator/memory/things") is called
    Then it returns LeastRole::Operator

  Scenario: Error — a caller token on the memory route
    Given a verified token with role caller
    When check_request runs for POST /operator/memory/things
    Then it fails with AUTH_FORBIDDEN
```

### Performance / Security / Observability (additions)

N/A — no change.

### Gaps

None.

---

## Amendment January 2026 — Knowledge agents (ADR-113)

> Source: decision 92 (the user's decisions of January 2026); HLD revision 0.61.0, ADR-113 (knowledge agents answer retrieval on a route group of the Caller API port, kept apart from the caller interface; askers are admitted by audience). Counterparts: `components/CallerApi.md` (amendment January 2026 — knowledge agents: routes 34–39), `components/KnowledgeQueryServer.md` (the audience and front-door rules applied to the principal this crate returns), `components/AgentConfiguration.md` (amendment January 2026: the `knowledge-agent` section that carries the audiences, the group claim and the front doors).
> Every rule below adds to the sections above. An agent whose configuration has no `knowledge-agent` section builds its `Verifier` exactly as before, and none of the new routes is served.

### Purpose (addition)

The knowledge routes (all but `GET /v1/knowledge/about`, which needs no token: a desktop reads the agent's issuer, token audience and sources-record policy from it before the person signs in, and it carries no content) are reached by people and programs that may hold neither the caller nor the operator role: membership of an **audience group** admits them, and a **front door** (another knowledge agent) is admitted by its own service identity. This crate verifies the token exactly as for every other route, and returns who the asker is, which of the agent's audience groups they belong to, and whether they are a listed front door. Whether that admits them to an answer is the Knowledge Query Server's decision.

### Approach (addition)

A fourth least role, `Member`, marks the knowledge routes. It needs a valid token and nothing from the role claim: the knowledge audience is a different set of people from the agent's callers and operators, and requiring the caller role would make every member of an audience a caller of the agent's runs. The result is a separate type, `KnowledgePrincipal`, because `Principal.role` is required and a knowledge asker has none; `Principal` is not changed, so no existing route can read a knowledge principal as a caller. Rejected: *adding `groups` and a `kind` to `Principal`* — `Principal.groups` already carries the approver group claim's values (SA-CLOSE-01), and a principal with no role would weaken `require_role` on every caller route. Rejected: *deciding the audience here and refusing with an `AUTH_` code* — the front door's access group arrives in the request body, which only the Knowledge Query Server reads, so the one refusal for "outside the audience" is that server's `KQ_NOT_IN_AUDIENCE`.

Assumptions (*chosen here*):

- **SA-CAUTH-KA-01** The audience group claim is named by the configuration's `knowledge-agent.group-claim` (required; `groups` in the examples) and read exactly as step 10 reads the role claim: a string is one value, an array contributes its string items, anything else contributes nothing; duplicates removed, at most 256 values of at most 255 characters.
- **SA-CAUTH-KA-02** A front door is recognised by the token's `sub` only, compared whole with each entry of `knowledge-agent.front-doors`, a list of subjects (the agent's identity provider issues a service identity a stable subject). A token whose subject is listed is a `FrontDoor` whatever its groups say; every other admitted token is a `Member`.
- **SA-CAUTH-KA-03** `audience_groups` lists the configured audiences the token is in, in configuration order; the Knowledge Query Server takes the first as a member's access group. An empty list is returned, not refused, so the refusal names the audience in one plain sentence from one place.
- **SA-CAUTH-KA-04** The issuer, audience, algorithms and key set are the agent's `auth` section, the same as every other route: a knowledge agent uses one identity provider.
- **SA-CAUTH-KA-05** `GET /v1/knowledge/about` has least role `none`, like health and metrics: it returns the agent's name, issuer, token audience, audiences' names, sources-record policy, repository count and "as of", and no fact, thing or repository address, so a desktop can mint the person's token for the right audience and show the policy before the person connects.

### Shared Context (additions)

From the Agent Configuration (amendment January 2026), copied for the fields this crate reads (`ordin_types::domain::agent_profile`):

```rust
pub struct KnowledgeAgentConfig {          // YAML key `knowledge-agent`; AgentConfig.knowledge_agent
    pub audiences: Vec<String>,            // 1–16 group names, each 1–256, unique
    pub group_claim: String,               // file key `group-claim`; 1–128, required
    pub front_doors: Vec<String>,          // 0–32 service-identity subjects, each 1–256, unique
    // … name, repositories, glossaries, proxies, service_identity, limits, mcp, sources_record: not read here
}
```

### Public Interface (additions)

```rust
// crate ordin-caller-auth
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeastRole { None, Caller, Operator, Member }     // Member added: the knowledge routes

/// What the knowledge routes need from the configuration, built by the program from KnowledgeAgentConfig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeAccess {
    pub group_claim: String,
    pub audiences: Vec<String>,          // configuration order
    pub front_door_subjects: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCallerKind { Member, FrontDoor }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgePrincipal {
    pub subject: String,                 // the token's "sub", 1–255 characters
    pub kind: KnowledgeCallerKind,
    pub audience_groups: Vec<String>,    // configured audiences the token is in, configuration order; may be empty
}

impl Verifier {
    /// Turns on the knowledge routes' check. Without it, verify_knowledge refuses with Forbidden.
    pub fn with_knowledge(self, access: KnowledgeAccess) -> Self;
    /// Steps 3–9 of verify, then the knowledge groups; no role is required.
    pub async fn verify_knowledge(&self, authorization_header: Option<&str>) -> Result<KnowledgePrincipal, AuthError>;
}

/// required_role (must be Member), then verify_knowledge.
pub async fn check_knowledge_request(verifier: &Verifier, method: &str, route_path: &str,
                                     authorization_header: Option<&str>) -> Result<KnowledgePrincipal, AuthError>;
```

**The route table** gains seven rows (*chosen here*), so it holds 40 entries; five are `member`, `GET /v1/knowledge/about` is `none` (SA-CAUTH-KA-05), and `GET /operator/knowledge` is `operator`, checked by `check_request` as every operator route:

| Method | Route path | Least role |
|---|---|---|
| `POST` | `/v1/knowledge/query` | member |
| `GET` | `/v1/knowledge/contents` | member |
| `GET` | `/v1/knowledge/contents/index.md` | member |
| `GET` | `/v1/knowledge/about` | none |
| `GET` | `/v1/knowledge/changes` | member |
| `POST` | `/mcp/knowledge` | member |
| `GET` | `/operator/knowledge` | operator |

**Called by (additions).**

| Public item | Called by |
|---|---|
| `Verifier::with_knowledge`, `KnowledgeAccess` | the server program (OrdinServer, agent profile), when the configuration has a `knowledge-agent` section |
| `check_knowledge_request`, `KnowledgePrincipal`, `KnowledgeCallerKind` | Caller API (routes 34–39, its knowledge layer); the principal is handed to the Knowledge Query Server's handlers |

### Internal Logic (additions)

13. **`verify_knowledge(header)`.** No `KnowledgeAccess` set → log `auth_forbidden` (WARN; `required` = `member`) and refuse `Forbidden`. Else steps 3–9 of `verify` unchanged (scheme, keys, signature, expiry, issuer, audience, subject), with the same refusals and log lines.
14. `gvalues` = the values of `claims[access.group_claim]` read as step 10 reads the role claim, with the bounds of SA-CAUTH-KA-01; an absent claim gives an empty list, never a refusal. The role claim is not read.
15. `kind` = `FrontDoor` when `sub` equals an entry of `access.front_door_subjects`, else `Member` (SA-CAUTH-KA-02). `audience_groups` = the entries of `access.audiences` that are in `gvalues`, in `access.audiences` order (SA-CAUTH-KA-03).
16. Return `KnowledgePrincipal`. Nothing is logged for an accepted token; groups and subjects are never logged.
17. **`check_knowledge_request`** — `required_role`; a result other than `Member` → log `auth_forbidden` (WARN; `required` = `member`) and `Forbidden`; else `verify_knowledge`.
18. **`check_request` on a `Member` route** → log `auth_forbidden` (WARN; `required` = `member`) and `Forbidden`: a knowledge route is checked only by step 17, so a caller-route principal can never stand for a knowledge asker. A unit test covers each of the six paths.

### Data Model

N/A — no table; the access settings are held in memory for the instance's life.

### Error Table (rows added)

No new code. The knowledge routes use the existing rows:

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No or malformed bearer on a knowledge route (existing code) | 401 | `AUTH_MISSING` | `{"error": "a bearer token is required", "code": "AUTH_MISSING"}` |
| Token fails a check of steps 6–9 on a knowledge route (existing code) | 401 | `AUTH_INVALID` | `{"error": "the request's token was not accepted", "code": "AUTH_INVALID"}` |
| `verify_knowledge` with no `KnowledgeAccess`, or a knowledge route checked by `check_request` (existing code) | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| Keys unreadable (existing code; retryable) | 503 | `AUTH_KEYS_UNAVAILABLE` | `{"error": "the identity provider's keys cannot be read; try again later", "code": "AUTH_KEYS_UNAVAILABLE"}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Caller Authentication — knowledge routes

  Background:
    Given the AuthConfig of the Background above
    And a KnowledgeAccess with group-claim "groups", audiences ["finance", "procurement"] and front-door subjects ["svc-frontdoor-ops"]

  Scenario: Happy path — a member with no role is admitted to a knowledge route
    Given a valid token with sub "00u1", groups ["Everyone", "procurement", "finance"] and no caller or operator value
    When check_knowledge_request runs for POST /v1/knowledge/query
    Then it returns kind member and audience_groups ["finance", "procurement"]

  Scenario: Happy path — a listed front door
    Given a valid token with sub "svc-frontdoor-ops" and no groups claim
    When check_knowledge_request runs for GET /v1/knowledge/changes
    Then it returns kind front_door and audience_groups []

  Scenario: Edge — outside every audience is returned, not refused
    Given a valid token with sub "00u2" and groups ["sales"]
    When check_knowledge_request runs for POST /mcp/knowledge
    Then it returns kind member and audience_groups []

  Scenario: Error — an expired token on a knowledge route
    Given a token whose exp is 60 s in the past
    When check_knowledge_request runs for GET /v1/knowledge/contents
    Then it fails with AUTH_INVALID

  Scenario: Error — a knowledge route through the caller check
    When check_request runs for POST /v1/knowledge/query with a valid caller token
    Then it fails with AUTH_FORBIDDEN

  Scenario: Error — no knowledge-agent section configured
    Given a Verifier built without with_knowledge
    When verify_knowledge runs with a valid token
    Then it fails with AUTH_FORBIDDEN

  Scenario: The route table names the six knowledge routes
    When required_role("GET", "/v1/knowledge/contents/index.md") is called
    Then it returns LeastRole::Member

  Scenario: The about route needs no token
    When required_role("GET", "/v1/knowledge/about") is called
    Then it returns LeastRole::None
```

### Performance / Security / Observability (additions)

- **Performance:** `verify_knowledge` costs what `verify` costs (no network call while the key set is current); group matching is linear over at most 256 values and 16 audiences.
- **Security:** the same signature, issuer, audience and expiry checks as every route; membership grants nothing beyond the knowledge routes, because `KnowledgePrincipal` cannot be passed where a `Principal` is required. A front door is trusted only by an exact subject the operator listed. Never logged: groups, subjects of accepted tokens, the token.
- **Observability:** the existing `auth_refused` and `auth_forbidden` lines (with `required` = `member`).

### Gaps

None.


## Amendment January 2026 — Per-document read access (ADR-115)

> Source: decision 94 (the user's decisions of January 2026); HLD revision 0.63.0, ADR-115 (a knowledge agent links a person's source-system accounts, receives GitHub and GitLab sharing-change notices, and lets its operator make it forget every remembered check). Counterparts: `components/CallerApi.md` (amendment January 2026: routes 41–45 and their layer), `components/KnowledgeQueryServer.md` (amendment January 2026: the handlers), `components/SourceAccess.md` (new: the notices' signature and token checks).
> Every rule below adds to the sections above. An agent without a `knowledge-agent` section serves none of the new routes.

### Purpose (addition)

Five more routes: two for a member to link and unlink their source-system accounts, checked like every member route; two for sharing-change notices from GitHub and GitLab, which carry no identity-provider token; and an operator route to forget remembered checks (45), checked like every operator route. A notice proves itself with the repository host's own signature or token, which Source Access checks against the secret the configuration names; this crate gives those two routes least role `none`, as it gives health, metrics and `GET /v1/knowledge/about`.

### Approach (addition)

The route table gains five rows; the checks are the existing ones. Rejected: *a fifth least role for signed notices* — the signature is a property of the body, which this crate never reads (step 2 of the Caller API runs before any body is read); the only place that reads the body, Source Access, checks it.

Assumptions (*chosen here*):

- **SA-CAUTH-PD-01** `POST /v1/knowledge/notices/github` and `POST /v1/knowledge/notices/gitlab` have least role `none`: a repository host cannot obtain a company token. They change no fact, answer no question and return `{}`; the worst a forged notice can do, were it accepted, is end the short memory early, which makes the next question check again.
- **SA-CAUTH-PD-02** `POST /v1/knowledge/accounts` and `DELETE /v1/knowledge/accounts/{system}` have least role `member`; `{system}` is a path parameter matched as one segment, so the table's entry is the declared path `/v1/knowledge/accounts/{system}`.
- **SA-CAUTH-PD-03** `POST /operator/knowledge/forget-checks` has least role `operator`, like `GET /operator/knowledge`: it changes what the agent remembers for every person, so only its operator may call it.

### Public Interface (additions)

**The route table** gains five rows, so it holds 45 entries; route 45 is an operator route, checked by `check_request` as every operator route:

| Method | Route path | Least role |
|---|---|---|
| `POST` | `/v1/knowledge/accounts` | member |
| `DELETE` | `/v1/knowledge/accounts/{system}` | member |
| `POST` | `/v1/knowledge/notices/github` | none |
| `POST` | `/v1/knowledge/notices/gitlab` | none |
| `POST` | `/operator/knowledge/forget-checks` | operator |

**Called by (additions).** `check_knowledge_request` — Caller API, routes 41 and 42; `required_role` — Caller API, routes 43 and 44 (returns `None`, no token read); `check_request` — Caller API, route 45.

### Internal Logic (additions)

19. `required_role` for the five rows as the table above; `check_knowledge_request` on routes 41 and 42 runs steps 13–17 unchanged; `check_request` on route 45 runs as on every operator route.
20. `check_knowledge_request` called for route 43 or 44 → `required_role` returns `None`, not `Member` → log `auth_forbidden` (WARN; `required` = `member`) and `Forbidden` (step 17); the Caller API never calls it for those routes (CallerApi.md amendment January 2026, step 45).

### Data Model

N/A — no table.

### Error Table (rows added)

No new code. Routes 41 and 42 use the rows of the amendment January 2026 (`AUTH_MISSING`, `AUTH_INVALID`, `AUTH_FORBIDDEN`, `AUTH_KEYS_UNAVAILABLE`); route 45 uses the operator rows of the sections above; routes 43 and 44 refuse through the Knowledge Query Server's `KQ_NOTICE_REJECTED`:

| Condition | Status | Code | Response Body |
|---|---|---|---|
| No or malformed bearer on route 41, 42 or 45 (existing code) | 401 | `AUTH_MISSING` | `{"error": "a bearer token is required", "code": "AUTH_MISSING"}` |
| Route 45 with a caller's token; `check_knowledge_request` called for a `none` route (existing code; the second is a wiring fault) | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |

### Acceptance Criteria (Gherkin, added)

```gherkin
Feature: Caller Authentication — account, notice and forget-checks routes

  Scenario: The account routes are member routes
    When required_role("POST", "/v1/knowledge/accounts") is called
    Then it returns LeastRole::Member
    When required_role("DELETE", "/v1/knowledge/accounts/{system}") is called
    Then it returns LeastRole::Member

  Scenario: The notice routes need no token
    When required_role("POST", "/v1/knowledge/notices/github") is called
    Then it returns LeastRole::None
    When required_role("POST", "/v1/knowledge/notices/gitlab") is called
    Then it returns LeastRole::None

  Scenario: Forgetting checks is an operator route
    When required_role("POST", "/operator/knowledge/forget-checks") is called
    Then it returns LeastRole::Operator
    When check_request runs for it with a valid caller token
    Then it fails with AUTH_FORBIDDEN

  Scenario: A member with no role links accounts
    Given a valid token with sub "00u1", groups ["procurement"] and no caller or operator value
    When check_knowledge_request runs for POST /v1/knowledge/accounts
    Then it returns kind member and audience_groups ["procurement"]

  Scenario: Error — an expired token on the account route
    Given a token whose exp is 60 s in the past
    When check_knowledge_request runs for DELETE /v1/knowledge/accounts/{system}
    Then it fails with AUTH_INVALID

  Scenario: The table holds 45 entries
    When ROUTE_ROLES is counted
    Then it holds 45 entries
```

### Performance / Security / Observability (additions)

- **Security:** routes 41 and 42 admit only a verified company token; route 45 only an operator's; routes 43 and 44 admit any sender but change nothing unless Source Access verifies the host's signature or token. Never logged: the bodies of any of the five routes.
- **Observability:** the existing `auth_refused` and `auth_forbidden` lines.

### Gaps

None.

