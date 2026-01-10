# SPEC: Localisation

> **Spec set:** `ordin-desktop` · **Phase:** 3 — Component spec · **Status:** Approved (user, January 2026; Phase 6 passed) · amended January 2026 (ADR-109; promoted to Approved by the user January 2026)
> **Mode:** greenfield
> **derivedFromHld:** 0.63.0
> **Crate:** `ordin-localisation` · **Build phase:** 2 · **Programs:** D P A
> **Dependencies:** Organisation Profile, Journal

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
- [Amendment January 2026 — Ordin's language and language files (ADR-109)](#amendment-january-2026--ordins-language-and-language-files-adr-109)

## Purpose

Localisation decides which locale the person sees, loads the Project Fluent message catalogues of that locale and of the organisation profile's fallback, serves the catalogue text to the WebView, formats every text the core composes (approval cards, errors, notifications, chat messages) with Unicode CLDR rules, and tells Ordin's sessions, the Dictation Service and Brand which locale is active (02:531, ADR-042). It owns the person's locale preference (02:532, 04:17–19) and, from revision 0.58.0, the person's own wording: per-message edits of a shipped language, and the languages the person made with Ordin or imported, each guarded by one fixed check against `en-GB` and exported and imported as one language file (ADR-109). It exists so that no interface string is literal text and no value is formatted by string concatenation (07 — *Internationalisation*; G23).

## Approach

One in-process service holds an immutable *catalogue set* built at compile time from the bundled `.ftl` files, and one `fluent_bundle::concurrent::FluentBundle` per bundled locale, with two custom Fluent functions (`NUMBER`, `DATETIME`) backed by ICU4X (`icu` 2.3.1, SA-I18N-02) so plural, number and date rules come from CLDR. The active locale is negotiated in fixed code (person's choice → operating system → profile fallback → `en-GB`) and published on a `tokio::sync::watch` channel that dependants subscribe to. Message identifiers are generated as Rust constants at build time, so a reference to an identifier absent from `en-GB` does not compile. Rejected: *`fluent-langneg` for negotiation* — not named in 1A, and the three-step rule below needs no library; *loading `.ftl` files from disk at run time* — a missing file would surface only at run time, and ADR-042 requires the build to fail.

*Amended January 2026 (ADR-109).* The compiled-in catalogues stay the base layer. On top of them sits one **person layer**, rows in the journal database: per-message overrides for shipped languages, and whole message sets for languages the person made (drafted by Ordin) or imported. One fixed function, `check_entry` (section H), run against the compiled-in `en-GB`, guards every way text enters that layer — an edit, a drafted batch, an import, and the re-check after an update. A safety message (approval cards and who-acts statements) from a draft or an import is used only after the person has checked it. Person-layer text is turned into bundles at start and on each change, so formatting stays in memory with no model call and no file read. Rejected: *a full copy per edited shipped language* — corrections and new messages in an update would not reach it (the user adopted per-message overrides); *drafting inside this crate* — Localisation calls no model; the Ordin Host drafts and returns text through `add_drafted`, which never writes unchecked text.

## Shared Context

### Types from Phase 2C (copied verbatim)

```rust
/// RFC 3339, UTC, millisecond precision, `Z` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub chrono::DateTime<chrono::Utc>);
// Serialises as `YYYY-MM-DDTHH:MM:SS.mmmZ`; any other form is refused with TS_NOT_RFC3339_MS_UTC.

/// A base-10 decimal number written as text (`-12.50`), so no binary float enters the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DecimalString(pub String); // ^-?[0-9]{1,28}(\.[0-9]{1,10})?$ else NUM_NOT_DECIMAL

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LocalePreference {
    pub locale: String,
    pub source: LocaleSource,
    pub updated_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocaleSource { OperatingSystem, Person }
```

| Field (2C) | Rule | Error code |
|---|---|---|
| `LocalePreference.locale` | BCP 47 tag, ≤ 35 chars, one of the locales offered to the person: the profile's offered bundled locales (04:19), or a language the person made or imported and confirmed (*amended January 2026, ADR-109*) | `LOC_NOT_OFFERED`, `LOC_LANGUAGE_NOT_CONFIRMED` |

Storage (2C): SQLite table `locale_preferences`, exactly one row per installation, in the journal database, owned by Localisation.

### Service API conventions (2C wire part §1; SA-ENV-01, SA-API-01)

- Commands are named `<module>.<verb>`; IPC name `<module>_<verb>`; Remote API `POST /v1/<module>/<verb>`. The `ServiceApi` trait is implemented by the programs, which call the public methods below.
- A command error is `ServiceError { code, message, action }` where `message` is one plain-language sentence **built by Localisation** in the person's locale (2C wire §1, `ServiceError.message`: "built by Localisation").

Paged results (2C wire §1, SA-PAGE-01, copied verbatim from SharedTypes.md; used by `messages` and `localisation.list_messages`, 100 per page):

```rust
pub struct Page<T> {
    pub items: Vec<T>,
    /// Always serialised; `null` on the last page.
    pub next_cursor: Option<Cursor>,
}
pub struct Cursor(String); // base64url of the JSON form of the last row's sort key; here the message id
```

Bounds the Service API's typed input check enforces before a method runs (SA-API-02; a breach is `SVC_INPUT_INVALID`, never a `LOC_` code): `mark_reviewed` 1–200 message ids; `filter.text` 1–200 characters; `import_language.file_name` 1–255 characters. The language name of `begin_draft` (1–60 characters) is checked by its only caller, the Ordin Host (`OH_INPUT_INVALID`).

`ServiceError` and `RepairAction { TryAgain, SignInAgain, SendToIt }` are 2C's (`ordin_types`); this crate imports `RepairAction` and `CodedError` and defines neither (reconciliation row 81). It never builds a `ServiceError` itself: the programs do, with `error_sentence`.

### Organisation Profile interface consumed (Organisation Profile spec, *The service*)

```rust
pub struct OrganisationProfileService { /* private */ }
impl OrganisationProfileService {
    pub fn current(&self) -> std::sync::Arc<ProfileSnapshot>;
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<std::sync::Arc<ProfileSnapshot>>;
}
pub struct ProfileSnapshot { pub document: OrganisationProfileDocument, pub source: SnapshotSource }
pub enum SnapshotSource {
    Verified { profile_id: OrganisationProfileId, profile_version: String, activated_at: Timestamp },
    Defaults,
}
// The section this component reads; key names from 2D-2 (`locales.offered[]`, `locales.fallback`).
pub struct LocalesSection { #[serde(default)] pub offered: Vec<String>, #[serde(default = "en_gb")] pub fallback: String }
// `OrganisationProfileDocument.locales: LocalesSection`.
// `offered` empty means every bundled locale; `fallback` defaults to "en-GB".
// Organisation Profile checks each tag is well-formed BCP 47, ≤ 35 chars, and that `fallback` ∈ `offered`
// when `offered` is non-empty; it does not know the bundled list (Localisation intersects).
```

### Configuration (2D)

| Source | Key | Value used here |
|---|---|---|
| Organisation profile (2D-2) | `locales.offered[]` | list of BCP 47 tags; ⊆ bundled locales; default every bundled locale |
| Organisation profile (2D-2) | `locales.fallback` | BCP 47 tag; default `en-GB` |
| Person's setting (2D-3) | Interface language | locale tag; default the operating system's, if offered; set in desktop Settings |
| 1A SA-I18N-00 | source and last-resort locale | `en-GB`; a missing `en-GB` message fails the build |
| 1A SA-I18N-01 | libraries | `fluent-bundle` 0.16.0 (core); `@fluent/bundle` 0.19.1 (WebView); both read the same `.ftl` files |
| 1A SA-I18N-02 | CLDR | ICU4X `icu` 2.3.1 in the core; the WebView's `Intl` in the screens |

The agent profile reads no organisation profile (2D, *Agent-profile server*); there the snapshot is always `SnapshotSource::Defaults`.

### Names chosen here

| Name | Kind | *chosen here* reason |
|---|---|---|
| `crates/ordin-localisation/catalogues/<locale>/<area>.ftl` | file layout | one folder per locale, one file per screen area (SA-I18N-00 groups identifiers by screen area) |
| `locale-native-name` | message identifier, required in every locale | the locale's own name for the language picker, so no display-names library is needed |
| `en-XA`, `ar-XB` | pseudo-locale tags (BCP 47 private-use region codes) | ADR-042 pseudo-locale tests: accented 40% longer; right-to-left |
| `localisation.get_settings`, `localisation.set_locale`, `localisation.get_catalogue` | Service API commands | SA-API-01 form |
| `locale_changed` | Service API event kind | G-1 leaves the event list open; the screens must reload text when a profile change moves the locale |
| `MessageId`, `MessageArgs`, `ActiveLocale`, `Localisation`, `LocalisationError` | Rust types | this crate's public surface |
| `SAFETY_IDS`, `MESSAGE_AREAS` | generated constants (build.rs) | the safety wording and each message's screen area (ADR-109; amended January 2026) |
| `# safety` | Fluent comment line in `en-GB` | marks a who-acts message as safety wording when its identifier does not begin `approval-` |
| `person_languages`, `person_messages` | SQLite tables, step 2 | the person layer |
| `localisation.list_languages`, `localisation.list_messages`, `localisation.edit_message`, `localisation.restore_message`, `localisation.restore_language`, `localisation.mark_reviewed`, `localisation.confirm_language`, `localisation.remove_language`, `localisation.export_language`, `localisation.import_language` | Service API commands | SA-API-01 form (ADR-109) |
| `language_changed` | Service API event kind (2C `EventKind::LanguageChanged`) | the screens reload a language's text and the Language screen after a change that does not move `ActiveLocale` |
| `ordin-language-<tag>.ftl` | export file name | one file per language (ADR-109) |
| `LanguageSummary`, `LanguageKind`, `LanguageState`, `DraftProgress`, `MessageSource`, `MessageFilter`, `MessageOnly`, `MessageRow`, `LanguageFile`, `ImportReport`, `LanguageMatch`, `DraftSource`, `DraftBatchOutcome`, `DraftEnd`, `LanguageChange`, `LanguageChangeKind`, `EntryProblem` | Rust types | the person layer's public surface |

### Reconciliation assumptions

- **SA-RECON-01** The preference row lives in the Journal's datastore through a Journal edge (D4, R20): the crate receives the `Datastore` and exports `migrations()`.
- **SA-RECON-02** The Service API catalogue takes `localisation.get_settings`, `localisation.set_locale`, `localisation.get_catalogue` and the `locale_changed` payload `{"active": ActiveLocale}` as written (R1, R6).
- **SA-RECON-03** The screens get catalogue text from the core at start and on `locale_changed`; nothing is bundled into the screens at build time (row 76).
- **SA-RECON-04** An error's repair action is its `CodedError::repair()`; Localisation supplies only the sentence (row 79).
- **SA-RECON-05** The programs forward `locale_changed` through `Journal::append_event` (row 80).
- **SA-RECON-06** The operating system's preferred locales come from the `sys-locale` crate, adopted in 05 revision 0.49.0 (decision D2); this closes the earlier gap on how the preferred locales are read.

## Public Interface

### Rust API (crate `ordin_localisation`)

```rust
use std::sync::Arc;
use tokio::sync::watch;

/// A message identifier that exists in the `en-GB` catalogue. Constructed only by the
/// generated constants in `ordin_localisation::id` (build.rs) — a typo does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(&'static str);
impl MessageId { pub const fn as_str(&self) -> &'static str { self.0 } }

/// Generated: one constant per `en-GB` message, SCREAMING_SNAKE_CASE of the kebab-case id.
/// Example: `pub const APPROVAL_CARD_SEND_BUTTON: MessageId = MessageId("approval-card-send-button");`
pub mod id { include!(concat!(env!("OUT_DIR"), "/message_ids.rs")); }

/// Every `en-GB` message identifier, sorted; read by the TypeScript-binding generator.
pub fn message_ids() -> &'static [&'static str];

/// Every locale with a bundled catalogue, `en-GB` first, then alphabetical by tag.
/// Builds with the cargo feature `pseudo-locales` also contain `en-XA` and `ar-XB`.
pub fn bundled_locales() -> &'static [&'static str];

/// One named argument of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgValue {
    Text(String),
    /// Formatted with CLDR number rules of the active locale.
    Number(DecimalString),
    /// Whole count; drives plural selection.
    Count(u64),
    /// Formatted with CLDR date-time rules in the computer's local offset.
    DateTime(Timestamp),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct MessageArgs(pub Vec<(&'static str, ArgValue)>);
impl MessageArgs {
    pub fn new() -> Self;
    pub fn with(mut self, name: &'static str, value: ArgValue) -> Self;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextDirection { Ltr, Rtl }

/// The locale in force, published to every subscriber.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ActiveLocale {
    /// The negotiated, bundled, offered tag, e.g. "en-GB".
    pub locale: String,
    pub source: LocaleSource,
    pub direction: TextDirection,
    /// Tags tried for a missing message, in order, ending in "en-GB"; no duplicates.
    pub fallback_chain: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DateTimeStyle { Date, Time, DateTime, ShortDate }

/// Where the preference row lives.
pub enum PreferenceStore {
    /// Desktop, personal server, agent single shape: the Journal's `Datastore` (D4, R20);
    /// this crate uses `datastore.sqlite()` and treats a PostgreSQL store as `None`.
    Datastore(ordin_journal::Datastore),
    /// Agent profile: no row; the locale is always the fallback ("en-GB" under Defaults).
    None,
}

pub struct LocalisationConfig {
    pub store: PreferenceStore,
    /// The operating system's preferred locales, most preferred first, as BCP 47 tags.
    /// Desktop and personal server: from `sys_locale::get_locales()` (05, revision 0.49.0).
    /// Agent profile: empty.
    pub os_locales: Vec<String>,
    /// The program's own version, e.g. "1.4.0" (amended January 2026, ADR-109): written into exported
    /// files and compared with `person_languages.checked_against` to re-check the person layer after an update.
    pub app_version: String,
}

pub struct Localisation { /* private: catalogue set, bundles, store, watch sender, Arc<OrganisationProfileService> */ }

impl Localisation {
    /// Loads the bundled catalogues, reads (or creates) the preference row, negotiates the
    /// active locale and starts a task that follows profile changes.
    pub async fn start(
        config: LocalisationConfig,
        profile: Arc<OrganisationProfileService>,
    ) -> Result<Arc<Localisation>, LocalisationError>;

    pub fn active(&self) -> ActiveLocale;
    pub fn subscribe(&self) -> watch::Receiver<ActiveLocale>;

    /// The locales the person may choose: offered ∩ bundled, in the profile's order.
    pub fn offered(&self) -> Vec<OfferedLocale>;

    /// `Some(tag)` stores the person's choice; `None` returns to following the operating system.
    pub async fn set_locale(&self, locale: Option<&str>) -> Result<ActiveLocale, LocalisationError>;

    /// Formats one message in the active locale, falling back along `fallback_chain`.
    /// Never fails: every MessageId exists in en-GB by construction.
    pub fn message(&self, id: MessageId, args: &MessageArgs) -> String;

    /// As `message`, for an explicit locale (used for a server's chat in the person's locale).
    pub fn message_in(&self, locale: &str, id: MessageId, args: &MessageArgs) -> String;

    /// The plain-language sentence for a registered error code: message id
    /// `error-<code in kebab-case>` (e.g. `PROF_SIGNATURE_INVALID` → `error-prof-signature-invalid`);
    /// an unregistered code yields the sentence of `error-srv-internal`.
    pub fn error_sentence(&self, code: &str, args: &MessageArgs) -> String;

    pub fn format_number(&self, value: &DecimalString) -> String;
    pub fn format_datetime(&self, at: Timestamp, style: DateTimeStyle) -> String;
    /// "A, B and C" by CLDR list rules ("and" type).
    pub fn format_list(&self, items: &[String]) -> String;
    /// Collation order of the active locale.
    pub fn compare(&self, a: &str, b: &str) -> std::cmp::Ordering;

    /// Fluent source text of `locale` and of each tag after it in its fallback chain.
    /// From January 2026 (ADR-109): a shipped locale's text has its served overrides applied, and a
    /// person language's text is its served entries.
    pub fn catalogue(&self, locale: &str) -> Result<CatalogueText, LocalisationError>;

    // ---- Person layer (amended January 2026, ADR-109; Internal Logic H–M) ----

    /// Every bundled language and every person language; en-GB first, then by tag.
    pub fn languages(&self) -> Vec<LanguageSummary>;
    /// Every area (en-GB file stem), sorted.
    pub fn areas(&self) -> Vec<String>;
    /// One page (100) of messages of `locale`, in message-id order.
    pub fn messages(&self, locale: &str, filter: &MessageFilter, cursor: Option<Cursor>)
        -> Result<Page<MessageRow>, LocalisationError>;
    pub async fn edit_message(&self, locale: &str, message_id: &str, ftl: &str) -> Result<MessageRow, LocalisationError>;
    pub async fn restore_message(&self, locale: &str, message_id: &str) -> Result<MessageRow, LocalisationError>;
    pub async fn restore_language(&self, locale: &str) -> Result<LanguageSummary, LocalisationError>;
    pub async fn mark_reviewed(&self, locale: &str, message_ids: &[String]) -> Result<LanguageSummary, LocalisationError>;
    pub async fn confirm_language(&self, locale: &str) -> Result<LanguageSummary, LocalisationError>;
    pub async fn remove_language(&self, locale: &str) -> Result<(), LocalisationError>;
    pub fn export_language(&self, locale: &str) -> Result<LanguageFile, LocalisationError>;
    pub async fn import_language(&self, file_name: &str, ftl: &str, replace: bool) -> Result<ImportReport, LocalisationError>;

    /// For the Ordin Host: changing the language in words, and drafting a language.
    pub fn match_language(&self, requested: &str) -> Result<LanguageMatch, LocalisationError>;
    pub fn draft_sources(&self, locale: &str) -> Result<Vec<DraftSource>, LocalisationError>;
    pub async fn begin_draft(&self, locale: &str, name: &str, total: u32) -> Result<(), LocalisationError>;
    pub async fn add_drafted(&self, locale: &str, reply: &str) -> Result<DraftBatchOutcome, LocalisationError>;
    pub async fn end_draft(&self, locale: &str, end: DraftEnd) -> Result<LanguageSummary, LocalisationError>;

    /// Changes to a language's wording or state; capacity 64 (a lagging receiver reloads with `languages()`).
    pub fn language_changes(&self) -> tokio::sync::broadcast::Receiver<LanguageChange>;
}

/// The one fixed check of person-layer text (section H). `expected_id` is `None` when the entry
/// comes from a drafted reply or an import, where the identifier is read from the entry.
pub fn check_entry(locale: &str, expected_id: Option<&str>, entry: &str) -> Result<(), EntryProblem>;

/// Generated by build.rs (step A3a). Every en-GB message whose identifier begins `approval-`, and every
/// en-GB message whose attached comment holds a line exactly `safety`; sorted.
pub const SAFETY_IDS: &[&str];
/// Generated by build.rs: (message id, area) for every en-GB message, sorted by id.
pub const MESSAGE_AREAS: &[(&str, &str)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageKind { Shipped, Made, Imported }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageState { Drafting, NeedsReview, InUse }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageSummary {
    pub locale: String,
    /// The language's own served `locale-native-name`, else the name it was made with; `None` when neither exists.
    pub native_name: Option<String>,
    pub direction: TextDirection,
    pub kind: LanguageKind,
    pub state: LanguageState,
    /// In `offered()`, so `set_locale` accepts it.
    pub choosable: bool,
    /// A bundled locale in the profile's `locales.offered` (or every bundled locale when it is empty); `false` for person languages.
    pub offered_by_company: bool,
    /// Served entries with origin `edited`.
    pub edited: u32,
    /// en-GB messages this language shows in English: no served entry and, for a shipped language, no shipped text.
    pub in_english: u32,
    /// Safety entries stored with `reviewed = 0`.
    pub needs_review: u32,
    /// Stored entries with `usable = 0` after a re-check.
    pub needs_changing: u32,
    /// `None` for a shipped language with no person row.
    pub made_for_version: Option<String>,
    /// `Some` while state is `drafting`.
    pub draft: Option<DraftProgress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftProgress { pub done: u32, pub total: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource { Shipped, Edited, Drafted, Imported, English }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MessageFilter {
    /// 1–200 characters; a case-folded substring of the identifier, the en-GB text or the language's text.
    #[serde(default)] pub text: Option<String>,
    /// One of `areas()`.
    #[serde(default)] pub area: Option<String>,
    #[serde(default)] pub only: Option<MessageOnly>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageOnly { NeedsReview, InEnglish, Edited, NeedsChanging }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MessageRow {
    pub message_id: String,
    pub area: String,
    /// The en-GB entry (Fluent source).
    pub english: String,
    /// The language's entry as stored (person layer) or shipped; `None` when it shows in English.
    pub text: Option<String>,
    /// The shipped entry of a shipped language; `None` otherwise.
    pub shipped: Option<String>,
    pub source: MessageSource,
    pub safety: bool,
    pub reviewed: bool,
    pub needs_changing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageFile { pub file_name: String, pub ftl: String }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportReport {
    pub locale: String,
    pub stored: u32,
    pub dropped_unknown: u32,
    pub in_english: u32,
    pub left_out: u32,
    /// At most 100, in message-id order.
    pub left_out_ids: Vec<String>,
    pub summary: LanguageSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum LanguageMatch {
    Choosable { locale: String },
    NotConfirmed { locale: String },
    NotOffered { locale: String },
    Missing { locale: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftSource { pub message_id: String, pub english: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DraftBatchOutcome { pub stored: u32, pub left_out: u32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftEnd { Finished, Stopped, Paused }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguageChangeKind { Edited, Restored, Reviewed, Confirmed, Imported, Removed, DraftStarted, DraftProgress, DraftEnded }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LanguageChange { pub locale: String, pub change: LanguageChangeKind }

/// Why `check_entry` refused an entry. Each has a plain sentence, catalogue id `language-problem-<kebab of the variant>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryProblem {
    TooLong,
    ForbiddenCharacter,
    DoesNotParse,
    NotOneMessage,
    UnknownMessage,
    IdMismatch,
    VariableMissing { name: String },
    VariableUnknown { name: String },
    AttributeMismatch,
    FunctionNotAllowed { name: String },
    ReferenceUnknown { id: String },
    PluralCategory { key: String },
    /// Not raised by `check_entry`: storing the entry would take the language's stored entries over 2,097,152 characters (LOC-LF-2a).
    LanguageFull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OfferedLocale { pub locale: String, pub native_name: String, pub direction: TextDirection }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueText {
    pub locale: String,
    pub direction: TextDirection,
    /// One entry per tag of the chain, first = `locale`; `ftl` is the concatenated source.
    pub resources: Vec<CatalogueResource>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CatalogueResource { pub locale: String, pub ftl: String }

#[derive(Debug, thiserror::Error)]
pub enum LocalisationError {
    #[error("locale tag is not valid BCP 47")] TagInvalid,                 // LOC_TAG_INVALID
    #[error("locale is not offered")] NotOffered,                         // LOC_NOT_OFFERED
    #[error("no bundled catalogue for the locale")] CatalogueMissing,     // LOC_CATALOGUE_MISSING
    #[error("preference store unavailable")] StoreUnavailable(#[source] sqlx::Error), // LOC_STORE_UNAVAILABLE
    #[error("preference row unreadable")] StoreCorrupt,                   // LOC_STORE_CORRUPT
    // Amended January 2026 (ADR-109):
    #[error("languages cannot be changed here")] LanguagesUnavailable,    // LOC_LANGUAGES_UNAVAILABLE
    #[error("language is not editable")] LanguageNotEditable,             // LOC_LANGUAGE_NOT_EDITABLE
    #[error("language unknown")] LanguageUnknown,                         // LOC_LANGUAGE_UNKNOWN
    #[error("message unknown")] MessageUnknown,                           // LOC_MESSAGE_UNKNOWN
    #[error("message refused")] MessageRefused { message_id: String, problem: EntryProblem }, // LOC_MESSAGE_REFUSED
    #[error("language is not shipped")] NotShipped,                       // LOC_NOT_SHIPPED
    #[error("file is not a language file")] FileUnreadable,               // LOC_FILE_UNREADABLE
    #[error("file too large")] FileTooLarge,                              // LOC_FILE_TOO_LARGE
    #[error("language exists")] LanguageExists,                           // LOC_LANGUAGE_EXISTS
    #[error("language is being drafted")] LanguageDrafting,               // LOC_LANGUAGE_DRAFTING
    #[error("review incomplete")] ReviewIncomplete { count: u32 },        // LOC_REVIEW_INCOMPLETE
    #[error("language not confirmed")] LanguageNotConfirmed,              // LOC_LANGUAGE_NOT_CONFIRMED
    #[error("language is active")] LanguageActive,                        // LOC_LANGUAGE_ACTIVE
    #[error("too many languages")] TooManyLanguages,                      // LOC_TOO_MANY_LANGUAGES
}
/// The one error contract of module errors (2C `CodedError`): `code()` is the code in the
/// comment of each variant; `retryable()` is `true` only for `StoreUnavailable`; `repair()` is
/// `Some(TryAgain)` for `StoreUnavailable` and `None` otherwise (every ADR-109 variant is permanent with no repair).
impl ordin_types::CodedError for LocalisationError {
    fn code(&self) -> &'static str;
    fn retryable(&self) -> bool;
    fn repair(&self) -> Option<ordin_types::RepairAction>;
}

/// Migrations for the program to register with the Journal's MigrationRegistry (Journal spec, *Migration registry*).
pub fn migrations() -> ordin_journal::ModuleMigrations; // { module: "localisation", sqlite: &[(1, "create locale_preferences", MIGRATION_0001), (2, "create person language tables", MIGRATION_0002)], postgres: &[] }
```

### Service API commands (implemented by the program over the methods above)

| Command *(chosen here)* | Input | Result | Method |
|---|---|---|---|
| `localisation.get_settings` | `{}` | `{"active": ActiveLocale, "offered": [OfferedLocale], "following_operating_system": bool}` | `active()`, `offered()` |
| `localisation.set_locale` | `{"locale": "<tag>" \| null}` | `ActiveLocale` | `set_locale` |
| `localisation.get_catalogue` | `{"locale": "<tag>"}` | `CatalogueText` | `catalogue` |
| `localisation.list_languages` | `{}` | `{"languages": [LanguageSummary], "areas": [String]}` | `languages`, `areas` |
| `localisation.list_messages` | `{"locale", "text"?, "area"?, "only"?, "cursor"?}` | `Page<MessageRow>` (100 per page, SA-PAGE-01) | `messages` |
| `localisation.edit_message` | `{"locale", "message_id", "ftl"}` | `MessageRow` | `edit_message` |
| `localisation.restore_message` | `{"locale", "message_id"}` | `MessageRow` | `restore_message` |
| `localisation.restore_language` | `{"locale"}` | `LanguageSummary` | `restore_language` |
| `localisation.mark_reviewed` | `{"locale", "message_ids": [String]}` (1–200) | `LanguageSummary` | `mark_reviewed` |
| `localisation.confirm_language` | `{"locale"}` | `LanguageSummary` | `confirm_language` |
| `localisation.remove_language` | `{"locale"}` | `{}` | `remove_language` |
| `localisation.export_language` | `{"locale"}` | `LanguageFile` (≤ 2 MiB); the screens write it where the person chooses, as `workflows.export` | `export_language` |
| `localisation.import_language` | `{"file_name", "ftl", "replace": bool}` (file name 1–255 characters; `ftl` ≤ 2 MiB) | `ImportReport` | `import_language` |

The ten commands from `localisation.list_languages` down are added January 2026 (ADR-109); every input is `deny_unknown_fields`, every scope is Any, and `list_languages`, `list_messages` and `export_language` are Read, the others Change. The existing three keep their input and reply shapes; `ActiveLocale` is unchanged.

Event `language_changed` (2C `EventKind::LanguageChanged`, added January 2026) with data `{"locale": "<tag>", "change": LanguageChangeKind}`, emitted by the program with `Journal::append_event(EventKind::LanguageChanged.as_str(), …)` for each item of `language_changes()`, exactly as `locale_changed` is forwarded. A change to the active language's wording emits `language_changed` only; `locale_changed` is emitted only when `ActiveLocale` changes.

Event `locale_changed` (2C `EventKind::LocaleChanged`) with data `{"active": ActiveLocale}`, emitted by the program with `Journal::append_event(EventKind::LocaleChanged.as_str(), …)` each time `subscribe()` yields a new value. These three commands and this payload are the definitive ones; the Service API catalogue takes them as written (R1, R6) — the event carries the whole `ActiveLocale` (locale, source, direction, fallback chain), not a `{locale, direction}` pair. A change of the Interface-language setting goes through `localisation.set_locale`, not through `settings.change`.

**How the screens get their text (reconciliation row 76; N16).** The core serves catalogue text: the screens load nothing at build time. At start, and again on every `locale_changed`, the screens call `localisation.get_settings` (active locale, offered locales, whether the operating system is followed) and `localisation.get_catalogue` for the active locale, and they call the Brand's `brand.get` for the product name, string overrides and design tokens (Brand spec). No combined start-up command exists; these three calls are the start-up set. The WebView formats with `@fluent/bundle` from the returned FTL text (SA-I18N-01).

**Error sentences and repair actions (row 79; N17).** `error_sentence(code, &MessageArgs::new())` is the sentence of any registered code, by the naming rule above; the programs' `ErrorSentences::sentence(code)` calls exactly that. Localisation holds no code → repair-action map: the action of an error is its `CodedError::repair()`, which every module error implements.

### Who calls each public item (reconciliation §3; R6)

| Item | Called by |
|---|---|
| `Localisation::start` | desktop program, server program (all profiles; `PreferenceStore::None` in the agent profile) |
| `active` | the programs, for `localisation.get_settings` (called by the screens at start and on `locale_changed`); the Dictation Service (the locale of a new dictation session) |
| `offered`, `set_locale`, `catalogue` | the programs, for `localisation.get_settings`, `localisation.set_locale`, `localisation.get_catalogue` (called by the screens at start and on `locale_changed`) |
| `subscribe` | the programs (forwarding `locale_changed` through the Journal); the Brand (recomputing its strings on a locale change); the Dictation Service (stopping a session with reason `LocaleChanged`) |
| `message`, `message_in` | desktop and server programs (window and notification text, chat text in the person's locale), and every core module handed `Arc<Localisation>` by the program — the Dictation Service among them |
| `error_sentence` | the programs' `ErrorSentences` implementation (Service API error replies) |
| `format_number`, `format_datetime`, `format_list`, `compare` | the programs and the modules handed `Arc<Localisation>` |
| `message_ids`, `bundled_locales` | the TypeScript-binding generator (build time) |
| `migrations` | the programs, registering it with the Journal's `MigrationRegistry` |
| `languages`, `areas`, `messages`, `edit_message`, `restore_message`, `restore_language`, `mark_reviewed`, `confirm_language`, `remove_language`, `export_language`, `import_language` | the programs (desktop and personal server), for the ten `localisation.*` commands added January 2026, called by the Screens Module's Settings › Language and Language screen |
| `match_language` | the Ordin Host (crate `ordin-host`, X-30): its tool `change_setting` (setting `interface_language`) |
| `set_locale` | also the Ordin Host (`ordin-host`, X-30): its tool `change_setting`, when the match is `Choosable` |
| `active` | also the Ordin Host (`ordin-host`, X-30): the seed's language line and the change prefix |
| `draft_sources`, `begin_draft`, `add_drafted`, `end_draft` | the Ordin Host (`ordin-host`, X-30): `ordin.estimate_language_draft`, `ordin.draft_language` and its drafting job |
| `language_changes` | the programs, forwarding `language_changed` through the Journal |
| `check_entry` | this crate only (sections H–L); public so its unit tests and the screens' error text share one definition |
| `SAFETY_IDS`, `MESSAGE_AREAS` | this crate; the TypeScript-binding generator (the Language screen's "Approval wording" mark and area filter) |

### Example

`localisation.set_locale` with input `{"locale": "fr-FR"}` on a profile offering `["en-GB", "fr-FR"]` with fallback `en-GB`:

```json
{"locale":"fr-FR","source":"person","direction":"ltr","fallback_chain":["fr-FR","en-GB"]}
```

`message(id::RUN_PROGRESS_DOCUMENTS_READ, &MessageArgs::new().with("count", ArgValue::Count(12)))` with catalogue

```ftl
run-progress-documents-read = { $count ->
    [one] Read one document
   *[other] Read { NUMBER($count) } documents
}
```

returns `"Read 12 documents"` in `en-GB`, `"12 documents lus"` in `fr-FR` (given a French message `run-progress-documents-read = { $count -> [one] Un document lu *[other] { NUMBER($count) } documents lus }`).

`format_datetime(Timestamp(2026-01-10T08:15:30.123Z), DateTimeStyle::Date)` on a computer at UTC+01:00 returns `"19 Oct 2026"` in `en-GB` and `"19 oct. 2026"` in `fr-FR`.

## Internal Logic

### A. Build time (`build.rs`)

1. Read every `catalogues/<tag>/*.ftl`. Each `<tag>` must parse as a BCP 47 locale with `icu::locale::Locale::try_from_str`; a folder name that does not parse fails the build with the folder name.
2. Parse each file with `fluent_syntax::parser::parse` (a dependency of `fluent-bundle`); any parse error fails the build with file, line and message.
3. For `en-GB`: collect message identifiers; each must match `^[a-z][a-z0-9]*(-[a-z0-9]+)*$` (SA-I18N-00 kebab-case) and be unique across the folder; `locale-native-name` must exist in every locale. Violations fail the build.
   3a. *(Amended January 2026, ADR-109.)* Collect the **safety wording**: every `en-GB` identifier beginning `approval-`, and every `en-GB` message whose attached comment holds a line exactly `safety` (`# safety` directly above the message). An empty set fails the build. Record each identifier's **area**, the stem of the `en-GB` file it is in.
4. For every other locale: an identifier not present in `en-GB` fails the build (orphan message). A missing identifier is allowed (runtime fallback, ADR-042 consequence).
5. With cargo feature `pseudo-locales`: generate `en-XA` from `en-GB` (each ASCII letter replaced by an accented form — `a→á, e→é, i→í, o→ó, u→ú, c→ç, n→ñ, A→Á, E→É, I→Í, O→Ó, U→Ú` — and the text padded with `·` to 140% of its length in characters, rounded up; placeables `{ … }` kept verbatim) and `ar-XB` (the `en-GB` text wrapped in U+202E … U+202C and the locale marked right-to-left).
6. Emit `OUT_DIR/message_ids.rs` (one `pub const` per `en-GB` id, plus `MESSAGE_IDS: &[&str]`, `SAFETY_IDS: &[&str]` and `MESSAGE_AREAS: &[(&str, &str)]`, each sorted by id), and `OUT_DIR/catalogues.rs` holding each locale's concatenated FTL text as `&'static str` via `include_str!`.
7. Emit `cargo:rerun-if-changed=catalogues`.

### B. Start (`Localisation::start`)

1. Build one `FluentBundle` per bundled locale (`fluent_bundle::concurrent::FluentBundle<FluentResource>`), with `set_use_isolating(true)` so right-to-left arguments do not reorder surrounding text. Register functions `NUMBER` (argument a decimal string or count → ICU4X `icu::decimal::DecimalFormatter` in that locale) and `DATETIME` (argument an RFC 3339 string → ICU4X `icu::datetime` formatter with the `style` named argument `date`, `time`, `datetime` or `shortdate`). A resource that fails to add (duplicate id) is impossible after step A3; if it happens, log `localisation.bundle_failed` at `error` and continue with that locale removed from `bundled_locales()`.
2. Read the profile: `profile.current().document.locales`.
3. Compute **offered**: if `locales.offered` is empty, every bundled locale in `bundled_locales()` order; otherwise `locales.offered` filtered to tags that are bundled (exact tag match, case-insensitive on canonical form), keeping profile order. Each dropped tag is logged once per profile version as `localisation.offered_not_bundled` (`warn`, field `locale`). If the result is empty, offered = `["en-GB"]` and `localisation.offered_empty` is logged (`warn`). *Amended January 2026 (ADR-109):* then every person language of kind `made` or `imported` in state `in_use` is appended, by tag alphabetically (a tag already in the list is not repeated); the company's list never hides them. With feature `pseudo-locales`, `en-XA` and `ar-XB` are appended last.
   3a. *(Amended January 2026, ADR-109; store `Datastore` only.)* Before step 3, load the **person layer** (LOC-LF-1): every `person_languages` row and its `person_messages`. For each language whose `checked_against` differs from `config.app_version`, run section H's `check_entry(locale, Some(message_id), ftl)` on every entry, in one transaction: an entry whose identifier is no longer in `en-GB` is deleted; every other entry's `usable` is set to 1 when it passes and 0 when it fails; `checked_against = app_version`; log `localisation.person_layer_rechecked` (`info`, fields `locale`, `need_changing`, `removed`). A language in state `drafting` (ordin stopped during a draft) becomes `needs_review`, `draft_total` `NULL`. Then build one `FluentBundle` per person language, configured as in step 1: for kind `shipped`, the bundled resource followed by the served entries added with `add_resource_overriding`; for `made` and `imported`, the served entries only. **Served** means `usable = 1` and (the identifier is not in `SAFETY_IDS` or `reviewed = 1`). A store error here → `StoreUnavailable`, as step 5.
4. Compute **fallback** = `locales.fallback` if bundled, else `en-GB` (logged `localisation.fallback_not_bundled`, `warn`).
5. Load the preference (store `Datastore` with a SQLite pool): `SELECT locale, source, updated_at FROM locale_preferences WHERE id = 1`.
   - No row: negotiate from the operating system (step 6), insert `(1, <tag>, 'operating_system', now)`.
   - Row unreadable (a `source` value outside the two, a timestamp that does not parse): `LocalisationError::StoreCorrupt`; replace the row with the operating-system negotiation and log `localisation.preference_reset` (`warn`). Start continues.
   - Store unavailable (`sqlx::Error` on connect or query): return `StoreUnavailable` from `start` (retryable); the program shows its start-up failure.
   - Store `None` (agent profile): no row; source `operating_system`, locale = fallback.
6. **Negotiation** (fixed code, no library beyond `icu::locale`):
   a. If the row's source is `person` and its locale is in offered → use it.
   b. Else, for each tag in `os_locales` in order: exact match in offered → use it; else the first offered tag with the same language subtag (and the same script subtag when the OS tag has one) → use it.
   c. Else → fallback.
   If (a) failed because the person's locale is no longer offered, rewrite the row to `(fallback-or-OS result, 'operating_system', now)` and log `localisation.preference_not_offered` (`info`, fields `locale`, `profile_version`) — this keeps the 2C rule "one of the locales the active profile offers" true.
7. **Direction**: `icu::locale::LocaleDirectionality::new_common().is_right_to_left(&locale)` → `Rtl`, else `Ltr`; `ar-XB` is `Rtl`.
8. **Fallback chain**: `[active, fallback, "en-GB"]` with duplicates removed, keeping first occurrence.
9. Publish `ActiveLocale` on the watch channel; log `localisation.active` (`info`, fields `locale`, `source`, `direction`).
10. Spawn the follow task (C).

### C. Following the profile

1. Await `profile_rx.changed()`. On each new snapshot repeat B3, B4, B6–B9 against the stored row.
2. Publish only if `ActiveLocale` differs from the last published value (`watch::Sender::send_if_modified`).
3. The task ends when the profile sender is dropped (program shut-down).

### D. `set_locale(locale)`

1. `None`: write `(1, <OS negotiation per B6b–c>, 'operating_system', now)` with `INSERT … ON CONFLICT(id) DO UPDATE`; publish; return.
2. `Some(tag)`: parse with `icu::locale::Locale::try_from_str`; failure → `TagInvalid`. Canonicalise (`en-gb` → `en-GB`). A person language of kind `made` or `imported` whose state is not `in_use` → `LanguageNotConfirmed` (*amended January 2026, ADR-109*). Not in offered → `NotOffered`. Otherwise write `(1, tag, 'person', now)`; publish; log `localisation.locale_set` (`info`, fields `locale`, `source`); return the new `ActiveLocale`.
3. Store `None` (agent profile) → `NotOffered` for any tag but the fallback, which returns the current value without writing.
4. Store error → `StoreUnavailable` (retryable); nothing is published, the previous locale stays in force.

### E. `message(id, args)` and `message_in`

1. For each tag of the chain (for `message_in`: `[locale, fallback, "en-GB"]`, dropping a tag that is neither bundled nor a person language): look up `id` in that tag's bundle — the person-layer bundle of step B3a when the tag has one, else the bundled one (*amended January 2026, ADR-109*; in memory, no model call and no file read); the first bundle holding it formats it with the arguments converted to `FluentArgs` (`Text` → string, `Number` → the decimal string passed to `NUMBER`, `Count` → `FluentValue::Number`, `DateTime` → the RFC 3339 string passed to `DATETIME`).
2. Formatting errors that Fluent reports (a missing argument, an unknown function) are logged `localisation.format_error` at `warn` with fields `message_id`, `locale`, `error_kind` (never argument values); the string Fluent produced is returned.
3. A miss in the active locale that is served by a later tag is logged `localisation.message_fallback` at `debug` (fields `message_id`, `locale`, `served_by`).

### F. `catalogue(locale)`

1. Parse and canonicalise; invalid → `TagInvalid`; neither bundled nor a person language → `CatalogueMissing`.
2. Return the FTL text of `locale` (for a shipped locale with served overrides, the bundled text with each overridden message replaced by its served entry; for a person language, its served entries in message-id order — *amended January 2026, ADR-109*) followed by each later chain tag (`[locale, fallback, "en-GB"]`, deduplicated), with the direction. The WebView builds one `@fluent/bundle` per resource and formats with the WebView's `Intl` (SA-I18N-02).

### G. Formatting helpers

- `format_number`: parse the `DecimalString` into `fixed_decimal::Decimal` (the ICU4X type re-exported by `icu`), format with `DecimalFormatter` of the active locale.
- `format_datetime`: convert to the computer's local offset with `chrono::Local`, build ICU4X date and time fields, format with the style's field set; no time-zone name is printed (the offset is the computer's own).
- `format_list`: `icu::list::ListFormatter::try_new_and` of the active locale.
- `compare`: `icu::collator::Collator` of the active locale, default strength (tertiary).
- ICU4X data: compiled data (`icu` with feature `compiled_data`), so no data file is read at run time.

## Data Model

```sql
-- SQLite, journal database (owned by Localisation; registered as module "localisation", version 1, through `migrations()`)
CREATE TABLE locale_preferences (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    locale      TEXT    NOT NULL CHECK (length(locale) BETWEEN 2 AND 35),
    source      TEXT    NOT NULL CHECK (source IN ('operating_system', 'person')),
    updated_at  TEXT    NOT NULL  -- RFC 3339, ms, Z
);
```

- Exactly one row (`id = 1`). No PostgreSQL variant: the agent profile's scaled shape holds no preference (Journal spec, *Migration registry*).
- Rollback: the migration only creates; dropping the table loses one row, which is rebuilt from the operating system at next start.

**Step 2, `create person language tables` (amended January 2026, ADR-109).** SQLite only, in the same journal database.

```sql
CREATE TABLE person_languages (
  locale            TEXT PRIMARY KEY CHECK (length(locale) BETWEEN 2 AND 35),
  kind              TEXT NOT NULL CHECK (kind IN ('shipped','made','imported')),
  state             TEXT NOT NULL CHECK (state IN ('drafting','needs_review','in_use')),
  name              TEXT CHECK (name IS NULL OR length(name) BETWEEN 1 AND 60),  -- the name given when made; NULL for shipped
  made_for_version  TEXT NOT NULL,     -- ordin version the language was made, imported or first edited under
  checked_against   TEXT NOT NULL,     -- ordin version whose en-GB the entries were last checked against
  draft_total       INTEGER CHECK (draft_total IS NULL OR draft_total >= 0),
  created_at        TEXT NOT NULL,     -- RFC 3339, ms, Z
  updated_at        TEXT NOT NULL,
  confirmed_at      TEXT,
  CHECK (kind <> 'shipped' OR state = 'in_use')
);
CREATE TABLE person_messages (
  locale      TEXT NOT NULL REFERENCES person_languages(locale) ON DELETE CASCADE,
  message_id  TEXT NOT NULL CHECK (length(message_id) BETWEEN 1 AND 200),
  ftl         TEXT NOT NULL CHECK (length(ftl) BETWEEN 1 AND 4096),
  origin      TEXT NOT NULL CHECK (origin IN ('edited','drafted','imported')),
  reviewed    INTEGER NOT NULL CHECK (reviewed IN (0,1)),
  usable      INTEGER NOT NULL CHECK (usable IN (0,1)),
  updated_at  TEXT NOT NULL,
  PRIMARY KEY (locale, message_id)
);
```

- A `shipped` row exists only once the person has edited, restored-then-edited or imported into a shipped language; it is never `drafting` or `needs_review` (the table check).
- Limits enforced in code: at most 20 rows of kind `made` or `imported`; at most 10,000 `person_messages` per locale.
- Forward-only creation (SA-XC-07). Rollback: dropping both tables loses the person's own wording, which the Journal's backup copy restores; the compiled-in catalogues are unaffected.

## Error Table

| Condition | Status | Code | Response Body |
|---|---|---|---|
| `set_locale` or `catalogue` given a tag that does not parse as BCP 47 | `Err(permanent)`; Remote API 200 `kind: error` | `LOC_TAG_INVALID` | `{"code":"LOC_TAG_INVALID","message":"That language isn't recognised.","action":null}` |
| `set_locale` given a tag the profile does not offer | `Err(permanent)` | `LOC_NOT_OFFERED` | `{"code":"LOC_NOT_OFFERED","message":"Your company doesn't offer that language.","action":null}` |
| `catalogue` for a tag with no bundled catalogue | `Err(permanent)` | `LOC_CATALOGUE_MISSING` | `{"code":"LOC_CATALOGUE_MISSING","message":"That language isn't available in this version.","action":null}` |
| Preference store cannot be read or written (`sqlx::Error`) | `Err(retryable)` | `LOC_STORE_UNAVAILABLE` | `{"code":"LOC_STORE_UNAVAILABLE","message":"Your language choice couldn't be saved.","action":"try_again"}` |
| Preference row holds a value outside the schema | recovered: row reset, start continues; `StoreCorrupt` logged only | `LOC_STORE_CORRUPT` | — (not returned to a caller) |
| Fluent reports a formatting error for one message | recovered: string returned as Fluent produced it; logged | `LOC_FORMAT_ERROR` | — (not returned) |
| A message identifier absent from `en-GB`, a bad `.ftl` file, an orphan message in another locale, or an empty safety set (A3a) | build fails | `LOC_BUILD_CATALOGUE_INVALID` | build output names file and line |
| Any person-layer command in the agent profile (`PreferenceStore::None`) | `Err(permanent)` | `LOC_LANGUAGES_UNAVAILABLE` | `{"code":"LOC_LANGUAGES_UNAVAILABLE","message":"Languages can't be changed here.","action":null}` |
| Edit, restore, export, import or draft of `en-GB`, `en-XA` or `ar-XB` | `Err(permanent)` | `LOC_LANGUAGE_NOT_EDITABLE` | `{"code":"LOC_LANGUAGE_NOT_EDITABLE","message":"English (UK) is ordin's own wording and can't be changed.","action":null}` |
| A tag that is neither bundled nor a person language, for `list_messages`, `edit_message`, `restore_*`, `mark_reviewed`, `confirm_language`, `remove_language`, `export_language` | `Err(permanent)` | `LOC_LANGUAGE_UNKNOWN` | `{"code":"LOC_LANGUAGE_UNKNOWN","message":"That language isn't on this computer.","action":null}` |
| `edit_message`, `restore_message` or `mark_reviewed` names an identifier not in `en-GB` | `Err(permanent)` | `LOC_MESSAGE_UNKNOWN` | `{"code":"LOC_MESSAGE_UNKNOWN","message":"That message doesn't exist in this version.","action":null}` |
| `edit_message` text fails `check_entry` | `Err(permanent)`; nothing saved | `LOC_MESSAGE_REFUSED` | `{"code":"LOC_MESSAGE_REFUSED","message":"“run-progress-documents-read” wasn't saved: it must still use { $count }.","action":null}` |
| `restore_message` or `restore_language` on a made or imported language | `Err(permanent)` | `LOC_NOT_SHIPPED` | `{"code":"LOC_NOT_SHIPPED","message":"Only a language that comes with ordin can be restored.","action":null}` |
| Import: not UTF-8, no `### ordin language file` line, no `### locale:` line, or no message that parses | `Err(permanent)`; nothing stored | `LOC_FILE_UNREADABLE` | `{"code":"LOC_FILE_UNREADABLE","message":"That file isn't an ordin language file.","action":null}` |
| Import larger than 2,097,152 bytes | `Err(permanent)`; nothing stored | `LOC_FILE_TOO_LARGE` | `{"code":"LOC_FILE_TOO_LARGE","message":"That file is too large to be a language file.","action":null}` |
| Import or `begin_draft` for a tag a made or imported language already has, with `replace` false | `Err(permanent)`; nothing stored | `LOC_LANGUAGE_EXISTS` | `{"code":"LOC_LANGUAGE_EXISTS","message":"You already have that language. Choose Replace to use the new file.","action":null}` |
| Edit, review, confirm, import or remove of a language in state `drafting`, or a second `begin_draft` for it | `Err(permanent)` | `LOC_LANGUAGE_DRAFTING` | `{"code":"LOC_LANGUAGE_DRAFTING","message":"Ordin is still drafting that language.","action":null}` |
| `confirm_language` while safety entries remain unreviewed | `Err(permanent)`; state unchanged | `LOC_REVIEW_INCOMPLETE` | `{"code":"LOC_REVIEW_INCOMPLETE","message":"Check the 14 approval messages first.","action":null}` |
| `set_locale` to a made or imported language not `in_use` | `Err(permanent)`; locale unchanged | `LOC_LANGUAGE_NOT_CONFIRMED` | `{"code":"LOC_LANGUAGE_NOT_CONFIRMED","message":"Review that language and choose Use this language first.","action":null}` |
| `remove_language` of the active locale | `Err(permanent)` | `LOC_LANGUAGE_ACTIVE` | `{"code":"LOC_LANGUAGE_ACTIVE","message":"Choose another language before removing this one.","action":null}` |
| Import or `begin_draft` of a 21st made or imported language | `Err(permanent)`; nothing stored | `LOC_TOO_MANY_LANGUAGES` | `{"code":"LOC_TOO_MANY_LANGUAGES","message":"You have 20 languages of your own, the most ordin keeps.","action":null}` |

The sentences above are catalogue messages `error-loc-tag-invalid`, `error-loc-not-offered`, `error-loc-catalogue-missing`, `error-loc-store-unavailable`, and (amended January 2026, ADR-109) `error-loc-languages-unavailable`, `error-loc-language-not-editable`, `error-loc-language-unknown`, `error-loc-message-unknown`, `error-loc-message-refused` (arguments `message`, `problem`), `error-loc-not-shipped`, `error-loc-file-unreadable`, `error-loc-file-too-large`, `error-loc-language-exists`, `error-loc-language-drafting`, `error-loc-review-incomplete` (argument `count`), `error-loc-language-not-confirmed`, `error-loc-language-active`, `error-loc-too-many-languages`; the en-GB text shown is *chosen here* and subject to the Phase 4 wording review (11 §6).

## Acceptance Criteria (Gherkin)

```gherkin
Feature: Localisation

  Scenario: Happy path — first start follows the operating system
    Given the bundled locales are "en-GB" and "fr-FR"
    And the active profile offers ["en-GB", "fr-FR"] with fallback "en-GB"
    And the operating system's preferred locales are ["fr-FR"]
    And no locale_preferences row exists
    When Localisation starts
    Then the active locale is "fr-FR" with source "operating_system" and direction "ltr"
    And the fallback chain is ["fr-FR", "en-GB"]
    And locale_preferences holds one row ("fr-FR", "operating_system")

  Scenario: Edge — operating system locale matched by language subtag
    Given the profile offers ["en-GB", "de-DE"]
    And the operating system's preferred locales are ["de-AT", "en-US"]
    When Localisation starts with no stored preference
    Then the active locale is "de-DE"

  Scenario: Happy path — the person chooses a locale
    Given the profile offers ["en-GB", "fr-FR"]
    When the person sends localisation.set_locale with locale "fr-FR"
    Then the reply is an ActiveLocale with locale "fr-FR" and source "person"
    And every subscriber receives the new ActiveLocale
    And the program emits a "locale_changed" event

  Scenario: Error — locale not offered
    Given the profile offers ["en-GB"]
    When the person sends localisation.set_locale with locale "fr-FR"
    Then the reply is an error with code LOC_NOT_OFFERED
    And the active locale is unchanged

  Scenario: Error — invalid tag
    When the person sends localisation.set_locale with locale "fr_FR!"
    Then the reply is an error with code LOC_TAG_INVALID

  Scenario: Edge — a newer profile withdraws the person's locale
    Given the person chose "fr-FR"
    When a newer verified profile offering only ["en-GB"] becomes active
    Then the stored preference becomes ("en-GB", "operating_system")
    And subscribers receive ActiveLocale "en-GB"

  Scenario: Edge — message missing in the active locale falls back
    Given the active locale is "fr-FR"
    And "approval-card-send-button" exists in en-GB but not in fr-FR
    When the core formats APPROVAL_CARD_SEND_BUTTON
    Then the en-GB text is returned
    And a "localisation.message_fallback" debug line names the message id

  Scenario: Edge — right-to-left locale
    Given "ar-XB" is bundled and offered in a pseudo-locale build
    When the person chooses "ar-XB"
    Then the active direction is "rtl"
    And localisation.get_catalogue for "ar-XB" returns direction "rtl"

  Scenario: Error — catalogue for an unbundled locale
    When the WebView sends localisation.get_catalogue with locale "ja-JP" and no ja-JP catalogue is bundled
    Then the reply is an error with code LOC_CATALOGUE_MISSING

  Scenario: Error — preference store unavailable
    Given the journal database refuses writes
    When the person sends localisation.set_locale with locale "fr-FR"
    Then the reply is an error with code LOC_STORE_UNAVAILABLE and action "try_again"
    And the active locale is unchanged

  Scenario: Error — build refuses an unknown message identifier
    Given a Rust file references id::NO_SUCH_MESSAGE
    When the workspace is built
    Then compilation fails

  Scenario: Edge — plural and number rules come from CLDR
    Given the active locale is "en-GB"
    When the core formats RUN_PROGRESS_DOCUMENTS_READ with count 1 and with count 1200
    Then the results are "Read one document" and "Read 1,200 documents"

  Scenario: Edge — agent profile
    Given the program runs in the agent profile with PreferenceStore::None
    When Localisation starts
    Then the active locale is "en-GB" and no table is read or written

  Scenario: Error — a store failure carries its repair action
    Given the preference store cannot be written
    When set_locale("fr-FR") is called
    Then the error's code() is "LOC_STORE_UNAVAILABLE", retryable() is true and repair() is TryAgain

  Scenario: Happy path — a locale change reaches the screens with the whole active locale
    Given the screens are open in "en-GB" and the profile offers ["en-GB", "fr-FR"]
    When the person chooses "fr-FR"
    Then subscribe() yields ActiveLocale {locale "fr-FR", source person, direction ltr, fallback_chain ["fr-FR", "en-GB"]}
    And the event appended for the screens has kind "locale_changed" and data {"active": that ActiveLocale}
```

## Performance, Security, Observability

- **Performance targets:**
  - `message` / `error_sentence`: p95 ≤ 0.5 ms for a message with up to 5 arguments (in-memory bundles, no I/O).
  - `start`: ≤ 50 ms for up to 10 bundled locales of up to 5,000 messages each, within A-03's 3 s start.
  - `localisation.get_catalogue`: p95 ≤ 20 ms (copy of compiled-in text); `localisation.set_locale`: p95 ≤ 30 ms (one SQLite upsert) — inside A-03's 150 ms.
  - Memory: ≤ 15 MiB for bundles and ICU4X formatters for 10 locales.
  - *Amended January 2026 (ADR-109):* `message` / `error_sentence` keep p95 ≤ 0.5 ms with a person language active (same in-memory lookup). `start` adds ≤ 5 ms per person language of up to 5,000 entries when no re-check is due (the bundled budget per locale: 50 ms for 10), and ≤ 100 ms per language on the first start after an update (re-check). `edit_message`, `restore_message`, `mark_reviewed`, `confirm_language`: p95 ≤ 30 ms including the rebuild of that language's bundle (≤ 20 ms for 5,000 entries). `list_messages`: p95 ≤ 50 ms. `export_language`: p95 ≤ 200 ms for 10,000 messages. `import_language`: ≤ 1 s for a 2 MiB file of 10,000 messages. `add_drafted`: ≤ 100 ms for a 100-message batch. Memory: ≤ 1.5 MiB per person language of 5,000 entries.
- **Security:**
  - Entry points: three Service API commands (thirteen from January 2026, ADR-109), reached through the Command Bridge (Tauri capability, no authentication beyond the local trust boundary, 07) or the Remote API (desktop key, SA-XC-01). Input validated by typed deserialisation with unknown fields refused (SA-API-02), then BCP 47 parse and the offered check.
  - *Amended January 2026 (ADR-109).* Shipped catalogue text is compiled in. The only other message text is the person layer in ordin's own journal database, which enters only through `check_entry`: text the person typed, a batch Ordin drafted and returned through `add_drafted`, or the contents of a file the person picked and passed to `import_language`. No catalogue text is read from the network or the organisation profile, so a profile cannot inject message text, and no file is read at run time. Approval-card text is only catalogue text: a safety message drafted by a model or imported is never used until the person has seen it (`reviewed = 1`) or typed it, and it changes afterwards only by the person's own edit, restore or import. Formatting reads only in-memory bundles and calls no model. Text with bidirectional override or isolate characters (U+202A–U+202E, U+2066–U+2069) or C0 controls other than tab and line feed is refused, so an entry cannot reorder the words around it on screen.
  - Argument values and message text — compiled-in, edited, drafted or imported — are never logged; only message identifiers, locales, counts and error kinds.
- **Observability** (`tracing` JSON lines, SA-OBS-01; no metrics or spans on desktop and personal server, SA-XC-04/05; none added in the agent profile):

| Event | Level | Fields |
|---|---|---|
| `localisation.active` | info | `locale`, `source`, `direction` |
| `localisation.locale_set` | info | `locale`, `source` |
| `localisation.preference_not_offered` | info | `locale`, `profile_version` |
| `localisation.preference_reset` | warn | `reason` |
| `localisation.offered_not_bundled` | warn | `locale`, `profile_version` |
| `localisation.offered_empty` | warn | `profile_version` |
| `localisation.fallback_not_bundled` | warn | `locale`, `profile_version` |
| `localisation.format_error` | warn | `message_id`, `locale`, `error_kind` |
| `localisation.message_fallback` | debug | `message_id`, `locale`, `served_by` |
| `localisation.bundle_failed` | error | `locale` |
| `localisation.person_layer_rechecked` | info | `locale`, `need_changing`, `removed` |
| `localisation.message_edited` | info | `locale`, `message_id`, `change` (`edited` or `restored`) |
| `localisation.entry_refused` | info | `locale`, `message_id`, `problem` (the `EntryProblem` kind), `origin` |
| `localisation.language_reviewed` | info | `locale`, `count` |
| `localisation.language_confirmed` | info | `locale` |
| `localisation.language_removed` | info | `locale`, `kind` |
| `localisation.language_exported` | info | `locale`, `messages` |
| `localisation.language_imported` | info | `locale`, `stored`, `dropped_unknown`, `in_english`, `left_out` |
| `localisation.draft_batch` | info | `locale`, `stored`, `left_out` |
| `localisation.draft_ended` | info | `locale`, `end`, `done`, `total` |

Every line carries `component = "localisation"`.

## Gaps

None.

## Amendment January 2026 — Ordin's language and language files (ADR-109)

> Source: decision 89 (the user's decisions of January 2026); HLD revision 0.58.0, ADR-109 (amends ADR-042 and ADR-063), 11 §4, §5.19a and UX-A77. The compiled-in catalogues stay the base layer and en-GB stays compiled in and not editable; the person layer sits on top. Review before use covers the safety wording only (decision 89, item 7). A person's own languages are always offered to them; the company's `locales.offered` governs shipped languages and the fallback. Edits to a shipped language are per-message overrides. The sections above carry the change in place: Purpose; Approach; the 2C `LocalePreference.locale` rule; *Names chosen here*; `LocalisationConfig.app_version`; the person-layer methods and types, `check_entry`, `SAFETY_IDS`, `MESSAGE_AREAS`; fourteen `LocalisationError` variants; `migrations()` step 2; ten Service API commands and the event `language_changed`; the callers table; build steps A3a and A6; start steps B3 and B3a; D2; E1; F1–F2; Data Model step 2; fourteen Error Table rows; Performance, Security and Observability.

### Internal Logic (added)

#### H. The fixed check (`check_entry`)

**LOC-LF-2** One pure function guards every way text enters the person layer: an edit (I), a drafted batch (L), an import (K) and the re-check after an update (B3a). Given locale L, an optional expected identifier and one entry, it runs, in this order, and returns the first problem:

1. Length 1–4,096 characters → else `TooLong`. A C0 control other than TAB or LF, or any of U+202A–U+202E, U+2066–U+2069 → `ForbiddenCharacter`.
2. `fluent_syntax::parser::parse` returns no error and exactly one `Message` and nothing else (no `Term`, no `Junk`; comments are dropped) → else `DoesNotParse` (parse errors or junk) or `NotOneMessage`.
3. The identifier is in `en-GB` → else `UnknownMessage`; equals the expected identifier when one is given → else `IdMismatch`.
4. **Variables.** Every `$name` the `en-GB` message references (value and attributes) appears in the entry → else `VariableMissing { name }` (the first missing, by name); no `$name` the `en-GB` message lacks → else `VariableUnknown { name }`.
5. **Attributes.** The entry has the same set of attribute names as the `en-GB` message, and a value exactly when `en-GB` has one → else `AttributeMismatch`.
6. **References.** Function references only `NUMBER` and `DATETIME` → else `FunctionNotAllowed { name }`; message and term references only to identifiers and terms that exist in `en-GB` → else `ReferenceUnknown { id }`.
7. **Plural forms.** In each select expression whose selector is a variable or `NUMBER(…)`, every identifier key is either a CLDR cardinal plural category of L (`icu::plurals::PluralRules::try_new_cardinal(L)`, its `categories()`) or a key the `en-GB` message uses for a select on that same variable → else `PluralCategory { key }`. Numeric keys (`[0]`, `[1]`) are always allowed. For a tag ICU4X compiled data does not know, the categories are those of its root, `other` only (HLD 10, A-57).

`check_entry` reads only the compiled-in `en-GB` resource and ICU4X compiled data; it does no I/O.

**LOC-LF-2a — size of one language.** The `ftl` of a language's stored entries totals at most 2,097,152 characters (*chosen here*, SA-LANG-03), so every language fits one export file (LOC-LF-9) and one import (LOC-LF-10). Checked when an entry is stored, after `check_entry` passes: an edit (I) that would exceed it → `LOC_MESSAGE_REFUSED` with problem `LanguageFull`; a drafted entry (L) that would exceed it is left out and counted in `left_out`. An import cannot exceed it, because the file itself is at most 2,097,152 bytes.

**Problem sentences** (en-GB, *chosen here*; message ids `language-problem-<kebab of the variant>`; `$name`, `$id` and `$key` are `ArgValue::Text`). They fill `$problem` in `LOC_MESSAGE_REFUSED`'s sentence and the left-out list of an import.

| Variant | Message id | en-GB text |
|---|---|---|
| `TooLong` | `language-problem-too-long` | it is longer than 4,096 characters. |
| `ForbiddenCharacter` | `language-problem-forbidden-character` | it contains a hidden control character. |
| `DoesNotParse` | `language-problem-does-not-parse` | its wording can't be read; check the braces and the line after the equals sign. |
| `NotOneMessage` | `language-problem-not-one-message` | it holds more than one message. |
| `UnknownMessage` | `language-problem-unknown-message` | this version has no message with that name. |
| `IdMismatch` | `language-problem-id-mismatch` | the message name before the equals sign was changed. |
| `VariableMissing` | `language-problem-variable-missing` | it leaves out { $name }, which the English uses. |
| `VariableUnknown` | `language-problem-variable-unknown` | it uses { $name }, which the English doesn't. |
| `AttributeMismatch` | `language-problem-attribute-mismatch` | its parts don't match the English message's parts. |
| `FunctionNotAllowed` | `language-problem-function-not-allowed` | it uses { $name }, which isn't allowed here. |
| `ReferenceUnknown` | `language-problem-reference-unknown` | it refers to { $id }, which doesn't exist. |
| `PluralCategory` | `language-problem-plural-category` | “{ $key }” isn't a plural form of this language. |
| `LanguageFull` | `language-problem-language-full` | this language has reached the most wording ordin keeps for one language. |

#### I. Edit, restore, review, confirm, remove

Every command in I–K first refuses: store `None` → `LanguagesUnavailable`; a tag that does not parse → `TagInvalid`; `en-GB`, `en-XA`, `ar-XB` → `LanguageNotEditable`; a tag neither bundled nor in `person_languages` → `LanguageUnknown`; a language in state `drafting` (except reads) → `LanguageDrafting`. Writes run in one SQLite transaction; a store error → `StoreUnavailable` and nothing changes in memory.

1. **LOC-LF-3** `edit_message(locale, message_id, ftl)`: identifier not in `en-GB` → `MessageUnknown`; `check_entry(locale, Some(message_id), ftl)` fails → `MessageRefused { message_id, problem }`, nothing written, log `localisation.entry_refused`. Otherwise, for a bundled locale with no `person_languages` row, insert one (`kind = 'shipped'`, `state = 'in_use'`, `made_for_version = checked_against = app_version`); upsert `person_messages (locale, message_id, ftl, 'edited', reviewed 1, usable 1, now)`; rebuild that locale's bundle; emit `LanguageChange { locale, Edited }`; log `localisation.message_edited`. Return the `MessageRow`. The compiled-in catalogue is never changed.
2. **LOC-LF-4** `restore_message(locale, message_id)`: kind `made` or `imported` → `NotShipped`; identifier not in `en-GB` → `MessageUnknown`; delete the row (no row is not an error); rebuild; emit `Restored`; return the row, now `source: shipped` (or `english` when the shipped catalogue lacks it). `restore_language(locale)`: kind check as above; delete the language's `person_messages`; rebuild; emit `Restored`; return the summary.
3. **LOC-LF-5** `mark_reviewed(locale, message_ids)`: 1–200 identifiers → else `InputInvalid` at the Service API (SA-API-02); each must be in `en-GB` → else `MessageUnknown`; set `reviewed = 1` on the stored rows among them (identifiers with no stored row, or not in `SAFETY_IDS`, are ignored); rebuild; emit `Reviewed`; log `localisation.language_reviewed`. A safety entry becomes served from this moment.
4. **LOC-LF-6** `confirm_language(locale)`: kind `shipped` → return the summary unchanged; any stored safety entry with `reviewed = 0` and `usable = 1` → `ReviewIncomplete { count }`; else `state = 'in_use'`, `confirmed_at = now`; recompute offered (B3); emit `Confirmed`; log `localisation.language_confirmed`. A safety message the language lacks shows in `en-GB` and needs no review.
5. **LOC-LF-7** `remove_language(locale)`: the active locale → `LanguageActive`; kind `shipped` → as `restore_language` and the row is deleted; else delete the row (messages cascade), drop its bundle, recompute offered; emit `Removed`; log `localisation.language_removed`.
6. **LOC-LF-8** `languages()`, `messages()`: computed from memory. `messages` filters by `text` (case-folded with `icu::casemap` simple folding, matched against identifier, `en-GB` text and the language's text), `area`, and `only`; ordered by identifier; cursor = the last identifier returned, opaque to the caller (2C `Cursor`). `MessageRow.source`: a stored row → its origin; else the shipped catalogue holds the message → `shipped`; else `english`.

#### J. Export

**LOC-LF-9** `export_language(locale)`: refusals as I (except `LanguageDrafting`, which does not apply to a read). The file is:

```
### ordin language file
### locale: <tag>
### made-for: <app_version>
### origin: <shipped-with-edits | drafted | imported>

<entries, one blank line between them, ordered by message identifier>
```

For a shipped locale, every message of the shipped catalogue with each served override in its place (`origin: shipped-with-edits`, also when there is no edit); for a made language (`drafted`) or an imported one (`imported`), its served entries only. Messages that show in English are left out, so they fall back on the receiving installation too. `file_name` = `ordin-language-<tag>.ftl`. The import limit of 2 MiB bounds a person language's text, and the shipped text is bounded by the build; log `localisation.language_exported`. Emits no change.

#### K. Import

**LOC-LF-10** `import_language(file_name, ftl, replace)`, refusals in this order, each leaving everything unchanged: store `None` → `LanguagesUnavailable`; more than 2,097,152 bytes → `FileTooLarge`; no line `### ordin language file`, no line `### locale: <tag>`, or no entry that parses → `FileUnreadable`; the tag does not parse → `TagInvalid`; `en-GB` or a pseudo-locale → `LanguageNotEditable`; a made or imported language with that tag in state `drafting` → `LanguageDrafting`; such a language exists and `replace` is false → `LanguageExists`; a new person language would be the 21st → `TooManyLanguages`. Then, per entry: an identifier not in `en-GB` → dropped, counted in `dropped_unknown`; `check_entry(tag, None, entry)` fails → left out, counted in `left_out`, its identifier in `left_out_ids` (first 100). A file that keeps no entry after these steps → `FileUnreadable`.

1. **LOC-LF-11** A bundled tag: each kept entry whose text differs from the shipped entry is stored as an override (`origin = 'imported'`, `reviewed = 0`, `usable = 1`), replacing any earlier override of that message; the language stays in use. An imported override of a safety message is not served until reviewed; the shipped text shows meanwhile.
2. **LOC-LF-12** Another tag: a new row `kind = 'imported'`, `state = 'needs_review'`, `made_for_version` = the file's `made-for` line when present, else `app_version`; every kept entry stored with `origin = 'imported'`, `reviewed = 0`. With `replace`, the language's earlier entries are deleted in the same transaction; an `in_use` language stays `in_use` (its unreviewed safety entries are not served until reviewed), any other state becomes `needs_review`.
3. `in_english` = `en-GB` messages with no served entry afterwards (for a bundled tag, and none in the shipped catalogue). Rebuild the bundle; emit `Imported`; log `localisation.language_imported`; return the `ImportReport`.

#### L. Drafting support for the Ordin Host (X-30)

Localisation calls no model. The Ordin Host drafts; every drafted entry reaches the person layer only through `add_drafted`, which runs `check_entry`.

1. **LOC-LF-13** `match_language(requested)`: parse and canonicalise → `TagInvalid`; exact match in offered → `Choosable`; else the first offered tag with the same language subtag (and the same script subtag when `requested` has one) → `Choosable`; else a person language of that tag, or of the same language, not `in_use` → `NotConfirmed`; else a bundled tag (exact or same language) the company does not offer → `NotOffered`; else `Missing { locale: <canonical tag> }`. Store `None`: `Missing` and `NotConfirmed` become `NotOffered`.
2. **LOC-LF-14** `draft_sources(locale)`: refusals as I; the `en-GB` messages with no served or stored entry in that language (for a bundled tag, also none in the shipped catalogue), each with its `en-GB` entry, in identifier order. An empty list is a valid result.
3. **LOC-LF-15** `begin_draft(locale, name, total)`: refusals as I except `LanguageUnknown` (a new tag is the point) and with `LanguageDrafting` when already drafting; name 1–60 characters → else `InputInvalid` at the Ordin Host. No row → insert `kind = 'made'`, `state = 'drafting'`, `name`, `draft_total = total` (21st → `TooManyLanguages`). An existing made or imported row → `state = 'drafting'`, `draft_total = total` (its entries and confirmation are kept: drafting fills gaps). A shipped row or bundled tag with no row → a `shipped` row stays `in_use` and holds `draft_total` (the drafted entries are overrides). Emit `DraftStarted`.
4. **LOC-LF-16** `add_drafted(locale, reply)`: the language must be in state `drafting` (or a shipped row with `draft_total` set) → else `LanguageUnknown`. Parse `reply` with `fluent_syntax::parser::parse`; junk is skipped; each `Message` is checked with `check_entry(locale, None, …)`: an identifier that already has an `edited` row is skipped (the person's own words win); a pass is upserted with `origin = 'drafted'`, `reviewed = 0`, `usable = 1`; a failure counts as `left_out` and is logged `localisation.entry_refused` (`origin: drafted`). Non-safety drafted entries of a language already `in_use` are served at once; safety ones wait for review. Rebuild; emit `DraftProgress`; log `localisation.draft_batch`. Returns the counts.
5. **LOC-LF-17** `end_draft(locale, end)`: a made or imported language → `needs_review` when any safety entry has `reviewed = 0` or the language was never confirmed, else back to `in_use`; `draft_total` `NULL`; a shipped row keeps `in_use`. Emit `DraftEnded`; log `localisation.draft_ended`. Returns the summary.

#### M. Language changes

**LOC-LF-18** Every write in I, K and L sends one `LanguageChange` on a `tokio::sync::broadcast` channel of capacity 64. The programs forward each as the event `language_changed` (`{locale, change}`) through `Journal::append_event`. When the changed language is the active locale or in its fallback chain, the screens reload its text with `localisation.get_catalogue`; the watch channel of `ActiveLocale` is not touched, so Dictation and Brand do not restart. A change that alters offered (`Confirmed`, `Removed`) recomputes offered; if the active locale is no longer offered (it was removed while active is refused, so this arises only from a profile change), C applies as before.

### Example

`edit_message("fr-FR", "run-progress-documents-read", …)` with the entry

```ftl
run-progress-documents-read = { $n ->
    [one] Un document lu
   *[other] Plusieurs documents lus
}
```

fails step H4: the `en-GB` message uses `$count`, so the first problem is `VariableMissing { name: "count" }`. Nothing is written, and the reply is `{"code":"LOC_MESSAGE_REFUSED","message":"“run-progress-documents-read” wasn't saved: it must still use { $count }.","action":null}`. The same text with `{ $count -> [one] Un document lu *[other] { NUMBER($count) } documents lus }` passes (French cardinal categories are `one`, `many` and `other`) and is stored with `origin = 'edited'`.

`export_language("fr-FR")` on version 1.4.0 after that edit returns `file_name` `ordin-language-fr-FR.ftl` and:

```ftl
### ordin language file
### locale: fr-FR
### made-for: 1.4.0
### origin: shipped-with-edits

approval-card-send-button = Envoyer

run-progress-documents-read = { $count ->
    [one] Un document lu
   *[other] { NUMBER($count) } documents lus
}
```

(two of the file's messages shown).

### Acceptance Criteria (Gherkin) — added scenarios

```gherkin
  Scenario Outline: Each fixed check refuses a failing entry and accepts a passing one
    Given the en-GB message "run-progress-documents-read" uses { $count } with keys "one" and "other"
    When check_entry runs for locale "fr-FR" with the entry <entry>
    Then the result is <result>

    Examples:
      | entry                                                                                   | result                                   |
      | "run-progress-documents-read = { $count -> [one] Un document lu *[other] { NUMBER($count) } documents lus }" | Ok                       |
      | "run-progress-documents-read = { $count -> [one] Un *[other] Deux"                      | DoesNotParse                             |
      | "run-progress-documents-read = Lu\nother-message = Lu"                                  | NotOneMessage                            |
      | "no-such-message = Lu"                                                                  | UnknownMessage                           |
      | "run-progress-documents-read = Documents lus"                                           | VariableMissing count                    |
      | "run-progress-documents-read = { $count } { $user } lus"                                | VariableUnknown user                     |
      | "run-progress-documents-read = { $count -> [few] Peu *[other] { $count } }"             | PluralCategory few                       |
      | "run-progress-documents-read = { $count -> [many] Beaucoup *[other] { $count } lus }"   | Ok                                       |
      | "run-progress-documents-read = { UPPER($count) }"                                       | FunctionNotAllowed UPPER                 |
      | "run-progress-documents-read = { $count } ‮ lus"                                   | ForbiddenCharacter                       |

  Scenario: Error — an edit that drops a variable is refused and nothing changes
    Given the person has no override of "run-progress-documents-read" in "fr-FR"
    When localisation.edit_message is sent for "fr-FR", "run-progress-documents-read" with "run-progress-documents-read = Documents lus"
    Then the reply is an error with code LOC_MESSAGE_REFUSED naming "run-progress-documents-read"
    And person_messages holds no row for that message

  Scenario: Error — an edit that does not parse is refused
    When localisation.edit_message is sent for "fr-FR", "run-progress-documents-read" with "run-progress-documents-read = { $count"
    Then the reply is an error with code LOC_MESSAGE_REFUSED

  Scenario: Happy path — an edit shows without a restart and restore brings the shipped text back
    Given the active locale is "fr-FR"
    When the person saves a valid new text for "run-progress-documents-read" in "fr-FR"
    Then a language_changed event with locale "fr-FR" and change "edited" is emitted
    And localisation.get_catalogue for "fr-FR" returns the new text
    And formatting RUN_PROGRESS_DOCUMENTS_READ returns the new text
    And the compiled-in fr-FR catalogue is unchanged
    When localisation.restore_message is sent for "fr-FR", "run-progress-documents-read"
    Then formatting RUN_PROGRESS_DOCUMENTS_READ returns the shipped text

  Scenario: Error — restore on a language the person made
    Given "ja-JP" is a made language
    When localisation.restore_language is sent for "ja-JP"
    Then the reply is an error with code LOC_NOT_SHIPPED

  Scenario: Error — en-GB cannot be edited
    When localisation.edit_message is sent for "en-GB", "approval-card-send-button" with any text
    Then the reply is an error with code LOC_LANGUAGE_NOT_EDITABLE

  Scenario: Error — unknown language and unknown message
    When localisation.list_messages is sent for "xx-YY"
    Then the reply is an error with code LOC_LANGUAGE_UNKNOWN
    When localisation.edit_message is sent for "fr-FR", "no-such-message" with "no-such-message = Lu"
    Then the reply is an error with code LOC_MESSAGE_UNKNOWN

  Scenario: Served rule — an unreviewed safety message is not used, a reviewed one is
    Given "ja-JP" is a made language in use with a drafted, unreviewed entry for "approval-card-send-button"
    And the active locale is "ja-JP"
    When the core formats APPROVAL_CARD_SEND_BUTTON
    Then the en-GB text is returned
    When localisation.mark_reviewed is sent for "ja-JP" with ["approval-card-send-button"]
    And the core formats APPROVAL_CARD_SEND_BUTTON
    Then the ja-JP text is returned

  Scenario: Served rule — a drafted non-safety message is used without review
    Given "ja-JP" is a made language in use with a drafted, unreviewed entry for "run-progress-documents-read"
    And the active locale is "ja-JP"
    When the core formats RUN_PROGRESS_DOCUMENTS_READ with count 3
    Then the ja-JP text is returned

  Scenario: Error — confirming before the safety wording is checked
    Given "ja-JP" is a made language in state "needs_review" with 14 unreviewed safety entries
    When localisation.confirm_language is sent for "ja-JP"
    Then the reply is an error with code LOC_REVIEW_INCOMPLETE
    And the state is still "needs_review"

  Scenario: Happy path — confirming makes the language choosable
    Given "ja-JP" is a made language in state "needs_review" whose safety entries are all reviewed
    When localisation.confirm_language is sent for "ja-JP"
    Then its state is "in_use" and localisation.get_settings lists "ja-JP" among the offered locales
    And a language_changed event with change "confirmed" is emitted

  Scenario: Error — an unconfirmed language cannot become active
    Given "ja-JP" is a made language in state "needs_review"
    When localisation.set_locale is sent with locale "ja-JP"
    Then the reply is an error with code LOC_LANGUAGE_NOT_CONFIRMED
    And the active locale is unchanged

  Scenario: Edge — the company's list does not hide the person's own language
    Given the profile offers ["en-GB"] only
    And "ja-JP" is a made language in use
    When localisation.set_locale is sent with locale "ja-JP"
    Then the reply is an ActiveLocale with locale "ja-JP", source "person" and fallback_chain ["ja-JP", "en-GB"]

  Scenario: Error — the active language cannot be removed
    Given the active locale is the made language "ja-JP"
    When localisation.remove_language is sent for "ja-JP"
    Then the reply is an error with code LOC_LANGUAGE_ACTIVE

  Scenario: Error — a language being drafted cannot be edited
    Given "ja-JP" is in state "drafting"
    When localisation.edit_message is sent for "ja-JP", "run-progress-documents-read" with a valid entry
    Then the reply is an error with code LOC_LANGUAGE_DRAFTING

  Scenario: Happy path — export names the locale and version
    Given the application version is "1.4.0" and "fr-FR" has one edited message
    When localisation.export_language is sent for "fr-FR"
    Then file_name is "ordin-language-fr-FR.ftl"
    And the file parses as Fluent, its first lines are "### ordin language file", "### locale: fr-FR", "### made-for: 1.4.0", "### origin: shipped-with-edits"
    And it holds the edited text

  Scenario: Happy path — import counts what it drops and what shows in English
    Given another installation's en-GB has 1,200 messages
    And a file for "fr-FR" made there holds 1,183 messages, 3 of them unknown here, one with an edited text
    When localisation.import_language is sent with replace false
    Then the report has stored 1, dropped_unknown 3 and left_out 0
    And the edited override is stored with origin "imported"

  Scenario: Happy path — an imported new language waits for review, then becomes choosable
    Given no "pt-BR" language exists
    When the person imports a valid "pt-BR" file holding 1,180 messages, 9 of them safety wording
    Then "pt-BR" is kind "imported", state "needs_review" with needs_review 9 and in_english 20
    And localisation.set_locale with "pt-BR" is refused with LOC_LANGUAGE_NOT_CONFIRMED
    When the person marks the 9 safety messages reviewed and confirms "pt-BR"
    Then localisation.set_locale with "pt-BR" succeeds

  Scenario Outline: Import refused, nothing stored
    When localisation.import_language is sent with <file>
    Then the reply is an error with code <code>
    And person_languages and person_messages are unchanged and the active locale is unchanged

    Examples:
      | file                                                       | code                      |
      | a PDF's bytes as text                                      | LOC_FILE_UNREADABLE       |
      | a Fluent file with no "### ordin language file" line       | LOC_FILE_UNREADABLE       |
      | 2,097,153 bytes                                            | LOC_FILE_TOO_LARGE        |
      | a language file with "### locale: fr_FR!"                  | LOC_TAG_INVALID           |
      | a language file for "en-GB"                                | LOC_LANGUAGE_NOT_EDITABLE |
      | a "pt-BR" file while "pt-BR" exists, replace false         | LOC_LANGUAGE_EXISTS       |
      | a "pt-BR" file while "pt-BR" is being drafted              | LOC_LANGUAGE_DRAFTING     |
      | a new language when 20 of the person's own exist           | LOC_TOO_MANY_LANGUAGES    |

  Scenario: Edge — drafted entries pass through the checker, failures are counted
    Given begin_draft was called for "ja-JP" named "Japanese" with total 100
    When add_drafted receives a reply of 100 messages, 2 of which drop a variable
    Then 98 entries are stored with origin "drafted" and reviewed 0
    And the outcome is stored 98, left_out 2
    And end_draft(Finished) leaves "ja-JP" in state "needs_review"

  Scenario: Edge — the person's own edit is not overwritten by a draft
    Given "ja-JP" has an edited entry for "run-progress-documents-read"
    When add_drafted receives a valid entry for "run-progress-documents-read"
    Then the edited entry is unchanged

  Scenario: Edge — a restart during a draft leaves the language to review
    Given "ja-JP" is in state "drafting" when ordin stops
    When Localisation starts
    Then "ja-JP" is in state "needs_review" and draft_sources("ja-JP") lists only the messages not yet drafted

  Scenario: Edge — after an update, new messages show in English and failing entries need changing
    Given "ja-JP" was checked against version "1.4.0" and is in use
    And version "1.5.0" adds 12 en-GB messages and adds { $name } to one message "ja-JP" holds
    When Localisation starts with app_version "1.5.0"
    Then "ja-JP" reports in_english 13 and needs_changing 1
    And formatting each of those 13 messages returns the en-GB text
    And a "localisation.person_layer_rechecked" line names "ja-JP"

  Scenario: Edge — match_language
    Given the profile offers ["en-GB", "fr-FR", "de-DE"] and the company does not offer bundled "es-ES"
    And "ja-JP" is a made language in state "needs_review"
    Then match_language("fr") is Choosable "fr-FR"
    And match_language("ja-JP") is NotConfirmed "ja-JP"
    And match_language("es") is NotOffered "es-ES"
    And match_language("ko") is Missing "ko"

  Scenario: Edge — a made right-to-left language
    Given "ar" is a made language in use
    When the person chooses "ar"
    Then the ActiveLocale has direction "rtl"

  Scenario: Error — the agent profile has no person layer
    Given the program runs in the agent profile with PreferenceStore::None
    When localisation.edit_message, localisation.import_language or localisation.export_language is sent, or begin_draft is called
    Then each reply is an error with code LOC_LANGUAGES_UNAVAILABLE
    And the active locale stays "en-GB"

  Scenario: Performance — formatting with a person language does no I/O
    Given "ja-JP" is a made language in use and active
    When the core formats 10,000 messages with up to 5 arguments each
    Then no file is opened, no database query runs and no model is called
    And the p95 time per message is at most 0.5 ms

  Scenario: Existing gates still hold
    Given the en-GB catalogue lacks a message a Rust file references
    When the workspace is built
    Then compilation fails
    And with feature pseudo-locales the en-XA and ar-XB screen tests still pass

  Scenario: Error — an empty safety set fails the build
    Given no en-GB identifier begins "approval-" and no en-GB message carries "# safety"
    When the workspace is built
    Then the build fails with LOC_BUILD_CATALOGUE_INVALID
```

### Performance, Security, Observability (added)

- **Performance:** the targets added under *Performance targets* above; the re-check of B3a runs once per language per version.
- **Security:** person-layer text enters only through `check_entry` (H); safety wording drafted or imported is unused until the person has checked it; bidirectional control characters are refused; no catalogue text from the network or the profile; import reads only the text the program passes from a file the person picked; no message text in logs or events (`language_changed` carries a tag and a change kind only).
- **Observability:** the ten log events added to the table above.

### Gaps

None.
