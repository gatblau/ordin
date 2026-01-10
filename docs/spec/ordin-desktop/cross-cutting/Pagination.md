# SPEC: Pagination

> **Spec set:** ordin-desktop · **Phase:** 4 — Cross-cutting · **Status:** Approved (user, January 2026; Phase 6 passed) · **Mode:** greenfield · **derivedFromHld:** 0.63.0
> **Implemented in:** `ordin-types` module `ordin_types::paging` (`Page<T>`, `PageRequest`, `Cursor`, `PAGE_SIZE`, `CURSOR_MAX_CHARS`); each paged reader applies it to its own keyset query.

## Purpose

Fixes one page form for every listing ordin serves — Service API commands (over Tauri IPC and the Remote API), the Caller API and the agent profile's operator routes — and the rule for adapter reads that page through a provider. The form is wrightrun's (SA-PAGE-01): `{"items": [...], "next_cursor": "<opaque>" | null}`, page size 100, an opaque cursor of at most 1,024 characters, a malformed cursor refused with `SRV_BAD_CURSOR`. The screens page the journal and artefact lists through these commands (07 — *Pagination*).

## Approach

Keyset (cursor) pagination with an opaque cursor that is base64url of the JSON form of the last row's sort key, and a fixed page size, defined once in `ordin-types` so every reader imports the same type. Each listing names a total order (a sort key ending in a unique column), so a page boundary never skips or repeats a row while rows are added. Rejected: offset pagination, because an insert between two requests shifts every later page and the journal is append-heavy; rejected: a caller-chosen page size, because wrightrun's Caller API fixes 100 and one size keeps every surface alike.

## Applicability by program

| Program | Applies | How | Source |
|---|---|---|---|
| Desktop (D) | Yes | Service API listings over Tauri IPC; adapter reads (SharePoint, Confluence, GitHub through `gh`, Slack replies) follow the provider's own cursor inside the adapter. | SA-XC-10; SA-PAGE-01 |
| Personal server (P) | Yes | The same Service API listings over the Remote API. | SA-XC-10; SA-PAGE-01 |
| Agent profile (A) | Yes | Caller API and operator routes (runs, settings, jobs, notes, entries, work-folder files), wrightrun's form unchanged. | SA-XC-10; SA-PAGE-01; ADR-098 |

## Shared Context

Copied from SharedTypes.md (`ordin_types::paging`):

```rust
pub const PAGE_SIZE: usize = 100;
pub const CURSOR_MAX_CHARS: usize = 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Always serialised; `null` on the last page.
    pub next_cursor: Option<Cursor>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PageRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(String);

impl Cursor {
    /// base64url of the JSON form of the last row's sort key.
    pub fn encode<K: Serialize>(key: &K) -> Cursor;
    /// Refuses more than 1,024 characters, bad base64url, or JSON that is not a `K` (`SRV_BAD_CURSOR`).
    pub fn decode<K: serde::de::DeserializeOwned>(&self) -> Result<K, TypesError>;
}
```

`Page<T>.next_cursor` is the one `Option` serialised as `null` rather than omitted. `TypesError::BadCursor` has code `SRV_BAD_CURSOR`.

## Public Interface

The paged listings, with each one's stable order (enumerated from the component specs):

| Listing | Owner | Surface | Order (sort key → cursor) | Source |
|---|---|---|---|---|
| Journal entries `list_entries` (optional `run_id`, `kind`) | Journal and Artefact Store | Service API | `(recorded_at DESC, id DESC)` → `Cursor::encode(&(recorded_at, id))` | JournalAndArtefactStore.md step 2 |
| Artefacts `list_artefacts` | Journal and Artefact Store | Service API | `(created_at DESC, id DESC)` | JournalAndArtefactStore.md step 4 |
| Folder listing `list_folder` | File Service | Service API | the chosen sort column, then `path` → `Cursor::encode(&(sort_key_value, path))` | FileService.md steps 2–3 |
| Agent library search | Catalogue Service | Service API | rank, then name; keyset over `(rank, part_id)` | CatalogueService.md §G |
| Pending approvals `approval.list_pending` | Approval Service | Service API, Remote API, Caller API route 31 | `(created_at ASC, id ASC)` → `Cursor::encode(&(created_at, id))` | ApprovalService.md Internal Logic H |
| Telegram link messages `messages` | Telegram Service | Service API | newest first, `(sent_at, message_id)` | TelegramService.md |
| SharePoint children `sharepoint.list` | SharePoint Adapter | Service API | Graph's order; one Graph page of `$top=100` per call, `@odata.nextLink` carried in the cursor | SharePointAdapter.md step 4 |
| Confluence children `confluence.list` | Confluence Adapter | Service API | Confluence's order; `_links.next` carried in the cursor (base64 of the next relative link) | ConfluenceAdapter.md step 4 |
| Runs `GET /operator/runs` (Run Queue `list`) | Run Queue | Caller API route 12 | newest first by `seq` (`seq < :before ORDER BY seq DESC LIMIT 101`) → `Cursor::encode(&seq)` | RunQueue.md step 20 |
| Settings `GET /operator/scopes/{scope}/settings` | Caller API over Settings Store | Caller API route 13 | declared names sorted; cursor = the last name | CallerApi.md step 33 |
| Jobs `GET /operator/jobs` (Job Runner `list`) | Job Runner | Caller API route 16 | newest first, `(created_at, id) < (:c, :i)` → `Cursor::encode(&(created_at, id))` | JobRunner.md step 8 |
| Notes `GET /operator/scopes/{scope}/notes`; entries `…/entries` | Knowledge Service | Caller API routes 18, 21 | `(created_at DESC, item_id DESC)` → `Cursor::encode(&(created_at, item_id))` | KnowledgeService.md Internal Logic 6a |
| Work-folder files `GET /work-folder/files?prefix=&cursor=` | File Service | Caller API route 7 | by path (wrightrun *FileList*) | CallerApi.md |
| Duties `GET /operator/duties` | Caller API | Caller API route 26 | one page, `next_cursor` always `null` | CallerApi.md |

