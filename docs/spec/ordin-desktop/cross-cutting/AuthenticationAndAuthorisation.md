# SPEC: Authentication and Authorisation

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-113; promoted to Approved by the user January 2026) · amended January 2026 (ADR-115) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** no crate of its own. Desktop and personal server: `ordin-identity` (sign-ins and tokens), `ordin-organisation-profile` (profile signature), `ordin-command-bridge` with the Tauri capability of `apps/desktop` (layer 1), `ordin-rules` (layer 2), `ordin-crew-orchestrator` (layer 3), `ordin-remote-api` (desktop key, client certificates). Agent profile: `ordin-caller-auth` (token, roles, route table), `ordin-caller-api` (intake signature, own-runs rule, approver flag), `ordin-approval` (approver checks).

## Purpose

Fixes, in one place, who is let in and what they may do in each of ordin's three programs, so that every entry point (Tauri IPC, Remote API, Caller API, ordin hook, MCP bridge, chat channels) applies the same rules. ordin has no application-level login on the desktop: opening the application on the person's computer is the local trust boundary, and each external system is reached with the person's own sign-in (HLD 07 — *Authentication*). The personal server admits only the person's own paired desktops, by a desktop key, and the paired chat account. A hosted agent (agent profile) admits identity-provider tokens verified in ordin's own code, with two roles, caller and operator, and an approver flag (ADR-097, ADR-098).

## Approach

Authentication and authorisation are decided in fixed code at each boundary by the component that owns the boundary; this spec states the shared rules and the codes, and each component spec carries the implementation. Rejected: one central authentication crate in front of every transport, because the three transports have unrelated credential forms (none, a 32-byte desktop key, an identity-provider token), and a shared crate would be an HLD-impact candidate (2B-1 allows no new crate) with nothing to share but the error form. Rejected: an application password on the desktop, because 07 fixes the person's computer as the trust boundary and the keychain already guards every secret.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | No application login. External systems with the person's own sign-in (Identity and Credentials). Authorisation in three layers: Tauri capability `ordin:default` granted to window `main` only; the Rules Engine decides every tool request; the Crew Orchestrator decides every delegation against the charter's guardrails. | SA-XC-01; 07 — Authentication, Authorisation |
| Personal server (P) | Yes | As the desktop, plus the Remote API desktop key (32 random bytes, kept as SHA-256, compared in constant time), optional client certificates, the failed-key lockout, and chat pairing to one user identifier. | SA-XC-01; ADR-096; ADR-073 |
| Agent profile (A) | Yes | Identity-provider token on every route but `/healthz`, `/readyz`, `/metrics` and a signed intake; roles caller and operator from the configured claim; approvers by user or group; intake by its own token or an HMAC-SHA256 signature with its shared secret. | SA-XC-01; ADR-097; ADR-098 |

## Shared Context

Copied from the component specs named; a reader implements from this section alone.

**Identity and Credentials (`ordin-identity`).** Tokens and passwords live only in the OS keychain (`keyring` 3; macOS Keychain, Windows Credential Manager, Secret Service on Linux) or, on a server with no Secret Service, in the encrypted file `<ORDIN_DATA_DIR>/secrets/secrets.enc` (XChaCha20-Poly1305, mode 0600) whose key comes from `ORDIN_SECRETS_KEY_FILE` or `ORDIN_SECRETS_KEY_CREDENTIAL` (never both). A credential record never holds secret material (`CRED_SECRET_IN_RECORD`). An access token due to expire within 5 minutes is refreshed, one refresh per credential at a time; a refused refresh (`invalid_grant`, HTTP 400 or 401) marks the credential `needs_sign_in` and returns `CRED_SIGN_IN_REQUIRED`, and the run that needs it pauses (07).

**Organisation profile signature (`ordin-organisation-profile`).** One JSON document with a detached JWS, Ed25519 (`ed25519-dalek` 3.0.0), verified against at most two public keys the installer carries in `installer.json` (SA-SIGN-01). A key's `kid` is the first 16 hex characters of SHA-256 of its raw 32 bytes. A profile that fails verification is ignored and the last verified profile is kept (2D precedence step 4).

