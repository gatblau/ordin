//! The person layer (Localisation.md, amendment January 2026, ADR-109; sections I–M): per-message
//! edits of a shipped language, languages made with Ordin or imported, export and import of one
//! language file, and the drafting support the Ordin Host calls.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::PoisonError;

use icu::casemap::CaseMapper;
use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use sqlx::SqlitePool;

use crate::catalogue::{canonical_tag, english, is_pseudo, messages_in, SOURCE_LOCALE};
use crate::check::check_entry;
use crate::error::LocalisationError;
use crate::fmt::direction_of;
use crate::service::{
    all_areas, is_safety, is_served, now_wire, same_language, Localisation, Person, State,
};
use crate::store::{self, LangRow, MsgRow, Origin};
use crate::types::{
    DraftBatchOutcome, DraftEnd, DraftProgress, DraftSource, EntryProblem, ImportReport,
    LanguageChange, LanguageChangeKind, LanguageFile, LanguageKind, LanguageMatch, LanguageState,
    LanguageSummary, MessageFilter, MessageOnly, MessageRow, MessageSource,
};
use crate::{COMPONENT, MESSAGE_AREAS};

/// The most characters one language's stored entries may total (LOC-LF-2a).
const MAX_LANGUAGE_CHARS: usize = 2_097_152;
/// The most messages one language may store.
const MAX_LANGUAGE_MESSAGES: usize = 10_000;
/// The most made or imported languages.
const MAX_OWN_LANGUAGES: usize = 20;
/// The longest name a language is made with.
const MAX_NAME_CHARS: usize = 60;
/// The longest `made-for` value an import keeps.
const MAX_VERSION_CHARS: usize = 64;
/// The most identifiers an import report lists.
const MAX_LEFT_OUT_IDS: usize = 100;

/// Which refusals a command applies before its own checks (section I).
#[derive(Clone, Copy)]
struct Guard {
    /// `en-GB` and the pseudo-locales are refused.
    editable: bool,
    /// A tag that is neither bundled nor a person language is refused.
    unknown: bool,
    /// A language being drafted is refused.
    drafting: bool,
}

const WRITE: Guard = Guard {
    editable: true,
    unknown: true,
    drafting: true,
};
const READ: Guard = Guard {
    editable: false,
    unknown: true,
    drafting: false,
};

fn is_drafting(person: &Person) -> bool {
    person.row.state == LanguageState::Drafting || person.row.draft_total.is_some()
}

fn fold(text: &str) -> String {
    CaseMapper::new().fold_string(text).into_owned()
}

fn area_of(id: &str) -> String {
    MESSAGE_AREAS
        .binary_search_by(|(candidate, _)| (*candidate).cmp(id))
        .ok()
        .and_then(|at| MESSAGE_AREAS.get(at))
        .map(|(_, area)| (*area).to_string())
        .unwrap_or_default()
}

/// Line breaks of a file or a reply are LF before the check (a CR is a refused control character).
fn lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Normalises a vetted entry to the stored form.
fn normalised(text: &str) -> Option<String> {
    messages_in(text).into_iter().next().map(|(_, entry)| entry)
}

impl Localisation {
    fn pool(&self) -> Result<&SqlitePool, LocalisationError> {
        self.inner
            .pool
            .as_ref()
            .ok_or(LocalisationError::LanguagesUnavailable)
    }

    /// The refusals every command of I–K applies first; returns the canonical tag.
    fn guard(&self, locale: &str, guard: Guard) -> Result<String, LocalisationError> {
        self.pool()?;
        let tag = canonical_tag(locale).ok_or(LocalisationError::TagInvalid)?;
        if guard.editable && (tag == SOURCE_LOCALE || is_pseudo(&tag)) {
            return Err(LocalisationError::LanguageNotEditable);
        }
        let state = self.read_state();
        let person = state.people.get(&tag);
        if guard.unknown && person.is_none() && !self.inner.bundled.contains_key(&tag) {
            return Err(LocalisationError::LanguageUnknown);
        }
        if guard.drafting && person.is_some_and(is_drafting) {
            return Err(LocalisationError::LanguageDrafting);
        }
        Ok(tag)
    }

    pub(crate) fn emit(&self, locale: &str, change: LanguageChangeKind) {
        // No receiver is not an error: the programs forward changes, tests may not listen.
        let _ = self.inner.changes.send(LanguageChange {
            locale: locale.to_string(),
            change,
        });
    }

