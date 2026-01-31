//! The Localisation service: start, negotiation, the active locale, message formatting and the
//! catalogue (Localisation.md Internal Logic B–G). The person-layer methods are in `person.rs`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

use fluent_bundle::types::FluentNumber;
use fluent_bundle::{FluentArgs, FluentError, FluentResource, FluentValue};
use icu::locale::Locale;
use ordin_journal::Datastore;
use ordin_organisation_profile::{OrganisationProfileService, ProfileSnapshot, SnapshotSource};
use ordin_types::domain::LocaleSource;
use ordin_types::num::DecimalString;
use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use sqlx::SqlitePool;
use tokio::sync::{broadcast, watch};

use crate::catalogue::{
    bundled_locales, bundled_text, canonical_tag, complete_locales, english, entries_by_id,
    is_pseudo, SOURCE_LOCALE,
};
use crate::check::check_entry;
use crate::error::LocalisationError;
use crate::fmt::{direction_of, new_bundle, Bundle, FormatterCache};
use crate::store::{self, LangRow, MsgRow};
use crate::types::{
    ActiveLocale, ArgValue, CatalogueResource, CatalogueText, DateTimeStyle, LanguageChange,
    LanguageKind, LanguageState, MessageArgs, MessageId, OfferedLocale,
};
use crate::{COMPONENT, SAFETY_IDS};

/// Where the preference row lives.
pub enum PreferenceStore {
    /// Desktop, personal server, agent single shape: the Journal's `Datastore` (D4, R20);
    /// this crate uses `datastore.sqlite()` and treats a PostgreSQL store as `None`.
    Datastore(Datastore),
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

/// A bundled locale: its compiled-in text and the bundle built from it.
pub(crate) struct Bundled {
    pub(crate) text: &'static str,
    pub(crate) resource: Arc<FluentResource>,
    pub(crate) bundle: Arc<Bundle>,
    entries: OnceLock<BTreeMap<String, String>>,
}

impl Bundled {
    /// message id → normalised entry of the shipped catalogue (built on first use).
    pub(crate) fn entries(&self) -> &BTreeMap<String, String> {
        self.entries.get_or_init(|| entries_by_id(self.text))
    }
}

/// One language of the person layer, in memory.
pub(crate) struct Person {
    pub(crate) row: LangRow,
    pub(crate) messages: BTreeMap<String, MsgRow>,
    /// `None` when the language serves nothing (a shipped language then uses the bundled bundle).
    pub(crate) bundle: Option<Arc<Bundle>>,
    pub(crate) draft_done: u32,
}

/// The stored preference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pref {
    pub(crate) locale: String,
    pub(crate) source: LocaleSource,
}

pub(crate) struct State {
    pub(crate) profile: Arc<ProfileSnapshot>,
    pub(crate) people: BTreeMap<String, Person>,
    /// The company's list (offered ∩ bundled), without person languages.
    pub(crate) company: Vec<String>,
    /// Everything `set_locale` accepts.
    pub(crate) offered: Vec<String>,
    pub(crate) fallback: String,
    pub(crate) pref: Option<Pref>,
}

pub(crate) struct Inner {
    pub(crate) app_version: String,
    pub(crate) pool: Option<SqlitePool>,
    pub(crate) os_locales: Vec<String>,
    pub(crate) bundled: BTreeMap<String, Bundled>,
    /// The bundled locales whose catalogue holds every `en-GB` identifier (LOC-CMP-1).
    pub(crate) complete: BTreeSet<String>,
    pub(crate) formatters: FormatterCache,
    pub(crate) state: RwLock<State>,
    pub(crate) active: watch::Sender<ActiveLocale>,
    pub(crate) changes: broadcast::Sender<LanguageChange>,
    /// Serialises every write (set_locale, profile following and the person layer).
    pub(crate) gate: tokio::sync::Mutex<()>,
    logged_versions: Mutex<HashSet<String>>,
}

pub struct Localisation {
    pub(crate) inner: Arc<Inner>,
}

/// What a recomputation of the locale decides.
struct Plan {
    company: Vec<String>,
    offered: Vec<String>,
    fallback: String,
    locale: String,
    source: LocaleSource,
    /// The row to write when the person's choice is no longer offered.
    rewrite: Option<Pref>,
}