**Remote API key (`ordin-remote-api`).** `Authorization: Bearer <43 characters of base64url>` = 32 bytes from `OsRng`, sent once in `PairReply.desktop_key`, stored only as `desktop_key_hash` (SHA-256). Pairing code: 8 symbols of `[A-HJ-NP-Z2-9]`, valid 10 minutes; 5 wrong codes in 10 minutes stop pairing until the setup command starts a new one; at most 10 paired desktops (`max_paired_desktops`). TLS 1.3 only; client certificates verified with rustls' `WebPkiClientVerifier` when `server.client_certificate_required` is `true`.

**Caller Authentication (`ordin-caller-auth`)**, copied from CallerAuthentication.md:

```rust
pub struct AuthConfig {            // agent configuration `auth` (kebab-case keys)
    pub issuer: String,            // https URL
    pub audience: String,
    pub jwks_url: String,          // https (http only for localhost / 127.0.0.1)
    pub role_claim: String,
    pub operator_value: String,
    pub caller_value: String,
    pub algorithms: Vec<String>,   // default ["RS256"]
}
pub enum Role { Caller, Operator }             // Operator > Caller
pub enum LeastRole { None, Caller, Operator }
pub struct Principal { pub subject: String, pub role: Role, pub approver: bool, pub groups: Vec<String> }
```

| Constant | Value |
|---|---|
| `KEY_SET_SECONDS` | 300 |
| `KEY_COOLDOWN_SECONDS` | 30 |
| `KEY_FETCH_TIMEOUT_SECONDS` | 5 |
| `KEY_RETRY_SECONDS` | 10 |
| `LEEWAY_SECONDS` | 30 |
| `MAX_TOKEN_BYTES` | 16,384 |

**Intake signature (`ordin-caller-api`).** `SIGNATURE_PREFIX` = `sha256=`; `TIMESTAMP_TOLERANCE_SECONDS` = 300; `MIN_SECRET_CHARS` = 32; header name from the intake's `signature_header` (default `X-Wrightrun-Signature`).

## Public Interface

Entry points and their check, one row per entry point:

| Entry point | Program | Credential | Checked by | Refusal |
|---|---|---|---|---|
| Tauri IPC `ordin_<module>_<verb>` | D | none; window `main` holds capability `ordin:default` | Tauri runtime, Command Bridge | command unreachable from any other window |
| `POST /v1/{module}/{verb}`, `GET /v1/events`, `POST /v1/pairing/end` | P | desktop key | Remote API step 3 | 401 `SVC_UNAUTHENTICATED`; 429 `DESK_ADDRESS_LOCKED` |
| `POST /v1/pairing` | P | pairing code | Remote API step 5 | 403 `DESK_CODE_INVALID`, `DESK_CODE_EXPIRED`, `DESK_PAIRING_STOPPED`, `DESK_TOO_MANY` |
| ordin hook request line | D P A | per-session secret, HMAC-SHA256 | Hook Relay | `deny` with `HOOK_UNAUTHENTICATED`, `HOOK_PEER_REFUSED` |
| Caller API routes 1–10, 12–27, 31–32 | A | identity-provider token | `ordin_caller_auth::check_request` | `AUTH_MISSING`, `AUTH_INVALID`, `AUTH_NO_ROLE`, `AUTH_FORBIDDEN`, `AUTH_KEYS_UNAVAILABLE` |
| `POST /intakes/{name}` (route 11) | A | intake token or signature | Caller API steps 25, 30 | `INTAKE_UNAUTHENTICATED`, `INTAKE_FORBIDDEN`, `INTAKE_UNAVAILABLE` |
| `/healthz`, `/readyz`, `/metrics` (28–30) | A | none | — | — |
| Tool request from a harness | D P A | the session | Rules Engine | verdict `deny` with a `RULE_` code |
| Approval press | D P A | origin (`PressOrigin`) | Approval Service | `APPR_NOT_AN_APPROVER`, `APPR_ASKER_MAY_NOT_APPROVE`, `APPR_OPERATOR_NOT_ALLOWED` |

Functions (copied signatures):