    /// Reloads one language from the database into memory and rebuilds its bundle (the write has
    /// committed). Recomputes the offered list, which a confirmation, removal or import changes.
    async fn sync_language(&self, locale: &str) -> Result<(), LocalisationError> {
        let pool = self.pool()?;
        let loaded = store::load_language(pool, locale).await?;
        let rebuilt = loaded.map(|(row, messages)| {
            let bundle = Localisation::build_person_bundle(
                &row,
                &messages,
                &self.inner.bundled,
                &self.inner.formatters,
            );
            (row, messages, bundle)
        });
        let mut state = self
            .inner
            .state
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        match rebuilt {
            Some((row, messages, bundle)) => {
                let draft_done = state.people.get(locale).map_or(0, |p| p.draft_done);
                state.people.insert(
                    locale.to_string(),
                    Person {
                        row,
                        messages,
                        bundle,
                        draft_done,
                    },
                );
            }
            None => {
                state.people.remove(locale);
            }
        }
        self.recompute_offered(&mut state);
        Ok(())
    }

    fn set_draft_done(&self, locale: &str, done: u32) {
        let mut state = self
            .inner
            .state
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(person) = state.people.get_mut(locale) {
            person.draft_done = done;
        }
    }

    fn own_language_count(&self) -> usize {
        self.read_state()
            .people
            .values()
            .filter(|person| person.row.kind != LanguageKind::Shipped)
            .count()
    }

    /// Runs `check_entry`, normalises the entry and logs a refusal.
    fn vet(
        &self,
        locale: &str,
        expected: Option<&str>,
        message_id: &str,
        text: &str,
        origin: &'static str,
    ) -> Result<String, EntryProblem> {
        let verdict = check_entry(locale, expected, text).and_then(|()| {
            let entry = normalised(text).ok_or(EntryProblem::NotOneMessage)?;
            // The stored form is what is served, so it passes the same check.
            check_entry(locale, expected, &entry).map(|()| entry)
        });
        if let Err(problem) = &verdict {
            tracing::info!(
                component = COMPONENT,
                locale = locale,
                message_id = message_id,
                problem = problem.kind(),
                origin = origin,
                "localisation.entry_refused"
            );
        }
        verdict
    }

    /// Whether storing `new_chars` for `message_id` would pass the size limits (LOC-LF-2a).
    // spec: ordin-desktop/components/Localisation#LOC-LF-2a
    fn fits(person: Option<&Person>, message_id: &str, new_chars: usize) -> bool {
        let (total, count) = person.map_or((0, 0), |person| {
            person
                .messages
                .iter()
                .filter(|(id, _)| id.as_str() != message_id)
                .fold((0usize, 0usize), |(chars, count), (_, row)| {
                    (chars + row.ftl.chars().count(), count + 1)
                })
        });
        total + new_chars <= MAX_LANGUAGE_CHARS && count < MAX_LANGUAGE_MESSAGES
    }

    // ------------------------------------------------------------------ reading

    /// Every bundled language and every person language; en-GB first, then by tag.
    pub fn languages(&self) -> Vec<LanguageSummary> {
        let state = self.read_state();
        let mut tags: BTreeSet<String> = self.inner.bundled.keys().cloned().collect();
        tags.extend(state.people.keys().cloned());
        let mut ordered: Vec<String> = tags.into_iter().collect();
        ordered.sort_by_key(|tag| (tag != SOURCE_LOCALE, tag.clone()));
        ordered
            .iter()
            .map(|tag| self.summary_in(&state, tag))
            .collect()
    }

    /// Every area (en-GB file stem), sorted.
    pub fn areas(&self) -> Vec<String> {
        all_areas()
    }

    pub(crate) fn summary_in(&self, state: &State, tag: &str) -> LanguageSummary {
        let person = state.people.get(tag);
        let bundled = self.inner.bundled.get(tag);
        let (kind, language_state) = match person {
            Some(person) => (person.row.kind, person.row.state),
            None => (LanguageKind::Shipped, LanguageState::InUse),
        };
        let mut edited = 0u32;
        let mut needs_review = 0u32;
        let mut needs_changing = 0u32;
        let mut served = BTreeSet::new();
        if let Some(person) = person {
            for (id, row) in &person.messages {
                if is_served(id, row) {
                    served.insert(id.as_str());
                    if row.origin == Origin::Edited {
                        edited += 1;
                    }
                }
                if !row.reviewed && is_safety(id) {
                    needs_review += 1;
                }
                if !row.usable {
                    needs_changing += 1;
                }
            }
        }
        let in_english = crate::id::MESSAGE_IDS
            .iter()
            .filter(|id| {
                !served.contains(**id) && !bundled.is_some_and(|b| b.bundle.has_message(id))
            })
            .count();
        let native_name = self
            .native_name(state, tag)
            .or_else(|| person.and_then(|person| person.row.name.clone()));
        LanguageSummary {
            locale: tag.to_string(),
            native_name,
            direction: direction_of(tag),
            kind,
            state: language_state,
            choosable: state.offered.iter().any(|offered| offered == tag),
            offered_by_company: bundled.is_some()
                && (state.company.iter().any(|offered| offered == tag) || is_pseudo(tag)),
            edited,
            in_english: u32::try_from(in_english).unwrap_or(u32::MAX),
            needs_review,
            needs_changing,
            made_for_version: person.map(|person| person.row.made_for_version.clone()),
            draft: person.and_then(|person| {
                person.row.draft_total.map(|total| DraftProgress {
                    done: person.draft_done,
                    total,
                })
            }),
        }
    }