Request: Service API commands carry `"cursor": string | null` in their input (absent or `null` = first page); Caller API routes carry `?cursor=` in the query. Response: `Page<T>`.

Adapter-side reads that page (Slack replies, GitHub through `gh`, Microsoft Graph search and change tracking, Confluence search, mail providers) follow each provider's own cursor and are read to the end or to the caller's limit inside the adapter; they are not exposed as ordin cursors except where the table above says so.

### Example

```json
{"request_id":"0192a1c0-0d1e-7f20-8a31-425364758697","kind":"ok",
 "result":{"items":[{"id":"0192a1bf-77aa-7c10-9e2f-0a1b2c3d4e5f","kind":"run_started"}],
           "next_cursor":"WyIyMDI2LTEwLTE5VDA4OjE1OjMwLjEyM1oiLCIwMTkyYTFiZi03N2FhLTdjMTAtOWUyZi0wYTFiMmMzZDRlNWYiXQ"}}
```

The cursor decodes to `["2026-01-10T08:15:30.123Z","0192a1bf-77aa-7c10-9e2f-0a1b2c3d4e5f"]`, the last row's `(recorded_at, id)`.

## Internal Logic

1. The reader receives `cursor: Option<Cursor>`. `None` → first page. `Some(c)` → `c.decode::<K>()` with the listing's own key type `K`; failure (over 1,024 characters, bad base64url, JSON not a `K`) → `SRV_BAD_CURSOR`.
2. The reader runs one keyset query: the listing's filter, `AND (sort key) <` (or `>` for ascending) `the decoded key`, `ORDER BY` the full sort key (ending in a unique column), `LIMIT 101`.
3. It returns the first 100 rows as `items`. When a 101st row came back, `next_cursor = Cursor::encode(&key of the 100th row)`; otherwise `next_cursor = None`, serialised as `null`.
4. Adapter cursors (SharePoint, Confluence) wrap the provider's continuation link in the same `Cursor` type, so the screens treat every listing alike.
5. A client follows `next_cursor` until it is `null`; ordin's own operator client (`OperatorClient::pages`) does so and refuses a 200 body without `items`.
6. On the Remote API, `SRV_BAD_CURSOR` is answered by the Service API's `accept` step as status 422 (ServiceApi.md Internal Logic 2.9); on the Caller API, as 400 with wrightrun's body.

## Data Model

N/A — no table. Every keyset query relies on an index over its sort key, owned by the table's component (for example the Journal's index over `(recorded_at, id)`).

## Error Table

| Condition | Status | Code | Response Body |
|-----------|--------|------|---------------|
| Cursor over 1,024 characters, bad base64url, or wrong key shape (any reader) | `Err(permanent)` | `SRV_BAD_CURSOR` | `TypesError::BadCursor` |
| Service API: malformed cursor in a command's input | 422 on the Remote API / `Err(permanent)` | `SRV_BAD_CURSOR` | `{"request_id":…,"kind":"error","error":{"code":"SRV_BAD_CURSOR",…}}` |
| Caller API: unreadable cursor on an operator or work-folder route | 400 | `SRV_BAD_CURSOR` | `{"error": …, "code": "SRV_BAD_CURSOR"}` |
| SharePoint Adapter: malformed paging cursor | `Err(permanent)` | `SRV_BAD_CURSOR` | `{"error":"ordin couldn't load more of this list.","code":"SRV_BAD_CURSOR","action":"try_again"}` |
| Telegram Service: malformed page cursor | `Refused` (permanent) | `SRV_BAD_CURSOR` | Service reply `error` |

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Pagination

  Scenario: Happy path — two pages of journal entries
    Given a journal with 150 entries
    When the screens list entries with no cursor
    Then 100 items come back newest first and next_cursor is not null
    When the screens list entries with that next_cursor
    Then the remaining 50 items come back and next_cursor is null

  Scenario: Edge case — a row added between two pages is neither skipped nor repeated
    Given the first page of runs was read with next_cursor C
    When a new run is recorded
    And the second page is read with C
    Then no run of the first page appears again and no older run is missing

  Scenario: Edge case — an exact multiple of the page size
    Given a listing with exactly 100 rows
    When the first page is read
    Then 100 items come back and next_cursor is null

  Scenario: Error path — a tampered cursor
    When GET /operator/jobs?cursor=not-base64! is sent by an operator
    Then the answer is 400 with code SRV_BAD_CURSOR
```

## Performance, Security, Observability

- **Performance targets:** a page of 100 from an indexed keyset query answers within the surface's p95 (150 ms for a Service API command, A-03, A-46); the first page of a 100,000-file place answers from the File Service's index, not the disk (A-21).
- **Security:** a cursor is decoded and type-checked before use; it is never placed in SQL text (the decoded key binds as parameters); a cursor carries no secret.
- **Observability:** a refused cursor is logged with the command or route and the code, never the cursor text.

*Closed gaps and defects.* The two sort-key defects (Approval Service `list_pending`; Knowledge Service `notes` and `entries`) were resolved in those Component specs in the correction round of January 2026; the Knowledge Service uses `ordin_types::paging::Page<T>`.

## Gaps

None.