```rust
// ordin-caller-auth
pub const ROUTE_ROLES: &[(&str, &str, LeastRole)];
pub fn required_role(method: &str, route_path: &str) -> Result<LeastRole, AuthError>;
pub fn require_role(principal: &Principal, role: Role) -> Result<(), AuthError>;
pub fn may_act(principal: &Principal, started_by: &ordin_types::RunStarter) -> bool;
impl Verifier {
    pub fn new(auth: AuthConfig, approvers: Option<ApproversConfig>) -> Result<Self, AuthError>;
    pub async fn verify(&self, authorization_header: Option<&str>) -> Result<Principal, AuthError>;
}
pub async fn check_request(verifier: &Verifier, method: &str, route_path: &str,
                           authorization_header: Option<&str>) -> Result<Option<Principal>, AuthError>;
// ordin-caller-api
pub fn verify_signature(secret: &IntakeSecret, body: &[u8], signature: Option<&str>, timestamp: Option<&str>,
                        signed_timestamp: bool, now_unix: i64) -> bool;
```

### Example

Caller API, a caller token on an operator route:

```
GET /operator/runs HTTP/1.1
Authorization: Bearer eyJhbGciOiJSUzI1NiIsImtpZCI6ImsxIn0.…

HTTP/1.1 403 Forbidden
Content-Type: application/json

{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}
```

Remote API, a wrong desktop key:

```
POST /v1/runs/start HTTP/2
Authorization: Bearer AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA

HTTP/2 401
{"request_id":null,"kind":"error","error":{"code":"SVC_UNAUTHENTICATED","message":"…"}}
```

## Internal Logic

**Desktop (D).**
1. The desktop program grants capability `ordin:default` (every `ordin:allow-<ipc_name>`) to window `main` only (`apps/desktop/capabilities/main.json`); no Tauri plugin is granted to the WebView. No command takes a file path, URL or key from the screens as an authority (CommandBridge.md *Security*).
2. Every sign-in to an external system goes through Identity and Credentials (browser with loopback redirect, device flow, or password kept in the keychain for the company mail server over TLS only). A refused mail password is never retried. Model vendors' sign-in stays with each harness (ADR-044); ordin only reads its state.
3. Before each use, a caller asks Identity and Credentials for an access token; refresh happens within 5 minutes of expiry; a refused refresh pauses the run with `CRED_SIGN_IN_REQUIRED` (action `sign_in_again`).
4. Every tool request passes the Rules Engine (layer 2), which decides from the package declaration and the profile's policy; a decision the engine cannot reach is `deny` (`fail_closed`).
5. Every delegation in a crew passes the Crew Orchestrator (layer 3) against the charter's guardrails.
6. Every externally visible action needs an approval card built by the application; a typed chat message is never an approval.

**Personal server (P).** Steps 2–6 as the desktop, and:
7. The Remote API listens only on loopback or a private address with a port above 1024 (`RAPI_ADDRESS_NOT_PRIVATE` otherwise), TLS 1.3 only; the profile may forbid listening (`RAPI_LISTENING_FORBIDDEN`).
8. Key check per request: a locked client address → 429 `DESK_ADDRESS_LOCKED` without reading the key; else decode the bearer value to 32 bytes, SHA-256 it, and compare with every paired row's hash using `subtle::ConstantTimeEq`, visiting every row whatever the result; no match → 401 `SVC_UNAUTHENTICATED` and count the failure.
9. Failed-check lockout: the tenth failure from one client address within 60 s locks it for 10 minutes and journals `remote_api_address_locked` with no key material. Locks live in memory and end with the program.
10. An approval press from a desktop goes to the Approval Service as `PressOrigin::PairedDesktop { computer_name }`, checked as a chat button press is.
11. The chat channel answers only the paired user identifier in a private chat; pairing changes only through the setup command over SSH.