    fn summary(&self, tag: &str) -> LanguageSummary {
        let state = self.read_state();
        self.summary_in(&state, tag)
    }

    fn row_in(&self, state: &State, tag: &str, id: &str) -> MessageRow {
        let english = english();
        let stored = state
            .people
            .get(tag)
            .and_then(|person| person.messages.get(id));
        let shipped = self
            .inner
            .bundled
            .get(tag)
            .and_then(|bundled| bundled.entries().get(id))
            .cloned();
        let source = match (stored, &shipped) {
            (Some(row), _) => match row.origin {
                Origin::Edited => MessageSource::Edited,
                Origin::Drafted => MessageSource::Drafted,
                Origin::Imported => MessageSource::Imported,
            },
            (None, Some(_)) => MessageSource::Shipped,
            (None, None) => MessageSource::English,
        };
        MessageRow {
            message_id: id.to_string(),
            area: area_of(id),
            english: english.entries.get(id).cloned().unwrap_or_default(),
            text: stored
                .map(|row| row.ftl.clone())
                .or_else(|| shipped.clone()),
            shipped,
            source,
            safety: is_safety(id),
            reviewed: stored.is_none_or(|row| row.reviewed),
            needs_changing: stored.is_some_and(|row| !row.usable),
        }
    }

    fn row(&self, tag: &str, id: &str) -> MessageRow {
        let state = self.read_state();
        self.row_in(&state, tag, id)
    }