pub(crate) fn now_wire() -> String {
    Timestamp::now().to_wire()
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A safety message (approval cards and who-acts statements): `SAFETY_IDS` is sorted.
pub(crate) fn is_safety(id: &str) -> bool {
    SAFETY_IDS.binary_search(&id).is_ok()
}

/// Served means usable, and either not safety wording or reviewed.
pub(crate) fn is_served(id: &str, row: &MsgRow) -> bool {
    row.usable && (row.reviewed || !is_safety(id))
}

fn version_of(snapshot: &ProfileSnapshot) -> String {
    match &snapshot.source {
        SnapshotSource::Verified {
            profile_version, ..
        } => profile_version.clone(),
        SnapshotSource::Defaults => "defaults".to_string(),
    }
}

fn source_str(source: LocaleSource) -> &'static str {
    match source {
        LocaleSource::OperatingSystem => "operating_system",
        LocaleSource::Person => "person",
    }
}

fn parse_source(raw: &str) -> Option<LocaleSource> {
    match raw {
        "operating_system" => Some(LocaleSource::OperatingSystem),
        "person" => Some(LocaleSource::Person),
        _ => None,
    }
}

/// A tag from the operating system, tolerating the underscore form some platforms report.
fn os_tag(raw: &str) -> Option<String> {
    canonical_tag(&raw.replace('_', "-"))
}

fn language_of(tag: &str) -> Option<(String, Option<String>)> {
    Locale::try_from_str(tag).ok().map(|locale| {
        (
            locale.id.language.to_string(),
            locale.id.script.map(|script| script.to_string()),
        )
    })
}

/// The first of `candidates` with the same language subtag as `wanted` (and the same script subtag
/// when `wanted` has one).
pub(crate) fn same_language<'a>(wanted: &str, candidates: &'a [String]) -> Option<&'a String> {
    let (language, script) = language_of(wanted)?;
    candidates.iter().find(|candidate| {
        language_of(candidate).is_some_and(|(candidate_language, candidate_script)| {
            candidate_language == language && (script.is_none() || candidate_script == script)
        })
    })
}

/// Negotiation, B6: the person's choice, else the operating system's, else the fallback.
fn negotiate(
    offered: &[String],
    os_locales: &[String],
    pref: Option<&Pref>,
    fallback: &str,
) -> (String, LocaleSource, bool) {
    let mut withdrawn = false;
    if let Some(pref) = pref {
        if pref.source == LocaleSource::Person {
            if offered.contains(&pref.locale) {
                return (pref.locale.clone(), LocaleSource::Person, false);
            }
            withdrawn = true;
        }
    }
    for raw in os_locales {
        let Some(tag) = os_tag(raw) else { continue };
        if let Some(found) = offered.iter().find(|candidate| **candidate == tag) {
            return (found.clone(), LocaleSource::OperatingSystem, withdrawn);
        }
        if let Some(found) = same_language(&tag, offered) {
            return (found.clone(), LocaleSource::OperatingSystem, withdrawn);
        }
    }
    (
        fallback.to_string(),
        LocaleSource::OperatingSystem,
        withdrawn,
    )
}

fn chain_of(locale: &str, fallback: &str) -> Vec<String> {
    let mut chain: Vec<String> = Vec::with_capacity(3);
    for tag in [locale, fallback, SOURCE_LOCALE] {
        if !chain.iter().any(|seen| seen == tag) {
            chain.push(tag.to_string());
        }
    }
    chain
}

fn active_of(locale: String, source: LocaleSource, fallback: &str) -> ActiveLocale {
    ActiveLocale {
        direction: direction_of(&locale),
        fallback_chain: chain_of(&locale, fallback),
        locale,
        source,
    }
}

fn error_kind(error: &FluentError) -> &'static str {
    match error {
        FluentError::Overriding { .. } => "overriding",
        FluentError::ParserError(_) => "parser",
        FluentError::ResolverError(_) => "resolver",
    }
}

fn to_fluent(args: &MessageArgs) -> FluentArgs<'static> {
    let mut out = FluentArgs::new();
    for (name, value) in &args.0 {
        match value {
            ArgValue::Text(text) => out.set(*name, FluentValue::String(Cow::Owned(text.clone()))),
            ArgValue::Number(decimal) => {
                out.set(*name, FluentValue::String(Cow::Owned(decimal.0.clone())));
            }
            ArgValue::Count(count) => out.set(
                *name,
                FluentValue::Number(FluentNumber::new(*count as f64, Default::default())),
            ),
            ArgValue::DateTime(at) => {
                out.set(*name, FluentValue::String(Cow::Owned(at.to_wire())));
            }
        }
    }
    out
}

