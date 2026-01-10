# SPEC: Brand

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-brand` · **Build phase:** 3 · **Programs:** D
> **Dependencies:** Organisation Profile, Localisation, Journal

## Table of contents

- [Purpose](#purpose)
- [Approach](#approach)
- [Shared Context](#shared-context)
- [Public Interface](#public-interface)
- [Internal Logic](#internal-logic)
- [Data Model](#data-model)
- [Error Table](#error-table)
- [Acceptance Criteria (Gherkin)](#acceptance-criteria-gherkin)
- [Performance, Security, Observability](#performance-security-observability)
- [Gaps](#gaps)

## Purpose

Brand applies the **run layer** of the brand pack (ADR-041): the product name, the coordinator's display name, logos, icon set, design tokens, fonts, links, About and legal text that the desktop shows inside its window. It reads the brand section of the active organisation profile, downloads and verifies the pack's assets, refuses a pack whose colour tokens fail WCAG 2.2 AA contrast or that tries to set a colour the application keeps fixed, falls back to the build's own pack in that case, and keeps the refusal reason for IT (02:535). It owns the active brand pack (02:536; 2C `BrandPack`). The build layer (application name, icon, bundle identifier, installer, signing identity, update feed) is the build pipeline's and is out of scope here.

## Approach

Brand is a small state machine per received pack (`received → contrast_checked → active`, or `received → refused`, 2C), driven by the Organisation Profile's watch channel and persisted in `brand_packs`; the screens read the active pack through two Service API commands and restyle from design tokens only. The build's own pack is compiled into the crate and stored as a row like any other, so every screen reads one shape. Contrast is computed in fixed code with the WCAG 2.2 relative-luminance formula over a fixed list of token pairs for both light and dark themes. Rejected: *serving asset URLs for the WebView to fetch* — the desktop's WebView makes no network call (07 — Remote API row; ADR-096) and loads bundled assets only; *accepting any token name* — ADR-041 rejects free styling because it can hide approval cards, so token names are limited to four namespaces and the fixed safety colours cannot be set.

## Shared Context

### Types from Phase 2C (copied verbatim)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>); // RFC 3339, ms, Z

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BrandPackId(pub uuid::Uuid); // UUIDv7, else ID_NOT_UUIDV7

/// SHA-256 as 64 lowercase hexadecimal characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BrandPack {
    pub id: BrandPackId,
    pub brand_identifier: String,
    pub version: String,
    pub source: BrandPackSource,
    pub strings: Vec<BrandStrings>,
    pub logo_light_path: String,
    pub logo_dark_path: String,
    pub icon_set_light_path: String,
    pub icon_set_dark_path: String,
    /// Token name → value, as the pack declares them.
    pub design_tokens: std::collections::BTreeMap<String, String>,
    pub font_file_paths: Vec<String>,
    pub help_url: String,
    pub support_url: String,
    pub privacy_url: String,
    pub state: BrandPackState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
    pub received_at: Timestamp,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activated_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct BrandStrings {
    pub locale: String,
    pub product_name: String,
    pub coordinator_name: String,
    pub about_text: String,
    pub legal_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrandPackSource { Build, OrganisationProfile }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrandPackState { Received, ContrastChecked, Active, Refused }
```

| Field (2C) | Rule | Error code |
|---|---|---|
| brand_identifier | 1–64 chars `[a-z0-9-]` (LD-07) | `BRAND_ID_INVALID` |
| version | semantic version `MAJOR.MINOR.PATCH` | `BRAND_VERSION_INVALID` |
| strings | at least one entry; one entry per locale; `locale` a BCP 47 tag | `BRAND_STRINGS_MISSING`, `BRAND_LOCALE_DUPLICATE` |
| strings.product_name, coordinator_name | 1–60 chars | `BRAND_STRING_INVALID` |
| strings.about_text, legal_text | 1–8000 chars | `BRAND_STRING_INVALID` |
| design_tokens | at most 500 entries; value at most 256 chars | `BRAND_TOKENS_INVALID` |
| design tokens' colour pairs | pass the contrast check (ADR-041) or the pack is `refused` | `BRAND_CONTRAST_FAILED` |
| help_url, support_url, privacy_url | `https` URL ≤ 2048 bytes | `BRAND_URL_INVALID` |
| refusal_reason | present if and only if `state = refused`; ≤ 500 chars | `BRAND_REFUSAL_REASON_MISMATCH` |

