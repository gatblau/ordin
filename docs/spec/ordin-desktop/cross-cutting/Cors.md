# SPEC: CORS

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-remote-api` (request pre-check, step 2 of its *Internal Logic*), `ordin-caller-api` (`build_router` step 4), `apps/desktop` (`tauri.conf.json` content security policy and the `main` capability). No shared helper: two checks of one line each, in two crates that do not depend on each other.

## Purpose

Records, for each of the three programs, why Cross-Origin Resource Sharing does not apply, and fixes the one behaviour that follows from that decision on the two HTTP surfaces ordin serves: a request carrying an `Origin` header is refused, and no response ever carries a CORS header. Browsers are not clients of ordin; the desktop's WebView talks to its own core over Tauri IPC only (HLD 07 — CORS, amended in revision 0.48.0; SA-XC-11).

## Approach

Refuse instead of configure. A CORS policy would only matter if a browser page were a legitimate caller, and none is: the Remote API is called by ordin desktops (a `reqwest` client, which sends no `Origin`), the Caller API by servers and the operator client, and the WebView by IPC. Refusing any request with an `Origin` header turns an accidental browser call (a page on the company network probing the server's private address) into a 403 before authentication or body reading. Rejected: an allow-list of origins, because no origin needs allowing and an allow-list is one more setting that could be widened; ignoring the header and simply sending no CORS headers, because the browser would still deliver simple requests (a `POST` with a form media type) and only hide the answer.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | **No** | No HTTP server. The WebView loads only the bundled assets (`frontendDist: ../ui/dist`); its content security policy allows no remote origin (`connect-src ipc: http://ipc.localhost`), and the `main` capability grants only the `ordin` plugin's Service API commands and event listening — no HTTP, file-system, shell or dialog permission. | SA-XC-11; OrdinDesktop.md (*Tauri configuration*, *Tauri capability*); HLD 07 — CORS |
| Personal server (P) | **No** (refusal enforced) | The Remote API serves no browser pages and sends no CORS header; a request with an `Origin` header → 403 `SRV_REFUSED`. | SA-XC-11; RemoteApi.md *Internal Logic* step 2 |
| Agent profile (A) | **No** (refusal enforced) | The Caller API is called by servers and the operator client; it sends no CORS header; a request with an `Origin` header → 403 `SRV_REFUSED`. | SA-XC-11; CallerApi.md `build_router` step 4 |

## Shared Context

- **SA-XC-11** (Phase 1A, copied): "Does not apply to any program. Desktop: no HTTP server; the WebView loads bundled assets only. Personal server: the Remote API serves no browser pages and sends no CORS headers; a request carrying an `Origin` header is refused with `SRV_REFUSED`, since only ordin desktops call it. Agent profile: the Caller API is called by servers, and sends no CORS headers."
- **SA-ENV-01 body (Remote API)**: `{"request_id": "<id>", "kind": "error", "error": {"code": "<CODE>", "message": "<sentence>"}}`; for a pre-check refusal no request id has been read, so `request_id` is omitted (RemoteApi.md Error Table row: `{"kind":"error","error":{"code":"SRV_REFUSED","message":"…"}}`).
- **SA-ENV-02 body (Caller API)**: exactly `{"error": "<sentence>", "code": "<CODE>"}`; the `SRV_REFUSED` sentence is "the request was refused" (CallerApi.md Error Table).
- **Remote API response headers on every answer** (RemoteApi.md): `Ordin-Api-Version: 1.0`, `Cache-Control: no-store`, `Content-Type: application/json` or `text/event-stream`, and no CORS header.
- **Desktop content security policy** (OrdinDesktop.md, copied): `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src ipc: http://ipc.localhost; worker-src 'self' blob:; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'`.

## Public Interface

No function is exported for this concern. The observable contract is:

| Surface | Request | Answer |
|---|---|---|
| Remote API, any route (`POST /v1/{module}/{verb}`, `GET /v1/events`, `POST /v1/pairing`) | carries an `Origin` header, any value | 403, `SRV_REFUSED`, SA-ENV-01 body, no `Access-Control-*` header |
| Caller API, any route (health and metrics included) | carries an `Origin` header, any value | 403, `SRV_REFUSED`, SA-ENV-02 body, no `Access-Control-*` header |
| Remote API and Caller API, `OPTIONS` (a browser preflight) | carries `Origin` | 403 `SRV_REFUSED` (the `Origin` check runs first) |
| Any response of either surface | — | never carries `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, `Access-Control-Allow-Headers`, `Access-Control-Allow-Credentials`, `Access-Control-Max-Age` or `Access-Control-Expose-Headers` |

Allowed origins, methods, headers, max-age and credentials: none — there is no CORS configuration key in 2D, the organisation profile or the agent configuration.

### Example

```
POST /v1/runs/list HTTP/2
Host: 10.20.4.17:8443
Origin: https://intranet.example.org
Authorization: Bearer <43 characters>
Content-Type: application/json