/// `error-<code in kebab-case>`.
pub(crate) fn error_message_id(code: &str) -> String {
    format!("error-{}", code.to_lowercase().replace('_', "-"))
}

impl Localisation {
    /// Loads the bundled catalogues, reads (or creates) the preference row, negotiates the
    /// active locale and starts a task that follows profile changes.
    pub async fn start(
        config: LocalisationConfig,
        profile: Arc<OrganisationProfileService>,
    ) -> Result<Arc<Localisation>, LocalisationError> {
        let snapshot = profile.current();
        let changes = profile.subscribe();
        Localisation::start_with_source(config, snapshot, changes).await
    }

    /// `start`, over any source of profile snapshots (the follow task reads `changes`).
    pub(crate) async fn start_with_source(
        config: LocalisationConfig,
        snapshot: Arc<ProfileSnapshot>,
        changes: watch::Receiver<Arc<ProfileSnapshot>>,
    ) -> Result<Arc<Localisation>, LocalisationError> {
        let complete = complete_locales().iter().map(|tag| (*tag).to_string());
        Localisation::start_with_catalogues(config, snapshot, changes, complete.collect()).await
    }

    /// `start_with_source`, naming which bundled catalogues count as complete (LOC-CMP-1); the
    /// crate's tests declare the short bundled `fr-FR` and `de-DE` complete through it.
    pub(crate) async fn start_with_catalogues(
        config: LocalisationConfig,
        snapshot: Arc<ProfileSnapshot>,
        mut changes: watch::Receiver<Arc<ProfileSnapshot>>,
        complete: BTreeSet<String>,
    ) -> Result<Arc<Localisation>, LocalisationError> {
        let started = std::time::Instant::now();
        let pool = match &config.store {
            PreferenceStore::Datastore(store) => store.sqlite().cloned(),
            PreferenceStore::None => None,
        };

        // B1: one bundle per bundled locale.
        let mut bundled = BTreeMap::new();
        let formatters = FormatterCache::default();
        for tag in bundled_locales() {
            let Some(text) = bundled_text(tag) else {
                continue;
            };
            let resource = match FluentResource::try_new(text.to_string()) {
                Ok(resource) => Arc::new(resource),
                Err(_) => {
                    tracing::error!(
                        component = COMPONENT,
                        locale = *tag,
                        "localisation.bundle_failed"
                    );
                    continue;
                }
            };
            let mut bundle = new_bundle(tag, formatters.get(tag));
            if bundle.add_resource(Arc::clone(&resource)).is_err() {
                tracing::error!(
                    component = COMPONENT,
                    locale = *tag,
                    "localisation.bundle_failed"
                );
                continue;
            }
            bundled.insert(
                (*tag).to_string(),
                Bundled {
                    text,
                    resource,
                    bundle: Arc::new(bundle),
                    entries: OnceLock::new(),
                },
            );
        }

        // B3a: the person layer.
        let mut people = BTreeMap::new();
        if let Some(pool) = &pool {
            people =
                Localisation::load_people(pool, &config.app_version, &bundled, &formatters).await?;
        }

        let fallback_seed = SOURCE_LOCALE.to_string();
        let state = State {
            profile: Arc::clone(&snapshot),
            people,
            company: Vec::new(),
            offered: Vec::new(),
            fallback: fallback_seed,
            pref: None,
        };
        let initial = active_of(
            SOURCE_LOCALE.to_string(),
            LocaleSource::OperatingSystem,
            SOURCE_LOCALE,
        );
        let (active_tx, _) = watch::channel(initial);
        let (changes_tx, _) = broadcast::channel(64);
        let inner = Arc::new(Inner {
            app_version: config.app_version,
            pool,
            os_locales: config.os_locales,
            bundled,
            complete,
            formatters,
            state: RwLock::new(state),
            active: active_tx,
            changes: changes_tx,
            gate: tokio::sync::Mutex::new(()),
            logged_versions: Mutex::new(HashSet::new()),
        });
        let service = Arc::new(Localisation {
            inner: Arc::clone(&inner),
        });

        // B2–B9.
        service.settle_start(snapshot).await?;

        // B10: follow the profile (C). The task holds only a weak reference.
        let weak = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while changes.changed().await.is_ok() {
                let snapshot = Arc::clone(&changes.borrow_and_update());
                let Some(inner) = weak.upgrade() else { break };
                Localisation { inner }.follow(snapshot).await;
            }
        });
        tracing::debug!(
            component = COMPONENT,
            elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            "localisation.started"
        );
        Ok(service)
    }

    /// Steps B2–B9 at start.
    async fn settle_start(&self, snapshot: Arc<ProfileSnapshot>) -> Result<(), LocalisationError> {
        let inner = &self.inner;
        // B5: the stored preference.
        let mut stored: Option<Pref> = None;
        let mut write_needed = false;
        if let Some(pool) = &inner.pool {
            match store::load_pref(pool).await? {
                None => write_needed = true,
                Some(raw) => {
                    let source = parse_source(&raw.source);
                    let valid_time = parse_rfc3339_ms_utc(&raw.updated_at).is_ok();
                    let length = raw.locale.chars().count();
                    match source {
                        Some(source) if valid_time && (2..=35).contains(&length) => {
                            stored = Some(Pref {
                                locale: raw.locale,
                                source,
                            });
                        }
                        _ => {
                            // LOC_STORE_CORRUPT: logged only; the row is replaced.
                            tracing::warn!(
                                component = COMPONENT,
                                reason = "store_corrupt",
                                "localisation.preference_reset"
                            );
                            write_needed = true;
                        }
                    }
                }
            }
        }
        let plan = {
            let mut state = inner.state.write().unwrap_or_else(PoisonError::into_inner);
            state.profile = Arc::clone(&snapshot);
            self.plan(&state, stored.as_ref(), true)
        };
        let version = version_of(&snapshot);
        let mut pref = stored;
        if let Some(pool) = &inner.pool {
            if let Some(rewrite) = &plan.rewrite {
                tracing::info!(
                    component = COMPONENT,
                    locale = pref.as_ref().map_or("", |p| p.locale.as_str()),
                    profile_version = version.as_str(),
                    "localisation.preference_not_offered"
                );
                store::save_pref(pool, &plan.locale, source_str(rewrite.source), &now_wire())
                    .await?;
                pref = Some(rewrite.clone());
            } else if write_needed {
                store::save_pref(pool, &plan.locale, "operating_system", &now_wire()).await?;
                pref = Some(Pref {
                    locale: plan.locale.clone(),
                    source: LocaleSource::OperatingSystem,
                });
            }
        }
        self.install(plan, pref, true);
        Ok(())
    }

    /// The follow task (C): a new profile snapshot.
    async fn follow(&self, snapshot: Arc<ProfileSnapshot>) {
        let _gate = self.inner.gate.lock().await;
        let pref = {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            state.profile = Arc::clone(&snapshot);
            state.pref.clone()
        };
        let plan = {
            let state = self
                .inner
                .state
                .read()
                .unwrap_or_else(PoisonError::into_inner);
            self.plan(&state, pref.as_ref(), true)
        };
        let mut pref = pref;
        if let Some(rewrite) = &plan.rewrite {
            tracing::info!(
                component = COMPONENT,
                locale = pref.as_ref().map_or("", |p| p.locale.as_str()),
                profile_version = version_of(&snapshot).as_str(),
                "localisation.preference_not_offered"
            );
            if let Some(pool) = &self.inner.pool {
                // A failed write is not fatal here: the choice in force is still published, and the
                // next start negotiates again.
                let _ =
                    store::save_pref(pool, &plan.locale, source_str(rewrite.source), &now_wire())
                        .await;
            }
            pref = Some(rewrite.clone());
        }
        self.install(plan, pref, false);
    }

    /// Steps B3, B4, B6–B8 over the current state.
    fn plan(&self, state: &State, pref: Option<&Pref>, log: bool) -> Plan {
        let inner = &self.inner;
        let locales = &state.profile.document.locales;
        let version = version_of(&state.profile);
        let first_time = log && lock(&inner.logged_versions).insert(version.clone());

        // B3: offered, over the bundled locales that are ready (LOC-CMP-1).
        let bundled_plain: Vec<String> = inner
            .bundled
            .keys()
            .filter(|tag| !is_pseudo(tag) && self.ready(state, tag))
            .cloned()
            .collect();
        let mut ordered_bundled = bundled_plain.clone();
        ordered_bundled.sort_by_key(|tag| {
            bundled_locales()
                .iter()
                .position(|candidate| candidate == tag)
                .unwrap_or(usize::MAX)
        });
        let mut company: Vec<String> = Vec::new();
        if locales.offered.is_empty() {
            company = ordered_bundled;
        } else {
            for raw in &locales.offered {
                let canonical = canonical_tag(raw).filter(|tag| bundled_plain.contains(tag));
                match canonical {
                    Some(tag) => {
                        if !company.contains(&tag) {
                            company.push(tag);
                        }
                    }
                    None => {
                        if first_time {
                            tracing::warn!(
                                component = COMPONENT,
                                locale = raw.as_str(),
                                profile_version = version.as_str(),
                                "localisation.offered_not_bundled"
                            );
                        }
                    }
                }
            }
            if company.is_empty() {
                if first_time {
                    tracing::warn!(
                        component = COMPONENT,
                        profile_version = version.as_str(),
                        "localisation.offered_empty"
                    );
                }
                company = vec![SOURCE_LOCALE.to_string()];
            }
        }
        let offered = self.build_offered(&company, &state.people);

        // B4: fallback, a ready bundled locale (LOC-CMP-1).
        let wanted = canonical_tag(&locales.fallback).filter(|tag| self.ready(state, tag));
        let fallback = match wanted {
            Some(tag) => tag,
            None => {
                if first_time {
                    tracing::warn!(
                        component = COMPONENT,
                        locale = locales.fallback.as_str(),
                        profile_version = version.as_str(),
                        "localisation.fallback_not_bundled"
                    );
                }
                SOURCE_LOCALE.to_string()
            }
        };

        // B6.
        let (locale, source, withdrawn) = if inner.pool.is_some() {
            negotiate(&offered, &inner.os_locales, pref, &fallback)
        } else {
            (fallback.clone(), LocaleSource::OperatingSystem, false)
        };
        let rewrite = withdrawn.then(|| Pref {
            locale: locale.clone(),
            source: LocaleSource::OperatingSystem,
        });
        Plan {
            company,
            offered,
            fallback,
            locale,
            source,
            rewrite,
        }
    }

    /// A bundled locale that may be offered: its catalogue is complete, or the person layer holds
    /// it (the person edited, imported or drafted into it, LOC-LF-3, -11, -15).
    // spec: ordin-desktop/components/Localisation#LOC-CMP-1
    pub(crate) fn ready(&self, state: &State, tag: &str) -> bool {
        self.inner.bundled.contains_key(tag)
            && (self.inner.complete.contains(tag)
                || state
                    .people
                    .get(tag)
                    .is_some_and(|person| person.row.kind == LanguageKind::Shipped))
    }

    /// The company's list and the full offered list again, after the person layer changed (a
    /// person-layer row can make a bundled locale ready, LOC-CMP-1). Logs nothing.
    pub(crate) fn recompute_offered(&self, state: &mut State) {
        let plan = self.plan(state, state.pref.as_ref(), false);
        state.company = plan.company;
        state.offered = plan.offered;
    }

    /// The company's list, then the person's own languages in use (alphabetical), then the
    /// pseudo-locales (B3, amended January 2026, ADR-109).
    pub(crate) fn build_offered(
        &self,
        company: &[String],
        people: &BTreeMap<String, Person>,
    ) -> Vec<String> {
        let mut offered = company.to_vec();
        // `people` is ordered by tag.
        for (tag, person) in people {
            let own = person.row.kind != LanguageKind::Shipped
                && person.row.state == LanguageState::InUse;
            if own && !offered.contains(tag) {
                offered.push(tag.clone());
            }
        }
        for tag in self.inner.bundled.keys().filter(|tag| is_pseudo(tag)) {
            if !offered.contains(tag) {
                offered.push(tag.clone());
            }
        }
        offered
    }

    /// Stores the plan in the state and publishes the active locale when it changed (B9, C2).
    fn install(&self, plan: Plan, pref: Option<Pref>, initial: bool) {
        let active = active_of(plan.locale.clone(), plan.source, &plan.fallback);
        {
            let mut state = self
                .inner
                .state
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            state.company = plan.company;
            state.offered = plan.offered;
            state.fallback = plan.fallback;
            state.pref = pref;
        }
        self.publish(active, initial);
    }

    pub(crate) fn publish(&self, active: ActiveLocale, log_always: bool) -> bool {
        let changed = self.inner.active.send_if_modified(|current| {
            if *current == active {
                false
            } else {
                *current = active.clone();
                true
            }
        });
        if changed || log_always {
            tracing::info!(
                component = COMPONENT,
                locale = active.locale.as_str(),
                source = source_str(active.source),
                direction = match active.direction {
                    crate::types::TextDirection::Ltr => "ltr",
                    crate::types::TextDirection::Rtl => "rtl",
                },
                "localisation.active"
            );
        }
        changed
    }

    /// The locale in force.
    pub fn active(&self) -> ActiveLocale {
        self.inner.active.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<ActiveLocale> {
        self.inner.active.subscribe()
    }

    /// The locales the person may choose: offered ∩ bundled, in the profile's order.
    pub fn offered(&self) -> Vec<OfferedLocale> {
        let state = self.read_state();
        state
            .offered
            .iter()
            .map(|tag| OfferedLocale {
                locale: tag.clone(),
                native_name: self.native_name(&state, tag).unwrap_or_else(|| tag.clone()),
                direction: direction_of(tag),
            })
            .collect()
    }

    pub(crate) fn read_state(&self) -> std::sync::RwLockReadGuard<'_, State> {
        self.inner
            .state
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// The language's own served `locale-native-name`.
    pub(crate) fn native_name(&self, state: &State, tag: &str) -> Option<String> {
        let bundle = self.bundle_for(state, tag)?;
        let message = bundle.get_message("locale-native-name")?;
        let pattern = message.value()?;
        let mut errors = Vec::new();
        let text = bundle
            .format_pattern(pattern, None, &mut errors)
            .into_owned();
        Some(text)
    }

    /// The bundle that answers for `tag`: its person bundle, else the compiled-in one.
    pub(crate) fn bundle_for(&self, state: &State, tag: &str) -> Option<Arc<Bundle>> {
        if let Some(person) = state.people.get(tag) {
            if let Some(bundle) = &person.bundle {
                return Some(Arc::clone(bundle));
            }
            if person.row.kind != LanguageKind::Shipped {
                return None;
            }
        }
        self.inner.bundled.get(tag).map(|b| Arc::clone(&b.bundle))
    }

    /// `Some(tag)` stores the person's choice; `None` returns to following the operating system.
    pub async fn set_locale(
        &self,
        locale: Option<&str>,
    ) -> Result<ActiveLocale, LocalisationError> {
        let _gate = self.inner.gate.lock().await;
        let (offered, fallback, people_state) = {
            let state = self.read_state();
            (
                state.offered.clone(),
                state.fallback.clone(),
                state.pref.clone(),
            )
        };
        let Some(raw) = locale else {
            // D1.
            let Some(pool) = &self.inner.pool else {
                return Ok(self.active());
            };
            let (tag, _, _) = negotiate(&offered, &self.inner.os_locales, None, &fallback);
            store::save_pref(pool, &tag, "operating_system", &now_wire()).await?;
            let active = active_of(tag.clone(), LocaleSource::OperatingSystem, &fallback);
            self.set_pref(Pref {
                locale: tag,
                source: LocaleSource::OperatingSystem,
            });
            self.publish(active, false);
            return Ok(self.active());
        };
        // D2.
        let tag = canonical_tag(raw).ok_or(LocalisationError::TagInvalid)?;
        // D3: the agent profile holds no row.
        let Some(pool) = &self.inner.pool else {
            if tag == fallback {
                return Ok(self.active());
            }
            return Err(LocalisationError::NotOffered);
        };
        {
            let state = self.read_state();
            if let Some(person) = state.people.get(&tag) {
                if person.row.kind != LanguageKind::Shipped
                    && person.row.state != LanguageState::InUse
                {
                    return Err(LocalisationError::LanguageNotConfirmed);
                }
            }
        }
        if !offered.contains(&tag) {
            return Err(LocalisationError::NotOffered);
        }
        // D4: a store error leaves the previous locale in force.
        store::save_pref(pool, &tag, "person", &now_wire()).await?;
        let _ = people_state;
        let active = active_of(tag.clone(), LocaleSource::Person, &fallback);
        self.set_pref(Pref {
            locale: tag.clone(),
            source: LocaleSource::Person,
        });
        self.publish(active, false);
        tracing::info!(
            component = COMPONENT,
            locale = tag.as_str(),
            source = "person",
            "localisation.locale_set"
        );
        Ok(self.active())
    }

    fn set_pref(&self, pref: Pref) {
        let mut state = self
            .inner
            .state
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        state.pref = Some(pref);
    }

    /// Formats one message in the active locale, falling back along `fallback_chain`.
    /// Never fails: every MessageId exists in en-GB by construction.
    pub fn message(&self, id: MessageId, args: &MessageArgs) -> String {
        let chain = self.inner.active.borrow().fallback_chain.clone();
        self.format_chain(&chain, id.as_str(), args)
    }

    /// As `message`, for an explicit locale (used for a server's chat in the person's locale).
    pub fn message_in(&self, locale: &str, id: MessageId, args: &MessageArgs) -> String {
        let chain = {
            let state = self.read_state();
            let mut chain: Vec<String> = Vec::with_capacity(3);
            let wanted = canonical_tag(locale).unwrap_or_else(|| locale.to_string());
            for tag in [wanted.as_str(), state.fallback.as_str(), SOURCE_LOCALE] {
                let known = self.inner.bundled.contains_key(tag) || state.people.contains_key(tag);
                if known && !chain.iter().any(|seen| seen == tag) {
                    chain.push(tag.to_string());
                }
            }
            chain
        };
        self.format_chain(&chain, id.as_str(), args)
    }

    /// The plain-language sentence for a registered error code: message id
    /// `error-<code in kebab-case>`; an unregistered code yields the sentence of `error-srv-internal`.
    pub fn error_sentence(&self, code: &str, args: &MessageArgs) -> String {
        let chain = self.inner.active.borrow().fallback_chain.clone();
        let id = error_message_id(code);
        let known = ordin_types::error_registry::lookup(code).is_some()
            && english().entries.contains_key(&id);
        if known {
            self.format_chain(&chain, &id, args)
        } else {
            self.format_chain(&chain, "error-srv-internal", args)
        }
    }

    /// E1–E3.
    pub(crate) fn format_chain(&self, chain: &[String], id: &str, args: &MessageArgs) -> String {
        let fluent_args = to_fluent(args);
        let state = self.read_state();
        for (position, tag) in chain.iter().enumerate() {
            let Some(bundle) = self.bundle_for(&state, tag) else {
                continue;
            };
            let Some(message) = bundle.get_message(id) else {
                continue;
            };
            let Some(pattern) = message.value() else {
                continue;
            };
            let mut errors = Vec::new();
            let text = bundle
                .format_pattern(pattern, Some(&fluent_args), &mut errors)
                .into_owned();
            for error in &errors {
                tracing::warn!(
                    component = COMPONENT,
                    message_id = id,
                    locale = tag.as_str(),
                    error_kind = error_kind(error),
                    "localisation.format_error"
                );
            }
            if position > 0 {
                if let Some(first) = chain.first() {
                    tracing::debug!(
                        component = COMPONENT,
                        message_id = id,
                        locale = first.as_str(),
                        served_by = tag.as_str(),
                        "localisation.message_fallback"
                    );
                }
            }
            return text;
        }
        id.to_string()
    }

    pub fn format_number(&self, value: &DecimalString) -> String {
        self.active_formatters().number_text(&value.0)
    }

    pub fn format_datetime(&self, at: Timestamp, style: DateTimeStyle) -> String {
        self.active_formatters().datetime(at, style)
    }

    /// "A, B and C" by CLDR list rules ("and" type).
    pub fn format_list(&self, items: &[String]) -> String {
        self.active_formatters().list(items)
    }

    /// Collation order of the active locale.
    pub fn compare(&self, a: &str, b: &str) -> std::cmp::Ordering {
        self.active_formatters().compare(a, b)
    }

    fn active_formatters(&self) -> Arc<crate::fmt::Formatters> {
        let locale = self.inner.active.borrow().locale.clone();
        self.inner.formatters.get(&locale)
    }

    /// Fluent source text of `locale` and of each tag after it in its fallback chain.
    /// From January 2026 (ADR-109): a shipped locale's text has its served overrides applied, and a
    /// person language's text is its served entries.
    pub fn catalogue(&self, locale: &str) -> Result<CatalogueText, LocalisationError> {
        let tag = canonical_tag(locale).ok_or(LocalisationError::TagInvalid)?;
        let state = self.read_state();
        if !self.inner.bundled.contains_key(&tag) && !state.people.contains_key(&tag) {
            return Err(LocalisationError::CatalogueMissing);
        }
        let chain = chain_of(&tag, &state.fallback);
        let resources = chain
            .iter()
            .map(|member| CatalogueResource {
                locale: member.clone(),
                ftl: self.resource_text(&state, member),
            })
            .collect();
        Ok(CatalogueText {
            direction: direction_of(&tag),
            locale: tag,
            resources,
        })
    }

    /// The Fluent source a tag serves: bundled text with overrides replaced in place, or the served
    /// entries of a person language in message-id order.
    pub(crate) fn resource_text(&self, state: &State, tag: &str) -> String {
        let served = self.served_entries(state, tag);
        match self.inner.bundled.get(tag) {
            Some(bundled) if served.is_empty() => bundled.text.to_string(),
            Some(bundled) => crate::catalogue::apply_overrides(bundled.text, &served),
            None => served.values().cloned().collect::<Vec<_>>().join("\n\n") + "\n",
        }
    }

    /// message id → entry text of every served stored entry of `tag`.
    pub(crate) fn served_entries(&self, state: &State, tag: &str) -> BTreeMap<String, String> {
        state
            .people
            .get(tag)
            .map(|person| {
                person
                    .messages
                    .iter()
                    .filter(|(id, row)| is_served(id, row))
                    .map(|(id, row)| (id.clone(), row.ftl.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-18
    /// Changes to a language's wording or state; capacity 64 (a lagging receiver reloads with `languages()`).
    pub fn language_changes(&self) -> broadcast::Receiver<LanguageChange> {
        self.inner.changes.subscribe()
    }

    // spec: ordin-desktop/components/Localisation#LOC-LF-1
    /// B3a: reads the person layer, re-checks it after an update and builds the bundles.
    async fn load_people(
        pool: &SqlitePool,
        app_version: &str,
        bundled: &BTreeMap<String, Bundled>,
        formatters: &FormatterCache,
    ) -> Result<BTreeMap<String, Person>, LocalisationError> {
        let now = now_wire();
        store::settle_interrupted_drafts(pool, &now).await?;
        let mut layer = store::load_layer(pool).await?;
        let english = english();
        for (locale, (row, messages)) in layer.iter_mut() {
            if row.checked_against == app_version {
                continue;
            }
            let mut tx = pool.begin().await?;
            let mut need_changing = 0u32;
            let mut removed = 0u32;
            let ids: Vec<String> = messages.keys().cloned().collect();
            for id in ids {
                if !english.facts.contains_key(&id) {
                    store::delete_message(&mut tx, locale, &id).await?;
                    messages.remove(&id);
                    removed += 1;
                    continue;
                }
                let Some(message) = messages.get_mut(&id) else {
                    continue;
                };
                let usable = check_entry(locale, Some(&id), &message.ftl).is_ok();
                store::recheck_message(&mut tx, locale, &id, usable).await?;
                message.usable = usable;
                if !usable {
                    need_changing += 1;
                }
            }
            store::mark_checked(&mut tx, locale, app_version, &now).await?;
            tx.commit().await?;
            row.checked_against = app_version.to_string();
            tracing::info!(
                component = COMPONENT,
                locale = locale.as_str(),
                need_changing = need_changing,
                removed = removed,
                "localisation.person_layer_rechecked"
            );
        }
        let mut people = BTreeMap::new();
        for (locale, (row, messages)) in layer {
            let bundle = Localisation::build_person_bundle(&row, &messages, bundled, formatters);
            people.insert(
                locale,
                Person {
                    row,
                    messages,
                    bundle,
                    draft_done: 0,
                },
            );
        }
        Ok(people)
    }

    /// A person language's bundle: the shipped resource then the served entries overriding it (kind
    /// `shipped`), or the served entries only. `None` when nothing is served.
    pub(crate) fn build_person_bundle(
        row: &LangRow,
        messages: &BTreeMap<String, MsgRow>,
        bundled: &BTreeMap<String, Bundled>,
        formatters: &FormatterCache,
    ) -> Option<Arc<Bundle>> {
        let served: Vec<&str> = messages
            .iter()
            .filter(|(id, message)| is_served(id, message))
            .map(|(_, message)| message.ftl.as_str())
            .collect();
        if served.is_empty() {
            return None;
        }
        let resource = match FluentResource::try_new(served.join("\n\n") + "\n") {
            Ok(resource) => resource,
            Err((resource, _)) => resource,
        };
        let mut bundle = new_bundle(&row.locale, formatters.get(&row.locale));
        if row.kind == LanguageKind::Shipped {
            if let Some(base) = bundled.get(&row.locale) {
                let _ = bundle.add_resource(Arc::clone(&base.resource));
            }
            bundle.add_resource_overriding(Arc::new(resource));
        } else {
            let _ = bundle.add_resource(Arc::new(resource));
        }
        Some(Arc::new(bundle))
    }
}

/// The distinct areas, sorted.
pub(crate) fn all_areas() -> Vec<String> {
    crate::id::MESSAGE_AREAS
        .iter()
        .map(|(_, area)| (*area).to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