State machine (2C): `— → received` (pack received with a profile); `received → contrast_checked` (check passes); `contrast_checked → active` (Brand applies it); `received → refused` (check fails; build's pack kept); `active →` superseded when a newer pack is activated (new row active). Storage: SQLite table `brand_packs` in the journal database, owned by Brand; asset files under the application data root, the row holds their paths.

### Organisation Profile section consumed (Organisation Profile spec; schema checks already done there)

```rust
pub struct BrandSection {                                       // profile key `brand`
    pub brand_identifier: String,          // [a-z0-9-]{1,64}
    pub version: String,                   // MAJOR.MINOR.PATCH
    pub strings: BTreeMap<String, BrandStringsEntry>,           // locale tag → strings
    pub logos: ThemedAsset,                // light and dark
    pub icon_set: ThemedAsset,
    pub design_tokens: BTreeMap<String, String>,
    pub fonts: Vec<BrandAsset>,
    pub links: BrandLinks,
    pub legal_text: BTreeMap<String, String>,                   // locale tag → text
}
pub struct BrandStringsEntry { pub product_name: String, pub coordinator_name: String, pub about_text: String }
pub struct ThemedAsset { pub light: BrandAsset, pub dark: BrandAsset }
pub struct BrandAsset { pub url: String /* https ≤ 2048 bytes */, pub sha256: Sha256Hex, pub media_type: String }
pub struct BrandLinks { pub help_url: String, pub support_url: String, pub privacy_url: String }
// OrganisationProfileDocument.brand: Option<BrandSection>
// OrganisationProfileService::current() -> Arc<ProfileSnapshot>; ::subscribe() -> watch::Receiver<Arc<ProfileSnapshot>>
// ProfileSnapshot { document, source: SnapshotSource::{ Verified { profile_id, profile_version, activated_at }, Defaults } }
```

Organisation Profile has already checked: schema, `https` URLs ≤ 2048 bytes, `sha256` form, ≤ 500 tokens of ≤ 256 chars, and the string lengths of LD-07. It does not fetch assets or check contrast.

### Localisation interface consumed

```rust
impl Localisation {
    pub fn active(&self) -> ActiveLocale;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<ActiveLocale>;
}
pub struct ActiveLocale { pub locale: String, pub source: LocaleSource, pub direction: TextDirection, pub fallback_chain: Vec<String> }
```

### Configuration (2D)

| Source | Key | Use |
|---|---|---|
| Organisation profile (2D-2) | `brand.*` (`strings.<locale>`, `logos`, `icon_set`, `design_tokens`, `fonts`, `links`, `legal_text`) | run layer; default the build's own brand; WCAG 2.2 AA contrast |
| Build input | the build's brand pack folder | compiled in (see *Names chosen here*) |
| SA-HTTP-02 | HTTP client | `reqwest` 0.13.5 with rustls and `rustls-platform-verifier` 0.7.1; system proxy (A-39) |

### Names chosen here

| Name | Kind | Reason |
|---|---|---|
| `crates/ordin-brand/build-brand/` holding `brand.json` (a `BrandSection` with `file:` asset paths relative to the folder) and the asset files | build input folder | the build pipeline replaces this folder to produce a branded build (ADR-041 build layer); default holds ordin's own brand |
| `<application data root>/brand/<brand_identifier>/<version>/<sha256>.<ext>` | asset path | content-addressed; one folder per pack version |
| Token namespaces `color.`, `font.`, `space.`, `radius.` | design-token name rule | ADR-041: every colour, font and spacing from tokens; nothing else styleable |
| Required colour tokens `color.<role>.<theme>`, role ∈ {`text`, `text-muted`, `background`, `surface`, `accent`, `on-accent`, `border-control`, `focus`}, theme ∈ {`light`, `dark`} | token names | the pairs of the contrast check |
| Fixed colour prefixes `color.danger.`, `color.waiting.`, `color.run-state.`, `color.approval.` | refused token names | ADR-041: danger and waiting colours, run-state colours and approval cards are fixed by the application |
| `brand.get`, `brand.get_asset` | Service API commands | SA-API-01 form |
| `brand_changed` | Service API event kind | screens restyle when a pack becomes active (G-1 leaves events open) |

### Reconciliation assumptions

- **SA-RECON-01** The `brand_packs` table lives in the Journal's datastore through a Journal edge (D4, R20): `BrandConfig` carries the `Datastore` and the crate exports `migrations()`.
- **SA-RECON-02** The Service API catalogue takes `brand.get`, `brand.get_asset` and the `brand_changed` payload `{brand_identifier, version, source}` as written (R1, R6).
- **SA-RECON-03** A pack carries only the fixed `BrandStrings` fields; there is no per-message-identifier override map (row 84).
- **SA-RECON-04** The desktop program forwards `brand_changed` and passes `coordinator_name()` to the Ordin Host as `OrdinHostPolicy.coordinator_name`, at start and through `OrdinHost::set_policy` on each brand change (rows 85, 86).

## Public Interface

```rust
use std::sync::Arc;

pub struct BrandConfig {
    /// The Journal's `Datastore` (D4, R20); the desktop is SQLite only and this crate uses `datastore.sqlite()`.
    pub datastore: ordin_journal::Datastore,
    /// `<application data root>`; assets go under `brand/`.
    pub data_root: std::path::PathBuf,
    /// Shared outbound client (SA-HTTP-02), built by the program.
    pub http: reqwest::Client,
}

pub struct Brand { /* private */ }

impl Brand {
    /// Installs the build pack row if missing, restores the active row, then follows the profile.
    pub async fn start(
        config: BrandConfig,
        profile: Arc<OrganisationProfileService>,
        localisation: Arc<Localisation>,
    ) -> Result<Arc<Brand>, BrandError>;

    /// The pack in force (never absent: the build pack is the floor).
    pub fn active(&self) -> Arc<BrandPack>;

    /// Strings of the active pack for the active locale, by the locale's fallback chain,
    /// then the pack's first entry (`strings[0]`).
    pub fn strings(&self) -> BrandStrings;

    /// The coordinator's display name in the active locale (A-27: the desktop program passes it to the Ordin Host as `OrdinHostPolicy.coordinator_name`).
    pub fn coordinator_name(&self) -> String;

    /// Bytes of one asset of the active pack.
    pub async fn asset(&self, which: AssetRef) -> Result<AssetBytes, BrandError>;

    /// The newest pack received from a profile that was refused, with its reason, if it is newer
    /// than the active one (for Settings › Diagnostics and **Send to IT**).
    pub fn last_refusal(&self) -> Option<Arc<BrandPack>>;

    /// Notified each time the active pack changes.
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<Arc<BrandPack>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssetRef { LogoLight, LogoDark, IconSetLight, IconSetDark, Font { index: u8 } }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AssetBytes {
    pub media_type: String,
    /// Standard base64 of the file.
    pub base64: String,
}

#[derive(Debug, thiserror::Error)]
pub enum BrandError {
    #[error("asset could not be fetched")] AssetFetchFailed,      // BRAND_ASSET_FETCH_FAILED, retryable
    #[error("asset hash mismatch")] AssetHashMismatch,             // BRAND_ASSET_HASH_MISMATCH
    #[error("asset not acceptable")] AssetInvalid,                 // BRAND_ASSET_INVALID
    #[error("unknown asset")] AssetUnknown,                        // BRAND_ASSET_UNKNOWN
    #[error("store unavailable")] StoreUnavailable(#[source] sqlx::Error), // BRAND_STORE_UNAVAILABLE, retryable
    #[error("pack refused: {0}")] Refused(RefusalKind),            // see Error Table
}

/// The one error contract of module errors (2C `CodedError`): `code()` is the code in each
/// variant's comment (for `Refused`, the code of its `RefusalKind` in the Error Table);
/// `retryable()` is `true` for `AssetFetchFailed` and `StoreUnavailable` only; `repair()` is
/// `Some(TryAgain)` for those two, `Some(SendToIt)` for `Refused`, `None` otherwise.
impl ordin_types::CodedError for BrandError {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<ordin_types::RepairAction>;
}

/// Why a pack was refused; each maps to one code and one reason sentence stored in `refusal_reason`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    IdInvalid, VersionInvalid, StringsMissing, LocaleDuplicate, StringInvalid,
    TokensInvalid, FixedToken, ContrastFailed, UrlInvalid, AssetHashMismatch, AssetInvalid,
}

pub fn migrations() -> ordin_journal::ModuleMigrations; // { module: "brand", sqlite: &[(1, "create brand_packs", MIGRATION_0001)], postgres: &[] }
```

### Service API

| Command *(chosen here)* | Input | Result |
|---|---|---|
| `brand.get` | `{}` | `{"brand_identifier", "version", "source", "strings": BrandStrings, "design_tokens": {…}, "links": {"help_url","support_url","privacy_url"}, "font_count": u8, "refusal": {"brand_identifier","version","reason","received_at"} \| null}` |
| `brand.get_asset` | `{"asset": AssetRef}` | `AssetBytes` |

Event `brand_changed` (2C `EventKind::BrandChanged`, desktop only) with data `{"brand_identifier","version","source"}`, emitted by the desktop program with `Journal::append_event(EventKind::BrandChanged.as_str(), …)` when `subscribe()` yields a new pack and when Localisation's `subscribe()` yields a new active locale (strings change with it). These two commands and this payload are the definitive ones; the Service API catalogue takes them as written (R1, R6): the screens key a pack by `brand_identifier`, and there is no `brand_pack_id` in the event.

**Strings the screens read (row 84).** A pack carries the fixed `BrandStrings` fields — product name, coordinator name, about text, legal text — and nothing else; there is no per-message-identifier override map. A screen shows the product or coordinator name by placing `BrandStrings.product_name` or `coordinator_name` into the Localisation message that takes it as an argument. The screens call `brand.get` at start, with `localisation.get_settings` and `localisation.get_catalogue` (Localisation spec), and again on `brand_changed`.

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `Brand::start` | desktop program |
| `active`, `strings`, `last_refusal`, `asset` | desktop program, for `brand.get` and `brand.get_asset` (called by the screens) and for the window title ("the brand's product name") |
| `coordinator_name` | desktop program, which puts it in `OrdinHostPolicy.coordinator_name` (A-27), passes that policy to the Ordin Host at start and calls `OrdinHost::set_policy` with a new policy on each `subscribe()` change |
| `subscribe` | desktop program (forwarding `brand_changed`) |
| `migrations` | desktop program, registering it with the Journal's `MigrationRegistry` |

### Example

Profile brand section (abridged) and the resulting `brand.get` reply:

```json
{"brand_identifier":"acme-assist","version":"2.1.0",
 "strings":{"en-GB":{"product_name":"Acme Assist","coordinator_name":"Ada","about_text":"Acme Assist helps…"}},
 "legal_text":{"en-GB":"© 2026 Acme Ltd."},
 "design_tokens":{"color.text.light":"#1A1A1A","color.background.light":"#FFFFFF","color.accent.light":"#0B5FFF", "…":"…"},
 "logos":{"light":{"url":"https://profile.acme.example/brand/logo-light.svg","sha256":"9f2c…e1","media_type":"image/svg+xml"},"dark":{"…":"…"}}}
```

```json
{"brand_identifier":"acme-assist","version":"2.1.0","source":"organisation_profile",
 "strings":{"locale":"en-GB","product_name":"Acme Assist","coordinator_name":"Ada","about_text":"Acme Assist helps…","legal_text":"© 2026 Acme Ltd."},
 "design_tokens":{"color.text.light":"#1A1A1A","…":"…"},
 "links":{"help_url":"https://help.acme.example","support_url":"https://support.acme.example","privacy_url":"https://acme.example/privacy"},
 "font_count":2,"refusal":null}
```

A refused pack keeps the build pack active and `refusal` reads `{"brand_identifier":"acme-assist","version":"2.2.0","reason":"Text colour on background in the dark theme has contrast 3.1:1; at least 4.5:1 is needed.","received_at":"2026-01-10T09:00:00.000Z"}`.

## Internal Logic

### A. Start

1. Use `config.datastore.sqlite()`; a datastore without a SQLite pool → `BRAND_STORE_UNAVAILABLE` (the desktop never has one). The program registers `migrations()` with the Journal before `start`.
2. **Build pack.** Parse the compiled-in `build-brand/brand.json` (`include_str!`) and its assets (`include_bytes!`). If no `brand_packs` row has `source = 'build'` with this `brand_identifier` and `version`, write each asset to `<data_root>/brand/<id>/<version>/<sha256>.<ext>` (atomic: write to `.tmp`, fsync, rename) and insert a row with state `active` if no other row is `active`, else `contrast_checked`. The build pack passes the same checks as in step C at build time (a `build.rs` test runs C2–C5 over it); a build pack that fails stops the build.
3. Restore the row with `state = 'active'`; verify each of its asset files exists and its SHA-256 matches the file name; a missing or altered file → activate the build pack row instead and log `brand.asset_missing` (`warn`).
4. Resume rows left in `received` (a fetch interrupted by quit): run step C for the newest one; mark older `received` rows `refused` with reason "Replaced by a newer brand before it was checked." (`BRAND_SUPERSEDED`).
5. Spawn the profile follower (B) and the locale follower (D). Publish the active pack.

### B. Following the profile

1. On each snapshot from `profile.subscribe()`:
   - `document.brand` is `None` (also under `SnapshotSource::Defaults`) → activate the newest build pack row if it is not active (step E).
   - `Some(section)` with the same `(brand_identifier, version)` as the active row, or as a row already `refused` → nothing to do (a refused version is not retried; IT publishes a new version).
   - Otherwise insert a `received` row (`id` new UUIDv7, `received_at` now, paths empty strings until fetched, `source = 'organisation_profile'`) and run C.

### C. Checking a received pack

1. **Fetch assets.** For logos (light, dark), icon set (light, dark) and each font, in that order: `GET url` with the shared client, timeout 30 s per asset, at most 3 attempts with backoff 2 s and 8 s plus up to 1 s jitter on network errors and on HTTP 5xx/429 (honouring `Retry-After` ≤ 60 s). HTTP 4xx other than 429 → refuse with `AssetInvalid`. Exhausted retries → `AssetFetchFailed`: the row stays `received`, the active pack stays, log `brand.asset_fetch_failed` (`warn`, fields `brand_identifier`, `version`, `asset`, `http_status`), and C is retried at the next profile snapshot or after 15 minutes, whichever comes first.
2. **Verify each asset.** Body ≤ 2 MiB for logos and icon sets, ≤ 5 MiB per font, ≤ 8 fonts *(assumed: bounds the WebView's memory and the data URLs)*; SHA-256 of the body equals `sha256` → else `AssetHashMismatch`. Media type (the section's, cross-checked against the body's magic bytes) in: logos and icon sets `image/svg+xml`, `image/png`; fonts `font/woff2`, `font/ttf`, `font/otf`; else `AssetInvalid`. An SVG is refused (`AssetInvalid`) if it contains, case-insensitive, `<script`, `<foreignObject`, an attribute starting `on`, `javascript:`, or an `href`/`xlink:href` not starting with `#` (no active content or external reference, 04:241 spirit; ADR-028). Write accepted bodies under `<data_root>/brand/<id>/<version>/`.
3. **Strings.** Merge `strings` and `legal_text` into `Vec<BrandStrings>` sorted by locale; a locale in `strings` without `legal_text`, or the reverse → `StringsMissing`; two tags equal after BCP 47 canonicalisation → `LocaleDuplicate`; lengths per 2C rules → `StringInvalid`; no entry at all → `StringsMissing`.
4. **Tokens.** Each name matches `^(color|font|space|radius)(\.[a-z0-9-]+){1,3}$` → else `TokensInvalid`. A name with a fixed prefix (`color.danger.`, `color.waiting.`, `color.run-state.`, `color.approval.`) → `FixedToken`. Each `color.*` value matches `^#[0-9A-Fa-f]{6}$`; each `space.*` and `radius.*` value matches `^[0-9]{1,3}(\.[0-9])?px$`; each `font.*` value is a family name of 1–64 chars from `[A-Za-z0-9 -]`. A required colour token missing → `TokensInvalid`.
5. **Contrast** (WCAG 2.2: relative luminance L = 0.2126 R + 0.7152 G + 0.0722 B with each sRGB channel linearised `c ≤ 0.04045 ? c/12.92 : ((c+0.055)/1.055)^2.4`; ratio = (L1+0.05)/(L2+0.05), lighter over darker), for each theme `t` ∈ {light, dark}, ratio ≥ threshold:

   | Foreground | Background | Threshold | WCAG criterion |
   |---|---|---|---|
   | `color.text.t` | `color.background.t` | 4.5 | 1.4.3 |
   | `color.text.t` | `color.surface.t` | 4.5 | 1.4.3 |
   | `color.text-muted.t` | `color.background.t` | 4.5 | 1.4.3 |
   | `color.text-muted.t` | `color.surface.t` | 4.5 | 1.4.3 |
   | `color.on-accent.t` | `color.accent.t` | 4.5 | 1.4.3 |
   | `color.accent.t` | `color.background.t` | 3.0 | 1.4.11 |
   | `color.border-control.t` | `color.background.t` | 3.0 | 1.4.11 |
   | `color.focus.t` | `color.background.t` | 3.0 | 2.4.13 / 1.4.11 |

   Computed in `f64` inside the check function only (2C bars `f64` from stored and wire types, not from local arithmetic); a ratio is compared unrounded, and the stored reason quotes it rounded down to one decimal. Every failing pair is listed; the first failing pair's sentence is the `refusal_reason`, followed by "and N more" when several fail. Failure → `ContrastFailed`.
6. **Links**: each `https` ≤ 2048 bytes (re-checked) → else `UrlInvalid`.
7. Pass → update the row to `contrast_checked` with paths, tokens and strings, then step E. Any refusal → update the row: `state = 'refused'`, `refusal_reason` = the reason sentence (≤ 500 chars, en-GB, a fixed template per `RefusalKind` filled with token or asset names — never a URL), delete the version folder, log `brand.refused` (`warn`, fields `brand_identifier`, `version`, `code`), and keep the active pack.

### D. Following the locale

On each `localisation.subscribe()` change, recompute `strings()` (fallback chain, then `strings[0]`) and publish the unchanged pack again on the watch channel so the program emits `brand_changed`.

### E. Activating a row

In one transaction: set the current `active` row to `contrast_checked` (2C: the superseded row leaves `active`), set the new row `active` with `activated_at = now`. Publish. Log `brand.activated` (`info`, fields `brand_identifier`, `version`, `source`). Afterwards delete the asset folders of rows beyond the 20 most recent (by `received_at`), except the active row and the newest build row, and delete those rows *(assumed: bounds disk use; refusal history beyond 20 versions has no reader)*.

### F. `asset(which)`

Read the file named by the active row's path (or `font_file_paths[index]`); `index` out of range → `AssetUnknown`; file missing → re-run A3 (activates the build pack) and return `AssetFetchFailed` (retryable). Encode standard base64.

### Reporting to IT

The refusal reason is shown in Settings › Diagnostics (`brand.get` → `refusal`) where the person can use **Send to IT**; Brand sends nothing itself, because 08:14 sends a help request only on the person's press (see HLD disagreement B2 partB-D1 in findings).

## Data Model

```sql
-- SQLite, journal database; module "brand", version 1
CREATE TABLE brand_packs (
    id                  TEXT PRIMARY KEY,                          -- UUIDv7
    brand_identifier    TEXT NOT NULL CHECK (length(brand_identifier) BETWEEN 1 AND 64),
    version             TEXT NOT NULL,
    source              TEXT NOT NULL CHECK (source IN ('build', 'organisation_profile')),
    strings             TEXT NOT NULL,                             -- JSON Vec<BrandStrings>
    logo_light_path     TEXT NOT NULL,
    logo_dark_path      TEXT NOT NULL,
    icon_set_light_path TEXT NOT NULL,
    icon_set_dark_path  TEXT NOT NULL,
    design_tokens       TEXT NOT NULL,                             -- JSON object
    font_file_paths     TEXT NOT NULL,                             -- JSON array
    help_url            TEXT NOT NULL,
    support_url         TEXT NOT NULL,
    privacy_url         TEXT NOT NULL,
    state               TEXT NOT NULL CHECK (state IN ('received', 'contrast_checked', 'active', 'refused')),
    refusal_reason      TEXT CHECK (refusal_reason IS NULL OR length(refusal_reason) <= 500),
    received_at         TEXT NOT NULL,
    activated_at        TEXT,
    CHECK ((state = 'refused') = (refusal_reason IS NOT NULL)),
    UNIQUE (brand_identifier, version, source)
);
CREATE UNIQUE INDEX brand_packs_one_active ON brand_packs (state) WHERE state = 'active';
```

- While a row is `received`, the path and string columns hold empty strings and `[]`/`{}`; they are filled when it reaches `contrast_checked` or are left empty when `refused`.
- No PostgreSQL variant (desktop only). Rollback: the migration only creates; dropping it falls back to the compiled build pack at next start.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| Asset download fails after 3 attempts (network, 5xx, 429) | `Err(retryable)`; row stays `received`; active pack kept | `BRAND_ASSET_FETCH_FAILED` | — (internal; logged) |
| Asset body's SHA-256 differs from the section | pack refused | `BRAND_ASSET_HASH_MISMATCH` | `refusal_reason`: "The [logo, light] file does not match the brand's checksum." |
| Asset too large, wrong media type, SVG with active content or external reference, HTTP 4xx | pack refused | `BRAND_ASSET_INVALID` | "The [font 2] file isn't an accepted font." |
| Brand identifier invalid | pack refused | `BRAND_ID_INVALID` | "The brand identifier isn't valid." |
| Version not `MAJOR.MINOR.PATCH` | pack refused | `BRAND_VERSION_INVALID` | "The brand version isn't valid." |
| No strings, or a locale lacking legal text or strings | pack refused | `BRAND_STRINGS_MISSING` | "The brand has no legal text for [fr-FR]." |
| Two locales equal after canonicalisation | pack refused | `BRAND_LOCALE_DUPLICATE` | "The brand lists [en-GB] twice." |
| A brand string outside its length | pack refused | `BRAND_STRING_INVALID` | "The product name for [en-GB] is too long." |
| Token name outside the four namespaces, bad value form, a required colour missing, or over 500 tokens | pack refused | `BRAND_TOKENS_INVALID` | "The design token [color.text.dark] is missing." |
| Token sets a fixed safety colour | pack refused | `BRAND_FIXED_TOKEN` | "The brand tries to change [color.danger.light], which ordin keeps fixed." |
| A pair below its contrast threshold | pack refused | `BRAND_CONTRAST_FAILED` | "Text colour on background in the dark theme has contrast 3.1:1; at least 4.5:1 is needed." |
| A link not `https` or over 2048 bytes | pack refused | `BRAND_URL_INVALID` | "The brand's privacy link isn't a secure web address." |
| A `received` row replaced by a newer pack before checking | row refused | `BRAND_SUPERSEDED` | "Replaced by a newer brand before it was checked." |
| `brand.get_asset` with a font index out of range | `Err(permanent)` | `BRAND_ASSET_UNKNOWN` | `{"code":"BRAND_ASSET_UNKNOWN","message":"That brand file doesn't exist.","action":null}` |
| `brand_packs` unreadable or unwritable | `Err(retryable)` | `BRAND_STORE_UNAVAILABLE` | `{"code":"BRAND_STORE_UNAVAILABLE","message":"ordin couldn't load its appearance.","action":"try_again"}` |
| A stored row with `refused` and no reason, or the reverse | prevented by the table `CHECK`; an insert attempt is a defect | `BRAND_REFUSAL_REASON_MISMATCH` | — |

Refusal reasons are en-GB templates *(chosen here)*: they are read by IT in Diagnostics and help requests, not by the person in a task, and Brand does not depend on any locale but its own fallback. The person never sees a code (07).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Brand

  Scenario: Happy path — profile brand becomes active
    Given the build pack "ordin" 1.0.0 is active
    And a verified profile carries brand "acme-assist" 2.1.0 whose assets match their checksums
    And every contrast pair passes in light and dark
    When Brand receives the profile snapshot
    Then a brand_packs row "acme-assist" 2.1.0 moves received → contrast_checked → active
    And the "ordin" build row is no longer active
    And brand.get returns product name "Acme Assist" and coordinator name "Ada"
    And the program emits "brand_changed"

  Scenario: Error — contrast failure keeps the build brand
    Given a profile brand whose "color.text.dark" on "color.background.dark" has contrast 3.1:1
    When Brand checks the pack
    Then the row is "refused" with code BRAND_CONTRAST_FAILED
    And its refusal_reason names "Text colour on background in the dark theme" and "3.1:1"
    And the active pack is still the build pack
    And brand.get returns that refusal

  Scenario: Error — fixed safety colour
    Given a profile brand that sets "color.danger.light"
    When Brand checks the pack
    Then the row is "refused" with code BRAND_FIXED_TOKEN

  Scenario: Error — asset checksum mismatch
    Given the logo served for the light theme hashes to a different SHA-256 than the section states
    When Brand checks the pack
    Then the row is "refused" with code BRAND_ASSET_HASH_MISMATCH and the version folder is deleted

  Scenario: Error — SVG with script
    Given the dark logo is an SVG containing "<script>"
    When Brand checks the pack
    Then the row is "refused" with code BRAND_ASSET_INVALID

  Scenario: Edge — download fails and is retried
    Given the asset host answers 503 three times
    When Brand checks the pack
    Then the row stays "received", the build pack stays active
    And "brand.asset_fetch_failed" is logged
    And the check is retried within 15 minutes

  Scenario: Edge — same refused version is not retried
    Given "acme-assist" 2.2.0 was refused
    When a later profile refresh carries the same "acme-assist" 2.2.0
    Then no new row is created

  Scenario: Edge — profile removes its brand section
    Given "acme-assist" 2.1.0 is active
    When a newer verified profile has no brand section
    Then the build pack becomes active again

  Scenario: Edge — strings follow the locale
    Given the active pack has strings for "en-GB" and "fr-FR"
    When the person's locale changes to "fr-FR"
    Then brand.get returns the fr-FR product name
    And with the locale "de-DE" (no strings) it returns the strings of the next locale in the fallback chain

  Scenario: Edge — damaged asset on disk
    Given the active pack's dark logo file was deleted
    When the desktop starts
    Then the build pack becomes active and "brand.asset_missing" is logged

  Scenario: Error — asset index unknown
    When the WebView sends brand.get_asset with {"kind":"font","index":9} and the pack has 2 fonts
    Then the reply is an error with code BRAND_ASSET_UNKNOWN

  Scenario: Error — a refused pack carries its repair action
    Given a profile brand pack whose design tokens fail the contrast check
    When the Brand verifies it
    Then the error's code() is the contrast refusal code of the Error Table and repair() is SendToIt
    And active() still returns the previous pack

  Scenario: Happy path — a locale change re-announces the brand
    Given the active pack has strings for "en-GB" and "fr-FR"
    When Localisation's active locale changes to "fr-FR"
    Then strings() returns the "fr-FR" strings
    And the desktop program appends a "brand_changed" event with brand_identifier, version and source
```

## Performance, Security, Observability

- **Performance targets:**
  - `brand.get`: p95 ≤ 5 ms (in memory). `brand.get_asset`: p95 ≤ 30 ms for a 2 MiB asset (file read + base64), inside A-03's 150 ms.
  - Applying a new pack after its profile arrives: ≤ 10 s on a 10 Mbit/s link for a pack of ≤ 20 MiB in all; nothing blocks start: the stored active pack is shown first (A-03's 3 s start).
  - Contrast check: ≤ 1 ms for the 16 pairs.
  - Memory: ≤ 25 MiB (one active pack's strings and tokens; asset bytes are read per request, not cached).
- **Security:**
  - Inputs: the brand section of a profile whose signature the Organisation Profile verified; asset bytes verified by SHA-256 before use, so a changed host file is refused. Downloads use the platform trust store and proxy (SA-HTTP-02); only `https`.
  - SVG active content and external references refused; fonts and images reach the WebView only as bytes from `brand.get_asset` — the WebView makes no network call.
  - Approval-card structure, danger, waiting and run-state colours cannot be set (fixed prefixes refused); all screens are drawn from tokens (ADR-041), so a pack cannot hide a safety cue.
  - Entry points: two read-only Service API commands through the Command Bridge (Tauri capability); typed input, unknown fields refused.
  - Never logged: asset URLs (they may carry company host names; only `asset` kind is logged), asset bytes, brand text.
- **Observability** (`tracing` JSON lines; desktop only, so no metrics or spans, SA-XC-04/05):

| Event | Level | Fields |
|---|---|---|
| `brand.received` | info | `brand_identifier`, `version` |
| `brand.activated` | info | `brand_identifier`, `version`, `source` |
| `brand.refused` | warn | `brand_identifier`, `version`, `code` |
| `brand.asset_fetch_failed` | warn | `brand_identifier`, `version`, `asset`, `http_status`, `attempt` |
| `brand.asset_missing` | warn | `brand_identifier`, `version`, `asset` |
| `brand.pruned` | debug | `rows_deleted` |

Every line carries `component = "brand"`.

## Gaps

None.