HTTP/2 403
ordin-api-version: 1.0
cache-control: no-store
content-type: application/json

{"kind":"error","error":{"code":"SRV_REFUSED","message":"…"}}
```

```
GET /v1/runs/0192a1c0-0d1e-7f20-8a31-425364758697 HTTP/1.1
Origin: https://example.org

HTTP/1.1 403 Forbidden
content-type: application/json

{"error": "the request was refused", "code": "SRV_REFUSED"}
```

## Internal Logic

1. **Remote API** (`ordin-remote-api`, pre-check before the key check): if the request has an `Origin` header, answer 403 `SRV_REFUSED` without reading the body or the `Authorization` header, and without counting a failed key check against the client address. RemoteApi.md names no log line for this pre-check refusal (its log lines are `rapi.listening`, `rapi.command`, `rapi.key_failed`, `rapi.stream*`, `rapi.off`, `rapi.bind_failed`, `rapi.event_dropped`), and the personal server exports no metric (SA-XC-04), so the refusal leaves no record on the server; the request never reached authentication.
2. **Caller API** (`ordin-caller-api`, router layer): if the request has an `Origin` header, answer 403 `SRV_REFUSED` before `check_request` verifies the token; record `TelemetryEvent::HttpRequest { route, status: 403 }` and `HttpRefusal { code: "SRV_REFUSED" }`, and log `request_refused` (INFO; `code`, `method`) as every Caller API refusal.
3. **Both surfaces**: no layer adds a CORS header; no `tower-http` CORS layer is installed. A build-time test asserts the router has no CORS layer by sending a request without `Origin` and checking that no response header name starts with `access-control-`.
4. **Desktop**: the WebView's requests to anything other than the IPC origin are blocked by the content security policy; Tauri refuses any command outside the `main` capability. Nothing in the core listens on a TCP port for the WebView.

## Data Model

N/A — no table; the concern holds no state.

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Remote API request carries an `Origin` header | 403 | `SRV_REFUSED` | `{"kind":"error","error":{"code":"SRV_REFUSED","message":"…"}}` |
| Caller API request carries an `Origin` header | 403 | `SRV_REFUSED` | `{"error": "the request was refused", "code": "SRV_REFUSED"}` |
| WebView attempts a fetch to a non-IPC origin | blocked by the WebView (no request leaves) | — | — (content security policy violation, console only) |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: CORS

  Scenario: Happy path — a desktop call without Origin is served without CORS headers
    Given a paired desktop with a valid desktop key
    When it sends POST /v1/runs/list with no Origin header
    Then the answer is 200
    And no response header name starts with "access-control-"

  Scenario: Edge case — a browser preflight is refused before authentication
    Given the Remote API is listening on 10.20.4.17:8443
    When an OPTIONS request to /v1/runs/list carries Origin "https://intranet.example.org" and no Authorization header
    Then the answer is 403 with code SRV_REFUSED
    And the failed-key counter for the client address is unchanged

  Scenario: Error path — a browser origin on the Caller API is refused
    Given a hosted agent's Caller API
    When GET /v1/runs/<run_id> is sent with Origin "https://example.org" and a valid caller token
    Then the answer is 403 with body {"error": "the request was refused", "code": "SRV_REFUSED"}
    And no CORS header is sent
```

## Performance, Security, Observability

- **Performance targets:** the check is one header lookup before any other work; it adds no measurable latency to the Remote API's 150 ms p95 target.
- **Security:** refusing `Origin` blocks cross-site request forgery from a page on the company network against the personal server's private address and against a hosted agent; it runs before the key check, so a browser cannot drive the failed-key lock-out of SA-XC-09. Cross-origin calls carry no credentials into ordin because none are accepted.
- **Observability:** Caller API — counter `HttpRefusal { code: "SRV_REFUSED" }` and log line `request_refused` (Telemetry.md, CallerApi.md). Remote API — the refusal appears with its code in the Remote API's request log; no metric on D or P (SA-XC-04).

## Gaps

None.