**Agent profile (A).**
12. At start, the Caller API checks that every route of its router is in `ROUTE_ROLES`; a missing route stops start with `AUTH_ROUTE_UNMAPPED` (exit 1). `Verifier::new` refuses an algorithm outside the accepted list with `AUTH_ALGORITHM`.
13. Per request: `required_role(method, path)` (HEAD → GET, query string dropped); `LeastRole::None` → no token read. Otherwise `verify`: header absent, not `Bearer`, or over 16,384 bytes → `AUTH_MISSING`; JWT decode with the configured algorithms, issuer, audience, 30 s leeway and required claims `exp`, `iss`, `aud`, `sub` → failure `AUTH_INVALID`; role claim holding neither configured value → `AUTH_NO_ROLE`; role below the route's least role → `AUTH_FORBIDDEN`.
14. Key set: cached 300 s; an unknown `kid` refetches at most once per 30 s; fetch limit 5 s; after a failed fetch, retry no sooner than 10 s; keys unreadable and no kept set younger than 300 s holds the `kid` → 503 `AUTH_KEYS_UNAVAILABLE` (retryable).
15. Own-runs rule: a caller acts only on runs and threads it started (`may_act`); an operator acts on all.
16. Approver routes 31 and 32: `Principal.approver` false → `AUTH_FORBIDDEN`; the Approval Service refuses the asker approving their own outbound action (`APPR_ASKER_MAY_NOT_APPROVE`) and an operator deciding while approvers are named (`APPR_OPERATOR_NOT_ALLOWED`).
17. Intake: with a secret and the signature header present → `verify_signature` over the body, or over `<timestamp>.<body>` when a timestamp header is configured (timestamp `^\d{1,12}$`, within 300 s); false → 401 `INTAKE_UNAUTHENTICATED`, and a failed signature is never followed by a token try. Otherwise, when the intake accepts tokens, `verify` with caller or operator role. An intake may start its one command only.

**Key rotation.**
18. Organisation profile: IT publishes the second key in `installer.json` (up to two), signs with it, then removes the first in a later installer; a `kid` matching no installer key → `PROF_KEY_UNKNOWN` and the last verified profile stays.
19. Desktop key: no rotation in place; **Disconnect this computer** or `ordin-server setup pair-desktop --end <computer name>` ends the pairing, and a new pairing issues a new key.
20. Identity-provider keys: followed through the key-set cache rules of step 14; no restart is needed.
21. Hook session secret: a new secret per session from the OS random source (`HOOK_RANDOM_UNAVAILABLE` when none).

**Logging.** No token, key, signature, password or secret appears in a log line, a journal record or an error message; refusals log the code and the route only.

## Data Model

N/A — this concern owns no table. Tables that hold authentication state are owned by their components: `desktop_pairings`, `desktop_pairing_codes`, `desktop_pairing_attempts` (Remote API); credential records (Identity and Credentials); `approvals`, `approval_buttons` (Approval Service). None holds secret material.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Remote API: missing, malformed or unknown desktop key | 401 | `SVC_UNAUTHENTICATED` | `{"request_id":null,"kind":"error","error":{"code":"SVC_UNAUTHENTICATED",…}}` |
| Remote API: client address locked after 10 failed key checks | 429 | `DESK_ADDRESS_LOCKED` | as above with `DESK_ADDRESS_LOCKED` |
| Remote API: pairing code malformed or not pending | 403 | `DESK_CODE_INVALID` | as above |
| Caller API: no or malformed `Authorization`, or token over 16,384 bytes | 401 | `AUTH_MISSING` | `{"error": "a bearer token is required", "code": "AUTH_MISSING"}` |
| Caller API: token fails signature, expiry, issuer, audience or claim checks | 401 | `AUTH_INVALID` | `{"error": "the request's token was not accepted", "code": "AUTH_INVALID"}` |
| Caller API: token role claim holds neither value | 403 | `AUTH_NO_ROLE` | `{"error": "the token carries no role of this agent", "code": "AUTH_NO_ROLE"}` |
| Caller API: role below the route's least role, or approver flag false on routes 31–32 | 403 | `AUTH_FORBIDDEN` | `{"error": "the token's role does not permit this request", "code": "AUTH_FORBIDDEN"}` |
| Caller API: identity provider's keys unreadable | 503, retryable | `AUTH_KEYS_UNAVAILABLE` | `{"error": "the identity provider's keys cannot be read; try again later", "code": "AUTH_KEYS_UNAVAILABLE"}` |
| Caller API: a route missing from `ROUTE_ROLES` at start | exit 1 | `AUTH_ROUTE_UNMAPPED` | `error[AUTH_ROUTE_UNMAPPED]: no role is stated for this route (…)` |
| Intake: no valid token or signature, or stale timestamp | 401 | `INTAKE_UNAUTHENTICATED` | `{"error": "the event carries no valid token or signature", "code": "INTAKE_UNAUTHENTICATED"}` |
| Hook request MAC mismatch | `deny` | `HOOK_UNAUTHENTICATED` | hook reply `deny` |
| Desktop / server: refresh refused, credential needs a new sign-in | `Err(permanent)` | `CRED_SIGN_IN_REQUIRED` | `{"code":"CRED_SIGN_IN_REQUIRED","action":"sign_in_again"}` |
| Profile signed with a key the installer does not carry | `Err(permanent)` → `Refused` | `PROF_KEY_UNKNOWN` | `{"refused":{"code":"PROF_KEY_UNKNOWN"}}` |
| Approval press by the asker on their own outbound action | `Err(permanent)` / 403 on the Caller API | `APPR_ASKER_MAY_NOT_APPROVE` | Service reply `error` / `{"error": …, "code": "APPR_ASKER_MAY_NOT_APPROVE"}` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Authentication and Authorisation

  Scenario: Happy path — an operator reads the run list on a hosted agent
    Given a hosted agent whose auth role_claim is "groups" and operator_value is "agent-ops"
    And a token signed by a key in the identity provider's key set, with groups ["agent-ops"]
    When GET /operator/runs is sent with that token
    Then the answer is 200 with a page of runs

  Scenario: Happy path — a paired desktop sends a command
    Given a personal server with one paired desktop "Asha's laptop"
    When the desktop sends POST /v1/runs/start with its desktop key
    Then the key's SHA-256 matches the stored hash in a constant-time comparison
    And the command runs with origin PairedDesktop "Asha's laptop"

  Scenario: Edge case — the identity provider rotates its signing key
    Given the key set cached 100 seconds ago holds kid "k1" only
    When a request arrives with a token whose kid is "k2"
    Then the key set is fetched again within 5 seconds
    And the request is accepted when "k2" is in the new set

  Scenario: Edge case — health routes need no token
    When GET /healthz is sent with no Authorization header
    Then the answer is 200

  Scenario: Error path — a caller token on an operator route
    Given a token whose role is caller
    When GET /operator/runs is sent with that token
    Then the answer is 403 with code AUTH_FORBIDDEN

  Scenario: Error path — ten wrong desktop keys from one address
    Given nine failed key checks from 10.20.4.30 within the last minute
    When a tenth request with a wrong key arrives from 10.20.4.30
    Then that address is refused with 429 DESK_ADDRESS_LOCKED for 10 minutes, even with a right key