    /// One page (100) of messages of `locale`, in message-id order.
    // spec: ordin-desktop/components/Localisation#LOC-LF-8
    pub fn messages(
        &self,
        locale: &str,
        filter: &MessageFilter,
        cursor: Option<Cursor>,
    ) -> Result<Page<MessageRow>, LocalisationError> {
        let tag = self.guard(locale, READ)?;
        let after: Option<String> = cursor
            .map(|cursor| cursor.decode::<String>())
            .transpose()
            .map_err(|_| LocalisationError::BadCursor)?;
        let needle = filter.text.as_deref().map(fold);
        let state = self.read_state();
        let mut items: Vec<MessageRow> = Vec::new();
        let mut more = false;
        for id in crate::id::MESSAGE_IDS {
            if after.as_deref().is_some_and(|last| *id <= last) {
                continue;
            }
            if filter
                .area
                .as_deref()
                .is_some_and(|area| area != area_of(id))
            {
                continue;
            }
            let row = self.row_in(&state, &tag, id);
            if let Some(only) = filter.only {
                let keep = match only {
                    MessageOnly::NeedsReview => row.safety && !row.reviewed,
                    MessageOnly::InEnglish => row.source == MessageSource::English,
                    MessageOnly::Edited => row.source == MessageSource::Edited,
                    MessageOnly::NeedsChanging => row.needs_changing,
                };
                if !keep {
                    continue;
                }
            }
            if let Some(needle) = &needle {
                let hit = fold(id).contains(needle.as_str())
                    || fold(&row.english).contains(needle.as_str())
                    || row
                        .text
                        .as_deref()
                        .is_some_and(|text| fold(text).contains(needle.as_str()));
                if !hit {
                    continue;
                }
            }
            if items.len() == PAGE_SIZE {
                more = true;
                break;
            }
            items.push(row);
        }
        let next_cursor = if more {
            items.last().map(|row| Cursor::encode(&row.message_id))
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }

    // ------------------------------------------------------------------ writing: I

    // spec: ordin-desktop/components/Localisation#LOC-LF-3
    pub async fn edit_message(
        &self,
        locale: &str,
        message_id: &str,
        ftl: &str,
    ) -> Result<MessageRow, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        if !english().facts.contains_key(message_id) {
            return Err(LocalisationError::MessageUnknown);
        }
        let refused = |problem| LocalisationError::MessageRefused {
            message_id: message_id.to_string(),
            problem,
        };
        let entry = self
            .vet(&tag, Some(message_id), message_id, &lf(ftl), "edited")
            .map_err(refused)?;
        {
            let state = self.read_state();
            if !Localisation::fits(state.people.get(&tag), message_id, entry.chars().count()) {
                tracing::info!(
                    component = COMPONENT,
                    locale = tag.as_str(),
                    message_id = message_id,
                    problem = EntryProblem::LanguageFull.kind(),
                    origin = "edited",
                    "localisation.entry_refused"
                );
                return Err(refused(EntryProblem::LanguageFull));
            }
        }
        let pool = self.pool()?;
        let now = now_wire();
        let has_row = self.read_state().people.contains_key(&tag);
        let mut tx = pool.begin().await?;
        if !has_row {
            let row = LangRow {
                locale: tag.clone(),
                kind: LanguageKind::Shipped,
                state: LanguageState::InUse,
                name: None,
                made_for_version: self.inner.app_version.clone(),
                checked_against: self.inner.app_version.clone(),
                draft_total: None,
                created_at: now.clone(),
                updated_at: now.clone(),
                confirmed_at: None,
            };
            store::insert_language(&mut tx, &row).await?;
        }
        let stored = MsgRow {
            ftl: entry,
            origin: Origin::Edited,
            reviewed: true,
            usable: true,
            updated_at: now,
        };
        store::upsert_message(&mut tx, &tag, message_id, &stored).await?;
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Edited);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            message_id = message_id,
            change = "edited",
            "localisation.message_edited"
        );
        Ok(self.row(&tag, message_id))
    }

    fn require_shipped(&self, tag: &str) -> Result<(), LocalisationError> {
        let state = self.read_state();
        match state.people.get(tag) {
            Some(person) if person.row.kind != LanguageKind::Shipped => {
                Err(LocalisationError::NotShipped)
            }
            _ => Ok(()),
        }
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-4
    pub async fn restore_message(
        &self,
        locale: &str,
        message_id: &str,
    ) -> Result<MessageRow, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        self.require_shipped(&tag)?;
        if !english().facts.contains_key(message_id) {
            return Err(LocalisationError::MessageUnknown);
        }
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::delete_message(&mut tx, &tag, message_id).await?;
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Restored);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            message_id = message_id,
            change = "restored",
            "localisation.message_edited"
        );
        Ok(self.row(&tag, message_id))
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-4
    pub async fn restore_language(
        &self,
        locale: &str,
    ) -> Result<LanguageSummary, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        self.require_shipped(&tag)?;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::delete_messages(&mut tx, &tag).await?;
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Restored);
        Ok(self.summary(&tag))
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-5
    pub async fn mark_reviewed(
        &self,
        locale: &str,
        message_ids: &[String],
    ) -> Result<LanguageSummary, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        let english = english();
        if message_ids
            .iter()
            .any(|id| !english.facts.contains_key(id.as_str()))
        {
            return Err(LocalisationError::MessageUnknown);
        }
        let pool = self.pool()?;
        let now = now_wire();
        let mut tx = pool.begin().await?;
        let mut count = 0u64;
        for id in message_ids.iter().filter(|id| is_safety(id)) {
            count += store::mark_reviewed(&mut tx, &tag, id, &now).await?;
        }
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Reviewed);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            count = count,
            "localisation.language_reviewed"
        );
        Ok(self.summary(&tag))
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-6
    pub async fn confirm_language(
        &self,
        locale: &str,
    ) -> Result<LanguageSummary, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        let waiting = {
            let state = self.read_state();
            match state.people.get(&tag) {
                Some(person) if person.row.kind != LanguageKind::Shipped => Some(
                    person
                        .messages
                        .iter()
                        .filter(|(id, row)| is_safety(id) && !row.reviewed && row.usable)
                        .count(),
                ),
                _ => None,
            }
        };
        let Some(waiting) = waiting else {
            // A shipped language is always in use.
            return Ok(self.summary(&tag));
        };
        if waiting > 0 {
            return Err(LocalisationError::ReviewIncomplete {
                count: u32::try_from(waiting).unwrap_or(u32::MAX),
            });
        }
        let pool = self.pool()?;
        let now = now_wire();
        let mut tx = pool.begin().await?;
        store::set_state(&mut tx, &tag, LanguageState::InUse, None, Some(&now), &now).await?;
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Confirmed);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            "localisation.language_confirmed"
        );
        Ok(self.summary(&tag))
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-7
    pub async fn remove_language(&self, locale: &str) -> Result<(), LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(locale, WRITE)?;
        if self.inner.active.borrow().locale == tag {
            return Err(LocalisationError::LanguageActive);
        }
        let kind = self
            .read_state()
            .people
            .get(&tag)
            .map_or(LanguageKind::Shipped, |person| person.row.kind);
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        store::delete_messages(&mut tx, &tag).await?;
        store::delete_language(&mut tx, &tag).await?;
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::Removed);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            kind = crate::store::kind_str(kind),
            "localisation.language_removed"
        );
        Ok(())
    }

    // ------------------------------------------------------------------ export: J

    /// The language as one file (≤ 2 MiB).
    // spec: ordin-desktop/components/Localisation#LOC-LF-9
    pub fn export_language(&self, locale: &str) -> Result<LanguageFile, LocalisationError> {
        let tag = self.guard(
            locale,
            Guard {
                editable: true,
                unknown: true,
                drafting: false,
            },
        )?;
        let state = self.read_state();
        let served = self.served_entries(&state, &tag);
        let (origin, entries): (&str, BTreeMap<String, String>) =
            match (self.inner.bundled.get(&tag), state.people.get(&tag)) {
                (Some(bundled), _) => {
                    let mut all = bundled.entries().clone();
                    all.extend(served);
                    ("shipped-with-edits", all)
                }
                (None, Some(person)) => (
                    if person.row.kind == LanguageKind::Made {
                        "drafted"
                    } else {
                        "imported"
                    },
                    served,
                ),
                (None, None) => return Err(LocalisationError::LanguageUnknown),
            };
        let mut ftl = format!(
            "### ordin language file\n### locale: {tag}\n### made-for: {}\n### origin: {origin}\n\n",
            self.inner.app_version
        );
        ftl.push_str(&entries.values().cloned().collect::<Vec<_>>().join("\n\n"));
        ftl.push('\n');
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            messages = entries.len(),
            "localisation.language_exported"
        );
        Ok(LanguageFile {
            file_name: format!("ordin-language-{tag}.ftl"),
            ftl,
        })
    }

    // ------------------------------------------------------------------ import: K

    /// Imports one language file.
    // spec: ordin-desktop/components/Localisation#LOC-LF-10
    pub async fn import_language(
        &self,
        file_name: &str,
        ftl: &str,
        replace: bool,
    ) -> Result<ImportReport, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let pool = self.pool()?;
        // The file name is shown by the screens; the language is named by the file's own header.
        let _ = file_name;
        if ftl.len() > MAX_LANGUAGE_CHARS {
            return Err(LocalisationError::FileTooLarge);
        }
        let text = lf(ftl);
        let mut marker = false;
        let mut tag_line: Option<&str> = None;
        let mut made_for: Option<&str> = None;
        for line in text.lines() {
            let line = line.trim_end();
            if line == "### ordin language file" {
                marker = true;
            } else if let Some(value) = line.strip_prefix("### locale:") {
                tag_line.get_or_insert(value.trim());
            } else if let Some(value) = line.strip_prefix("### made-for:") {
                made_for.get_or_insert(value.trim());
            }
        }
        let entries = messages_in(&text);
        let Some(tag_line) = tag_line.filter(|_| marker && !entries.is_empty()) else {
            return Err(LocalisationError::FileUnreadable);
        };
        let tag = canonical_tag(tag_line).ok_or(LocalisationError::TagInvalid)?;
        if tag == SOURCE_LOCALE || is_pseudo(&tag) {
            return Err(LocalisationError::LanguageNotEditable);
        }
        let bundled = self.inner.bundled.get(&tag);
        let existing_kind;
        {
            let state = self.read_state();
            let person = state.people.get(&tag);
            existing_kind = person.map(|person| person.row.kind);
            if let Some(person) = person.filter(|p| p.row.kind != LanguageKind::Shipped) {
                if is_drafting(person) {
                    return Err(LocalisationError::LanguageDrafting);
                }
                if !replace {
                    return Err(LocalisationError::LanguageExists);
                }
            }
        }
        if bundled.is_none()
            && existing_kind.is_none()
            && self.own_language_count() >= MAX_OWN_LANGUAGES
        {
            return Err(LocalisationError::TooManyLanguages);
        }

        // Per entry: unknown identifiers are dropped, failing entries left out.
        let english = english();
        let mut kept: BTreeMap<String, String> = BTreeMap::new();
        let mut dropped_unknown = 0u32;
        let mut left_out_ids: BTreeSet<String> = BTreeSet::new();
        for (id, entry) in entries {
            if !english.facts.contains_key(&id) {
                dropped_unknown += 1;
                continue;
            }
            match self.vet(&tag, None, &id, &entry, "imported") {
                Ok(entry) => {
                    kept.insert(id, entry);
                }
                Err(_) => {
                    left_out_ids.insert(id);
                }
            }
        }
        if kept.is_empty() {
            return Err(LocalisationError::FileUnreadable);
        }
        let left_out = u32::try_from(left_out_ids.len()).unwrap_or(u32::MAX);
        let now = now_wire();
        let imported = |ftl: String| MsgRow {
            ftl,
            origin: Origin::Imported,
            reviewed: false,
            usable: true,
            updated_at: now.clone(),
        };
        let mut tx = pool.begin().await?;
        let stored;
        if let Some(bundled) = bundled {
            // LOC-LF-11: only entries that differ from the shipped text are stored, as overrides.
            let shipped = bundled.entries();
            let changed: Vec<(&String, &String)> = kept
                .iter()
                .filter(|(id, entry)| shipped.get(*id) != Some(*entry))
                .collect();
            stored = u32::try_from(changed.len()).unwrap_or(u32::MAX);
            if !changed.is_empty() && existing_kind.is_none() {
                let row = LangRow {
                    locale: tag.clone(),
                    kind: LanguageKind::Shipped,
                    state: LanguageState::InUse,
                    name: None,
                    made_for_version: self.inner.app_version.clone(),
                    checked_against: self.inner.app_version.clone(),
                    draft_total: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    confirmed_at: None,
                };
                store::insert_language(&mut tx, &row).await?;
            }
            for (id, entry) in changed {
                store::upsert_message(&mut tx, &tag, id, &imported(entry.clone())).await?;
            }
        } else {
            // LOC-LF-12.
            stored = u32::try_from(kept.len()).unwrap_or(u32::MAX);
            let version = made_for
                .filter(|value| {
                    !value.is_empty()
                        && value.chars().count() <= MAX_VERSION_CHARS
                        && !value.chars().any(char::is_control)
                })
                .map_or_else(|| self.inner.app_version.clone(), str::to_string);
            let previous = self.read_state().people.get(&tag).map(|p| p.row.clone());
            match previous {
                Some(mut row) => {
                    store::delete_messages(&mut tx, &tag).await?;
                    if row.state != LanguageState::InUse {
                        row.state = LanguageState::NeedsReview;
                    }
                    row.made_for_version = version;
                    row.checked_against = self.inner.app_version.clone();
                    row.draft_total = None;
                    row.updated_at = now.clone();
                    store::replace_language_meta(&mut tx, &row).await?;
                }
                None => {
                    let row = LangRow {
                        locale: tag.clone(),
                        kind: LanguageKind::Imported,
                        state: LanguageState::NeedsReview,
                        name: None,
                        made_for_version: version,
                        checked_against: self.inner.app_version.clone(),
                        draft_total: None,
                        created_at: now.clone(),
                        updated_at: now.clone(),
                        confirmed_at: None,
                    };
                    store::insert_language(&mut tx, &row).await?;
                }
            }
            for (id, entry) in &kept {
                store::upsert_message(&mut tx, &tag, id, &imported(entry.clone())).await?;
            }
        }
        tx.commit().await?;
        self.sync_language(&tag).await?;
        let summary = self.summary(&tag);
        self.emit(&tag, LanguageChangeKind::Imported);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            stored = stored,
            dropped_unknown = dropped_unknown,
            in_english = summary.in_english,
            left_out = left_out,
            "localisation.language_imported"
        );
        Ok(ImportReport {
            locale: tag,
            stored,
            dropped_unknown,
            in_english: summary.in_english,
            left_out,
            left_out_ids: left_out_ids.into_iter().take(MAX_LEFT_OUT_IDS).collect(),
            summary,
        })
    }

    // ------------------------------------------------------------------ drafting: L

    /// Changing the language in words: how a requested language relates to what the person may choose.
    // spec: ordin-desktop/components/Localisation#LOC-LF-13
    pub fn match_language(&self, requested: &str) -> Result<LanguageMatch, LocalisationError> {
        let tag = canonical_tag(requested).ok_or(LocalisationError::TagInvalid)?;
        let state = self.read_state();
        let agent = self.inner.pool.is_none();
        if state.offered.contains(&tag) {
            return Ok(LanguageMatch::Choosable { locale: tag });
        }
        if let Some(found) = same_language(&tag, &state.offered) {
            return Ok(LanguageMatch::Choosable {
                locale: found.clone(),
            });
        }
        let own: Vec<String> = state
            .people
            .iter()
            .filter(|(_, person)| {
                person.row.kind != LanguageKind::Shipped && person.row.state != LanguageState::InUse
            })
            .map(|(tag, _)| tag.clone())
            .collect();
        let not_confirmed = if own.contains(&tag) {
            Some(tag.clone())
        } else {
            same_language(&tag, &own).cloned()
        };
        if let Some(locale) = not_confirmed {
            return Ok(if agent {
                LanguageMatch::NotOffered { locale }
            } else {
                LanguageMatch::NotConfirmed { locale }
            });
        }
        // A bundled locale that is not ready (LOC-CMP-1) is not a language ordin has.
        let bundled: Vec<String> = self
            .inner
            .bundled
            .keys()
            .filter(|candidate| self.ready(&state, candidate))
            .cloned()
            .collect();
        let not_offered = if bundled.contains(&tag) {
            Some(tag.clone())
        } else {
            same_language(&tag, &bundled).cloned()
        };
        if let Some(locale) = not_offered {
            return Ok(LanguageMatch::NotOffered { locale });
        }
        Ok(if agent {
            LanguageMatch::NotOffered { locale: tag }
        } else {
            LanguageMatch::Missing { locale: tag }
        })
    }

    /// The `en-GB` messages with no served or stored entry in that language, in identifier order.
    // The Ordin Host estimates a language that does not exist yet, so an unknown tag lists every
    // message, and a language being drafted is not refused (a drafting job reads its remaining
    // sources).
    // spec: ordin-desktop/components/Localisation#LOC-LF-14
    pub fn draft_sources(&self, locale: &str) -> Result<Vec<DraftSource>, LocalisationError> {
        let tag = self.guard(
            locale,
            Guard {
                editable: true,
                unknown: false,
                drafting: false,
            },
        )?;
        let state = self.read_state();
        let person = state.people.get(&tag);
        let bundled = self.inner.bundled.get(&tag);
        let english = english();
        Ok(english
            .entries
            .iter()
            .filter(|(id, _)| {
                !person.is_some_and(|person| person.messages.contains_key(id.as_str()))
                    && !bundled.is_some_and(|b| b.bundle.has_message(id))
            })
            .map(|(id, entry)| DraftSource {
                message_id: id.clone(),
                english: entry.clone(),
            })
            .collect())
    }

    /// Starts drafting `locale` (LOC-LF-15).
    // `begin_draft` has no `replace` parameter: an existing made or imported language goes back to
    // `drafting`.
    // spec: ordin-desktop/components/Localisation#LOC-LF-15
    pub async fn begin_draft(
        &self,
        locale: &str,
        name: &str,
        total: u32,
    ) -> Result<(), LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(
            locale,
            Guard {
                editable: true,
                unknown: false,
                drafting: true,
            },
        )?;
        let pool = self.pool()?;
        let now = now_wire();
        let previous = self.read_state().people.get(&tag).map(|p| p.row.clone());
        let mut tx = pool.begin().await?;
        match previous {
            Some(row) if row.kind == LanguageKind::Shipped => {
                store::set_draft_total(&mut tx, &tag, Some(total), &now).await?;
            }
            Some(_) => {
                store::set_state(
                    &mut tx,
                    &tag,
                    LanguageState::Drafting,
                    Some(total),
                    None,
                    &now,
                )
                .await?;
            }
            None => {
                let bundled = self.inner.bundled.contains_key(&tag);
                if !bundled && self.own_language_count() >= MAX_OWN_LANGUAGES {
                    return Err(LocalisationError::TooManyLanguages);
                }
                let name = Some(name.to_string())
                    .filter(|n| (1..=MAX_NAME_CHARS).contains(&n.chars().count()));
                let row = LangRow {
                    locale: tag.clone(),
                    kind: if bundled {
                        LanguageKind::Shipped
                    } else {
                        LanguageKind::Made
                    },
                    state: if bundled {
                        LanguageState::InUse
                    } else {
                        LanguageState::Drafting
                    },
                    name: if bundled { None } else { name },
                    made_for_version: self.inner.app_version.clone(),
                    checked_against: self.inner.app_version.clone(),
                    draft_total: Some(total),
                    created_at: now.clone(),
                    updated_at: now.clone(),
                    confirmed_at: None,
                };
                store::insert_language(&mut tx, &row).await?;
            }
        }
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.set_draft_done(&tag, 0);
        self.emit(&tag, LanguageChangeKind::DraftStarted);
        Ok(())
    }

    /// Stores a drafted reply, each entry through `check_entry` (LOC-LF-16).
    // spec: ordin-desktop/components/Localisation#LOC-LF-16
    pub async fn add_drafted(
        &self,
        locale: &str,
        reply: &str,
    ) -> Result<DraftBatchOutcome, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(
            locale,
            Guard {
                editable: true,
                unknown: false,
                drafting: false,
            },
        )?;
        let (mut chars, mut count, edited): (usize, usize, BTreeSet<String>) = {
            let state = self.read_state();
            let Some(person) = state.people.get(&tag).filter(|p| is_drafting(p)) else {
                return Err(LocalisationError::LanguageUnknown);
            };
            (
                person
                    .messages
                    .values()
                    .map(|row| row.ftl.chars().count())
                    .sum(),
                person.messages.len(),
                person
                    .messages
                    .iter()
                    .filter(|(_, row)| row.origin == Origin::Edited)
                    .map(|(id, _)| id.clone())
                    .collect(),
            )
        };
        let pool = self.pool()?;
        let now = now_wire();
        let mut stored = 0u32;
        let mut left_out = 0u32;
        let mut tx = pool.begin().await?;
        for (id, entry) in messages_in(&lf(reply)) {
            if edited.contains(&id) {
                continue;
            }
            let Ok(entry) = self.vet(&tag, None, &id, &entry, "drafted") else {
                left_out += 1;
                continue;
            };
            let existing = self
                .read_state()
                .people
                .get(&tag)
                .and_then(|p| p.messages.get(&id).map(|row| row.ftl.chars().count()));
            let length = entry.chars().count();
            let after_chars = chars - existing.unwrap_or(0) + length;
            let after_count = count + usize::from(existing.is_none());
            if after_chars > MAX_LANGUAGE_CHARS || after_count > MAX_LANGUAGE_MESSAGES {
                left_out += 1;
                continue;
            }
            chars = after_chars;
            count = after_count;
            let row = MsgRow {
                ftl: entry,
                origin: Origin::Drafted,
                reviewed: false,
                usable: true,
                updated_at: now.clone(),
            };
            store::upsert_message(&mut tx, &tag, &id, &row).await?;
            stored += 1;
        }
        tx.commit().await?;
        self.sync_language(&tag).await?;
        {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(person) = state.people.get_mut(&tag) {
                person.draft_done = person.draft_done.saturating_add(stored);
            }
        }
        self.emit(&tag, LanguageChangeKind::DraftProgress);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            stored = stored,
            left_out = left_out,
            "localisation.draft_batch"
        );
        Ok(DraftBatchOutcome { stored, left_out })
    }

    /// Ends a draft (LOC-LF-17).
    // spec: ordin-desktop/components/Localisation#LOC-LF-17
    pub async fn end_draft(
        &self,
        locale: &str,
        end: DraftEnd,
    ) -> Result<LanguageSummary, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let tag = self.guard(
            locale,
            Guard {
                editable: true,
                unknown: false,
                drafting: false,
            },
        )?;
        let (row, done, unreviewed) = {
            let state = self.read_state();
            let Some(person) = state.people.get(&tag).filter(|p| is_drafting(p)) else {
                return Err(LocalisationError::LanguageUnknown);
            };
            (
                person.row.clone(),
                person.draft_done,
                person
                    .messages
                    .iter()
                    .any(|(id, message)| is_safety(id) && !message.reviewed),
            )
        };
        let pool = self.pool()?;
        let now = now_wire();
        let mut tx = pool.begin().await?;
        if row.kind == LanguageKind::Shipped {
            store::set_draft_total(&mut tx, &tag, None, &now).await?;
        } else {
            let next = if unreviewed || row.confirmed_at.is_none() {
                LanguageState::NeedsReview
            } else {
                LanguageState::InUse
            };
            store::set_state(&mut tx, &tag, next, None, None, &now).await?;
        }
        tx.commit().await?;
        self.sync_language(&tag).await?;
        self.emit(&tag, LanguageChangeKind::DraftEnded);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            end = match end {
                DraftEnd::Finished => "finished",
                DraftEnd::Stopped => "stopped",
                DraftEnd::Paused => "paused",
            },
            done = done,
            total = row.draft_total.unwrap_or(0),
            "localisation.draft_ended"
        );
        Ok(self.summary(&tag))
    }
}