```

## Performance, Security, Observability

- **Performance targets:** token verification with a cached key set adds at most 2 ms p95 to a Caller API request; the desktop-key check adds at most 1 ms p95 for 10 paired desktops; a key-set fetch is bounded by 5 s.
- **Security:** credentials only in the keychain, the Secret Service or the encrypted 0600 file; desktop keys stored as hashes and compared in constant time; tokens, keys, signatures and passwords never logged; every authorisation decision in fixed code, never by a model; a request with an `Origin` header is refused (see `Cors.md`).
- **Observability:** journal entries `remote_api_address_locked`, `remote_api_desktop_paired`, `remote_api_desktop_unpaired`; log lines `auth_route_unmapped` (ERROR), `intake_refused` (WARN; `intake`, `reason`); identity log line `cred.refreshed`; agent-profile counters as the Telemetry closed list (see `Metrics.md`).

## Gaps

None.

## Amendment January 2026 — Knowledge agents (ADR-113)

Entry points added, one row each (copied from CallerAuthentication.md and CallerApi.md, amendments of January 2026). They exist only on an agent-profile server whose agent configuration has a `knowledge-agent` section:

| Entry point | Program | Credential | Checked by | Refusal |
|---|---|---|---|---|
| `POST /v1/knowledge/query`, `GET /v1/knowledge/contents`, `GET /v1/knowledge/contents/index.md`, `GET /v1/knowledge/changes`, `POST /mcp/knowledge` (routes 34–36, 38, 39) | A | identity-provider token of a person, a service or a front door; least role `Member` | `ordin_caller_auth::check_knowledge_request` (`verify_knowledge`), then the Knowledge Query Server's audience check | `AUTH_MISSING`, `AUTH_INVALID`, `AUTH_FORBIDDEN`, `AUTH_KEYS_UNAVAILABLE`; then `KQ_NOT_IN_AUDIENCE`, `KQ_NOT_A_FRONT_DOOR` |
| `GET /v1/knowledge/about` (route 37) | A | none (least role `None`); returns no content, only what a desktop needs before it can obtain a token | — | — |
| `POST /operator/memory/{verb}` (route 33; added by this amendment because the ADR-111 amendment of CallerApi.md did not reach this table) | A | identity-provider token, operator role | `ordin_caller_auth::check_request` | as routes 1–10 |
| `GET /operator/knowledge` (route 40) | A | identity-provider token, operator role | `ordin_caller_auth::check_request` | as routes 1–10 |
| `POST /v1/knowledge/accounts`, `DELETE /v1/knowledge/accounts/{system}` (routes 41, 42; *added January 2026, ADR-115*) | A | identity-provider token of a person in an audience; least role `Member`; a front door or service identity is refused | `ordin_caller_auth::verify_knowledge` | `KQ_NOT_IN_AUDIENCE`, `KQ_BAD_ACCOUNTS` |
| `POST /v1/knowledge/notices/github`, `POST /v1/knowledge/notices/gitlab` (routes 43, 44; *added January 2026, ADR-115*) | A | no token (least role `None`); the GitHub HMAC-SHA256 signature or the GitLab token, compared in constant time; mounted only when the secret is configured | Source Access (`accept_notice`) | `KQ_NOTICE_REJECTED` |
| `POST /operator/knowledge/forget-checks` (route 45; *added January 2026, ADR-115*) | A | identity-provider token, operator role | `ordin_caller_auth::check_request` | as routes 1–10 |
| Asker statement on a front door's query (*added January 2026, ADR-115*) | A | carried on the front door's own service-identity request; trusted only from a subject listed in `knowledge-agent.front-doors[]`, issued within the last 30 s | the proxy (`verify_knowledge`, then the statement's bounds) | `KQ_ASKER_STATEMENT_INVALID` |
| SharePoint delegation on a person's query (*added January 2026, ADR-115*) | D P → A | a Microsoft token whose audience is the front door's own app; accepted only from a member, used once for an on-behalf-of exchange, never stored | the front door | `KQ_DELEGATION_NOT_ALLOWED` (from a front door or service) |
| Desktop or personal server asking a front door | D P | the person's company sign-in (identity-provider token for the front door's audience), obtained through Identity and Credentials | the front door, as above | `KQ_SIGN_IN_NEEDED`, `KQ_NOT_IN_AUDIENCE` shown in one sentence |
| Front door asking a proxy | A | the front door's own service-identity token (client credentials from `knowledge-agent.service-identity`) | the proxy: `verify_knowledge` with the front door's subject in `knowledge-agent.front-doors[]`, and the request's `access_group` among the proxy's audiences | `KQ_NOT_A_FRONT_DOOR`, `KQ_NOT_IN_AUDIENCE` |

Authorisation for knowledge is by **audience** (ADR-113): a knowledge agent reads only repositories its operator states every member of its audience groups may read (`audience-may-read: true`), and serves only askers in one of those groups; the asker's **access group** is the first configured audience group the asker is in, and the cache is kept per access group. Per-person checks against each asker's Git rights are not made in this revision. `LeastRole` gains `Member`, used only by the knowledge routes; a token with the caller or operator role but in no audience group is refused on them, and a `Member` grants nothing on routes 1–33. *Amended January 2026 (ADR-115):* the audience still admits an asker, but no longer decides what they receive: a source only part of the audience may read is checked per person at question time, against the system holding the document, by the agent holding the fact, before ranking; no reader list is stored, and a check that cannot be made withholds (Knowledge inherits its readers).

```gherkin
Feature: Knowledge routes' authentication (ADR-113)

  Scenario: An asker outside every audience group gets no facts
    Given a knowledge agent with audiences ["procurement"]
    And a valid token whose groups claim holds ["finance"]
    When POST /v1/knowledge/query is sent
    Then the answer is 403 with code KQ_NOT_IN_AUDIENCE and holds no fact

  Scenario: The about route needs no token and holds no content
    When GET /v1/knowledge/about is sent with no Authorization header
    Then the answer is 200 with the agent's name, issuer, token audience and sources-record statement
    And it holds no fact, thing name or repository path

  Scenario: A member token does not open a caller route
    Given a token in the audience group but with no caller role
    When POST /v1/runs is sent with it
    Then the answer is 403 with code AUTH_NO_ROLE, as for any token whose role claim holds neither value
```

### Gaps

None.
