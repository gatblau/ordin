//! `Brand`: start (Internal Logic A), following the profile (B), checking a received pack (C),
//! following the locale (D), activating a row (E) and reading an asset (F).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock, Weak};

use base64::Engine as _;
use ordin_localisation::{ActiveLocale, Localisation};
use ordin_organisation_profile::{
    BrandAsset, BrandSection, OrganisationProfileService, ProfileSnapshot,
};
use ordin_types::domain::identity::{BrandPack, BrandPackSource, BrandPackState, BrandStrings};
use ordin_types::text::{check_https_url, AreaCodes};
use ordin_types::time::Timestamp;
use ordin_types::{BrandPackId, CodedError as _};
use sqlx::SqlitePool;
use tokio::sync::watch;

use crate::check::{self, AssetSlot, EntryInput, Refusal, RefusalKind};
use crate::error::{BrandError, SUPERSEDED};
use crate::fetch::{self, FetchError, Timing};
use crate::model::{AssetBytes, AssetRef, BrandConfig};
use crate::{build_files, store, COMPONENT};

/// Rows kept by pruning (E, assumed bound).
const KEEP_ROWS: usize = 20;
/// The reason stored on a `received` row replaced before it was checked (A4).
const SUPERSEDED_REASON: &str = "Replaced by a newer brand before it was checked.";

/// The codes a link check reports; only `url_invalid` can be returned by `check_https_url`.
const LINK_CODES: AreaCodes = AreaCodes {
    name_empty: "BRAND_URL_INVALID",
    name_too_long: "BRAND_URL_INVALID",
    path_invalid: "BRAND_URL_INVALID",
    url_invalid: "BRAND_URL_INVALID",
};

/// The brand pack in force, its refusals and the assets on disk (Brand.md).
pub struct Brand {
    inner: Arc<Inner>,
}

struct Inner {
    pool: SqlitePool,
    data_root: PathBuf,
    http: reqwest::Client,
    timing: Timing,
    /// The compiled-in build pack.
    build: BuildPack,
    active: watch::Sender<Arc<BrandPack>>,
    locale: RwLock<ActiveLocale>,
    /// The newest refused profile row, kept in memory for `last_refusal`.
    refusal: RwLock<Option<Arc<BrandPack>>>,
    /// One check, activation or restore at a time.
    gate: tokio::sync::Mutex<()>,
}

/// The build's own pack: its section and the bytes of each asset slot.
struct BuildPack {
    section: BrandSection,
    strings: Vec<BrandStrings>,
    assets: Vec<(AssetSlot, BrandAsset, &'static [u8])>,
}

/// Why a check did not activate a pack.
#[derive(Debug)]
pub(crate) enum Failure {
    Fetch {
        slot: AssetSlot,
        http_status: Option<u16>,
        attempt: u32,
    },
    Refused(Refusal),
    Store(BrandError),
}

impl From<BrandError> for Failure {
    fn from(error: BrandError) -> Self {
        Failure::Store(error)
    }
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Self {
        Failure::Refused(refusal)
    }
}

fn read<T>(lock: &RwLock<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

fn to_brand_strings(merged: Vec<check::MergedStrings>) -> Vec<BrandStrings> {
    merged
        .into_iter()
        .map(|m| BrandStrings {
            locale: m.locale,
            product_name: m.product_name,
            coordinator_name: m.coordinator_name,
            about_text: m.about_text,
            legal_text: m.legal_text,
        })
        .collect()
}

fn entries_of(section: &BrandSection) -> BTreeMap<String, EntryInput> {
    section
        .strings
        .iter()
        .map(|(locale, entry)| {
            (
                locale.clone(),
                EntryInput {
                    product_name: entry.product_name.clone(),
                    coordinator_name: entry.coordinator_name.clone(),
                    about_text: entry.about_text.clone(),
                },
            )
        })
        .collect()
}

/// The asset slots of a section, in fetch order (C1).
fn slots_of(section: &BrandSection) -> Vec<(AssetSlot, &BrandAsset)> {
    let mut slots = vec![
        (AssetSlot::LogoLight, &section.logos.light),
        (AssetSlot::LogoDark, &section.logos.dark),
        (AssetSlot::IconSetLight, &section.icon_set.light),
        (AssetSlot::IconSetDark, &section.icon_set.dark),
    ];
    slots.extend(
        section
            .fonts
            .iter()
            .enumerate()
            .map(|(index, font)| (AssetSlot::Font(index), font)),
    );
    slots
}

/// The media type of a stored asset, from its extension (C2 writes `<sha256>.<ext>`).
fn media_type_of(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|e| e.to_str()) {
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    }
}

/// Every stored asset path of a row with its slot.
fn row_paths(pack: &BrandPack) -> Vec<(AssetSlot, &str)> {
    let mut paths = vec![
        (AssetSlot::LogoLight, pack.logo_light_path.as_str()),
        (AssetSlot::LogoDark, pack.logo_dark_path.as_str()),
        (AssetSlot::IconSetLight, pack.icon_set_light_path.as_str()),
        (AssetSlot::IconSetDark, pack.icon_set_dark_path.as_str()),
    ];
    paths.extend(
        pack.font_file_paths
            .iter()
            .enumerate()
            .map(|(index, path)| (AssetSlot::Font(index), path.as_str())),
    );
    paths
}

/// Writes `bytes` to `path` atomically: a `.tmp` file, fsync, rename (A2).
async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt as _;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = path.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&tmp, path).await
}

fn io_error(_: std::io::Error) -> BrandError {
    BrandError::corrupt("brand asset folder")
}

impl BuildPack {
    /// Parses the compiled-in `build-brand/brand.json`; build.rs has already run C2–C5 over it.
    fn load() -> Result<BuildPack, BrandError> {
        // A section that build.rs accepted but the profile's `BrandSection` shape refuses is a
        // defect of the build input: the pack is not acceptable.
        let section: BrandSection =
            serde_json::from_str(build_files::BRAND_JSON).map_err(|_| BrandError::AssetInvalid)?;
        let strings = check::merge_strings(&entries_of(&section), &section.legal_text)
            .map_err(|refusal| BrandError::Refused(refusal.kind))?;
        let mut assets = Vec::new();
        for (slot, asset) in slots_of(&section) {
            let name = asset.url.strip_prefix("file:").unwrap_or(&asset.url);
            let bytes = build_files::ASSET_FILES
                .iter()
                .find(|(file, _)| *file == name)
                .map(|(_, bytes)| *bytes)
                .ok_or(BrandError::AssetInvalid)?;
            assets.push((slot, asset.clone(), bytes));
        }
        Ok(BuildPack {
            section,
            strings: to_brand_strings(strings),
            assets,
        })
    }
}

impl Brand {
    /// Installs the build pack row if missing, restores the active row, then follows the profile.
    pub async fn start(
        config: BrandConfig,
        profile: Arc<OrganisationProfileService>,
        localisation: Arc<Localisation>,
    ) -> Result<Arc<Brand>, BrandError> {
        // A1: the desktop never has a PostgreSQL datastore.
        let Some(pool) = config.datastore.sqlite().cloned() else {
            return Err(BrandError::StoreUnavailable(sqlx::Error::Configuration(
                "brand needs the SQLite datastore".into(),
            )));
        };
        Brand::start_with_sources(
            pool,
            config.data_root,
            config.http,
            Timing::production(),
            profile.subscribe(),
            localisation.active(),
            localisation.subscribe(),
        )
        .await
    }

    /// `start`, over any source of profile snapshots and locales (the followers read the receivers).
    pub(crate) async fn start_with_sources(
        pool: SqlitePool,
        data_root: PathBuf,
        http: reqwest::Client,
        timing: Timing,
        profile: watch::Receiver<Arc<ProfileSnapshot>>,
        locale: ActiveLocale,
        locales: watch::Receiver<ActiveLocale>,
    ) -> Result<Arc<Brand>, BrandError> {
        let build = BuildPack::load()?;
        let placeholder = Arc::new(BrandPack {
            id: BrandPackId::new(),
            brand_identifier: build.section.brand_identifier.clone(),
            version: build.section.version.clone(),
            source: BrandPackSource::Build,
            strings: build.strings.clone(),
            logo_light_path: String::new(),
            logo_dark_path: String::new(),
            icon_set_light_path: String::new(),
            icon_set_dark_path: String::new(),
            design_tokens: build.section.design_tokens.clone(),
            font_file_paths: Vec::new(),
            help_url: build.section.links.help_url.clone(),
            support_url: build.section.links.support_url.clone(),
            privacy_url: build.section.links.privacy_url.clone(),
            state: BrandPackState::Active,
            refusal_reason: None,
            received_at: Timestamp::now(),
            activated_at: None,
        });
        let (active, _) = watch::channel(placeholder);
        let brand = Arc::new(Brand {
            inner: Arc::new(Inner {
                pool,
                data_root,
                http,
                timing,
                build,
                active,
                locale: RwLock::new(locale),
                refusal: RwLock::new(None),
                gate: tokio::sync::Mutex::new(()),
            }),
        });
        {
            let _gate = brand.inner.gate.lock().await;
            // A2.
            brand.install_build_pack().await?;
            // A3.
            brand.restore_active().await?;
            // A4.
            brand.supersede_older_received(None).await?;
            brand.refresh_refusal().await?;
        }
        // A5: the followers hold only a weak reference.
        spawn_profile_follower(
            Arc::downgrade(&brand.inner),
            profile,
            brand.inner.timing.recheck_after,
        );
        spawn_locale_follower(Arc::downgrade(&brand.inner), locales);
        Ok(brand)
    }

    /// The pack in force (never absent: the build pack is the floor).
    pub fn active(&self) -> Arc<BrandPack> {
        self.inner.active.borrow().clone()
    }

    /// Strings of the active pack for the active locale, by the locale's fallback chain,
    /// then the pack's first entry (`strings[0]`).
    // spec: ordin-desktop/components/Brand#SA-RECON-03
    pub fn strings(&self) -> BrandStrings {
        let pack = self.active();
        let locale = read(&self.inner.locale).clone();
        let wanted = std::iter::once(&locale.locale).chain(locale.fallback_chain.iter());
        for tag in wanted {
            let canonical = check::canonical_tag(tag).unwrap_or_else(|| tag.clone());
            if let Some(found) = pack.strings.iter().find(|s| s.locale == canonical) {
                return found.clone();
            }
        }
        pack.strings
            .first()
            .or_else(|| self.inner.build.strings.first())
            .cloned()
            .unwrap_or_else(|| BrandStrings {
                locale: locale.locale.clone(),
                product_name: String::new(),
                coordinator_name: String::new(),
                about_text: String::new(),
                legal_text: String::new(),
            })
    }

    /// The coordinator's display name in the active locale (A-27: the desktop program passes it to
    /// the Ordin Host as `OrdinHostPolicy.coordinator_name`).
    // spec: ordin-desktop/components/Brand#SA-RECON-04
    pub fn coordinator_name(&self) -> String {
        self.strings().coordinator_name
    }

    /// Bytes of one asset of the active pack (F).
    pub async fn asset(&self, which: AssetRef) -> Result<AssetBytes, BrandError> {
        let pack = self.active();
        let path = match which {
            AssetRef::LogoLight => pack.logo_light_path.clone(),
            AssetRef::LogoDark => pack.logo_dark_path.clone(),
            AssetRef::IconSetLight => pack.icon_set_light_path.clone(),
            AssetRef::IconSetDark => pack.icon_set_dark_path.clone(),
            AssetRef::Font { index } => pack
                .font_file_paths
                .get(usize::from(index))
                .cloned()
                .ok_or(BrandError::AssetUnknown)?,
        };
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(AssetBytes {
                media_type: media_type_of(&path).to_owned(),
                base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            }),
            Err(_) => {
                // A missing file: re-run A3, which activates the build pack.
                let _gate = self.inner.gate.lock().await;
                self.restore_active().await?;
                Err(BrandError::AssetFetchFailed)
            }
        }
    }

    /// The newest pack received from a profile that was refused, with its reason, if it is newer
    /// than the active one (for Settings › Diagnostics and **Send to IT**).
    pub fn last_refusal(&self) -> Option<Arc<BrandPack>> {
        let refusal = read(&self.inner.refusal).clone()?;
        let active = self.active();
        (refusal.received_at >= active.received_at && refusal.id != active.id).then_some(refusal)
    }

    /// Notified each time the active pack changes.
    pub fn subscribe(&self) -> watch::Receiver<Arc<BrandPack>> {
        self.inner.active.subscribe()
    }

    // ---- A: start

    fn folder(&self, brand_identifier: &str, version: &str) -> PathBuf {
        self.inner
            .data_root
            .join("brand")
            .join(brand_identifier)
            .join(version)
    }

    /// A2: the build pack's files (written when missing) and its row (inserted when missing).
    async fn install_build_pack(&self) -> Result<(), BrandError> {
        let build = &self.inner.build;
        let section = &build.section;
        let folder = self.folder(&section.brand_identifier, &section.version);
        let mut paths: Vec<(AssetSlot, String)> = Vec::new();
        for (slot, asset, bytes) in &build.assets {
            let extension = Path::new(&asset.url)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("bin");
            let path = folder.join(format!("{}.{extension}", asset.sha256.0));
            // A file deleted later would leave the floor pack without its assets and A3 with
            // nothing to fall back to, so a missing build file is written again at every start.
            if tokio::fs::metadata(&path).await.is_err() {
                write_atomic(&path, bytes).await.map_err(io_error)?;
            }
            paths.push((*slot, path.to_string_lossy().into_owned()));
        }
        let existing = store::by_key(
            &self.inner.pool,
            &section.brand_identifier,
            &section.version,
            BrandPackSource::Build,
        )
        .await?;
        if existing.is_some() {
            return Ok(());
        }
        let now = Timestamp::now();
        let none_active = store::active(&self.inner.pool).await?.is_none();
        let path_of = |wanted: AssetSlot| {
            paths
                .iter()
                .find(|(slot, _)| *slot == wanted)
                .map(|(_, path)| path.clone())
                .unwrap_or_default()
        };
        let pack = BrandPack {
            id: BrandPackId::new(),
            brand_identifier: section.brand_identifier.clone(),
            version: section.version.clone(),
            source: BrandPackSource::Build,
            strings: build.strings.clone(),
            logo_light_path: path_of(AssetSlot::LogoLight),
            logo_dark_path: path_of(AssetSlot::LogoDark),
            icon_set_light_path: path_of(AssetSlot::IconSetLight),
            icon_set_dark_path: path_of(AssetSlot::IconSetDark),
            design_tokens: section.design_tokens.clone(),
            font_file_paths: paths
                .iter()
                .filter(|(slot, _)| slot.is_font())
                .map(|(_, path)| path.clone())
                .collect(),
            help_url: section.links.help_url.clone(),
            support_url: section.links.support_url.clone(),
            privacy_url: section.links.privacy_url.clone(),
            state: if none_active {
                BrandPackState::Active
            } else {
                BrandPackState::ContrastChecked
            },
            refusal_reason: None,
            received_at: now,
            activated_at: none_active.then_some(now),
        };
        store::insert(&self.inner.pool, &pack).await
    }

    /// The first asset of `pack` whose file is missing or whose SHA-256 differs from its name.
    async fn damaged_asset(pack: &BrandPack) -> Option<AssetSlot> {
        for (slot, path) in row_paths(pack) {
            let expected = Path::new(path).file_stem().and_then(|s| s.to_str());
            let intact = match (tokio::fs::read(path).await, expected) {
                (Ok(bytes), Some(expected)) => check::sha256_hex(&bytes) == expected,
                _ => false,
            };
            if !intact {
                return Some(slot);
            }
        }
        None
    }

    /// A3: restores the `active` row if its files are intact, else activates the build pack.
    async fn restore_active(&self) -> Result<(), BrandError> {
        let active = store::active(&self.inner.pool).await?;
        if let Some(pack) = active {
            // An older build pack left active by an earlier install gives way to the build pack
            // this build stored in A2 (B1's rule for the build floor; amendment January 2026,
            // item 3). A company pack stays active.
            if pack.source == BrandPackSource::Build {
                if let Some(newest) = store::newest_build(&self.inner.pool).await? {
                    if newest.id != pack.id {
                        return self.activate(newest.id).await;
                    }
                }
            }
            match Brand::damaged_asset(&pack).await {
                None => {
                    self.publish(pack);
                    return Ok(());
                }
                Some(slot) => {
                    tracing::warn!(
                        component = COMPONENT,
                        brand_identifier = pack.brand_identifier.as_str(),
                        version = pack.version.as_str(),
                        asset = slot.kind(),
                        "brand.asset_missing"
                    );
                    if pack.source == BrandPackSource::Build {
                        // The build pack's own files were rewritten by A2; a damaged build row is
                        // still the floor.
                        self.publish(pack);
                        return Ok(());
                    }
                }
            }
        }
        self.activate_newest_build().await
    }

    /// A4 and B: every `received` row other than `keep` is refused as superseded; with `keep`
    /// `None`, every row but the newest.
    async fn supersede_older_received(&self, keep: Option<BrandPackId>) -> Result<(), BrandError> {
        let received = store::received(&self.inner.pool).await?;
        let keep = keep.or_else(|| received.first().map(|row| row.id));
        // A `received` row holds no section (its columns are empty until checked), so C cannot run
        // for it at start. The newest row is kept `received`; the
        // profile follower's first snapshot carries its section and runs C on it (B, the same
        // `(brand_identifier, version)`), or supersedes it if the profile now names another pack.
        for row in received.into_iter().filter(|row| Some(row.id) != keep) {
            store::mark_refused(&self.inner.pool, row.id, SUPERSEDED_REASON).await?;
            tracing::warn!(
                component = COMPONENT,
                brand_identifier = row.brand_identifier.as_str(),
                version = row.version.as_str(),
                code = SUPERSEDED,
                "brand.refused"
            );
        }
        Ok(())
    }

    async fn refresh_refusal(&self) -> Result<(), BrandError> {
        let newest = store::newest_refusal(&self.inner.pool).await?;
        *write(&self.inner.refusal) = newest.map(Arc::new);
        Ok(())
    }

    fn publish(&self, pack: BrandPack) {
        self.inner.active.send_replace(Arc::new(pack));
    }

    async fn activate_newest_build(&self) -> Result<(), BrandError> {
        let Some(build) = store::newest_build(&self.inner.pool).await? else {
            return Err(BrandError::corrupt("brand_packs build row"));
        };
        if build.state == BrandPackState::Active {
            self.publish(build);
            return Ok(());
        }
        self.activate(build.id).await
    }

    // ---- B: following the profile

    /// B for one snapshot.
    pub(crate) async fn follow(&self, snapshot: &ProfileSnapshot) -> Result<(), BrandError> {
        match &snapshot.document.brand {
            None => {
                let _gate = self.inner.gate.lock().await;
                let active = self.active();
                if active.source == BrandPackSource::Build {
                    // Activate the newest build row if it is not the one active.
                    if let Some(newest) = store::newest_build(&self.inner.pool).await? {
                        if newest.id != active.id {
                            self.activate(newest.id).await?;
                        }
                    }
                    return Ok(());
                }
                self.activate_newest_build().await
            }
            Some(section) => self.receive(section).await,
        }
    }

    /// B and C for one brand section: `Ok` when nothing was to do or the pack became active;
    /// `Err(Refused(..))` when it was refused; `Err(AssetFetchFailed)` when it stays `received`.
    pub(crate) async fn receive(&self, section: &BrandSection) -> Result<(), BrandError> {
        let _gate = self.inner.gate.lock().await;
        let active = self.active();
        if active.brand_identifier == section.brand_identifier && active.version == section.version
        {
            return Ok(());
        }
        let existing = store::by_key(
            &self.inner.pool,
            &section.brand_identifier,
            &section.version,
            BrandPackSource::OrganisationProfile,
        )
        .await?;
        let row = match existing {
            // A refused version is not retried; IT publishes a new version.
            Some(row) if row.state == BrandPackState::Refused => return Ok(()),
            Some(row) if row.state == BrandPackState::Active => return Ok(()),
            // A `received` row is checked again (C1: retried at the next snapshot); a
            // `contrast_checked` row superseded earlier is checked again before it returns.
            // A profile that names a pack superseded earlier: its row is checked again (C) and
            // activated, since a new row would break the table's UNIQUE (brand_identifier, version,
            // source).
            Some(row) => row,
            None => {
                if !(1..=64).contains(&section.brand_identifier.len()) {
                    // An identifier outside 1–64 characters cannot be stored (the
                    // table's CHECK), so the pack is refused without a row; only the log line
                    // records it. The Organisation Profile refuses such a section first.
                    tracing::warn!(
                        component = COMPONENT,
                        brand_identifier = "",
                        version = section.version.as_str(),
                        code = RefusalKind::IdInvalid.code(),
                        "brand.refused"
                    );
                    return Err(BrandError::Refused(RefusalKind::IdInvalid));
                }
                let row = BrandPack {
                    id: BrandPackId::new(),
                    brand_identifier: section.brand_identifier.clone(),
                    version: section.version.clone(),
                    source: BrandPackSource::OrganisationProfile,
                    strings: Vec::new(),
                    logo_light_path: String::new(),
                    logo_dark_path: String::new(),
                    icon_set_light_path: String::new(),
                    icon_set_dark_path: String::new(),
                    design_tokens: BTreeMap::new(),
                    font_file_paths: Vec::new(),
                    help_url: String::new(),
                    support_url: String::new(),
                    privacy_url: String::new(),
                    state: BrandPackState::Received,
                    refusal_reason: None,
                    received_at: Timestamp::now(),
                    activated_at: None,
                };
                store::insert(&self.inner.pool, &row).await?;
                tracing::info!(
                    component = COMPONENT,
                    brand_identifier = row.brand_identifier.as_str(),
                    version = row.version.as_str(),
                    "brand.received"
                );
                row
            }
        };
        self.supersede_older_received(Some(row.id)).await?;
        self.check_and_apply(row, section).await
    }

    // ---- C: checking a received pack

    async fn check_and_apply(
        &self,
        row: BrandPack,
        section: &BrandSection,
    ) -> Result<(), BrandError> {
        match self.check(&row, section).await {
            Ok(checked) => {
                // C7, pass.
                store::mark_checked(&self.inner.pool, &checked).await?;
                self.activate(checked.id).await
            }
            Err(Failure::Fetch {
                slot,
                http_status,
                attempt,
            }) => {
                let status = http_status.map_or_else(|| "none".to_owned(), |s| s.to_string());
                tracing::warn!(
                    component = COMPONENT,
                    brand_identifier = row.brand_identifier.as_str(),
                    version = row.version.as_str(),
                    asset = slot.kind(),
                    http_status = status.as_str(),
                    attempt = u64::from(attempt),
                    "brand.asset_fetch_failed"
                );
                Err(BrandError::AssetFetchFailed)
            }
            Err(Failure::Refused(refusal)) => {
                // C7, refusal.
                store::mark_refused(&self.inner.pool, row.id, &refusal.reason).await?;
                self.delete_folder_unless_used(&row).await?;
                tracing::warn!(
                    component = COMPONENT,
                    brand_identifier = row.brand_identifier.as_str(),
                    version = row.version.as_str(),
                    code = refusal.kind.code(),
                    "brand.refused"
                );
                self.refresh_refusal().await?;
                Err(BrandError::Refused(refusal.kind))
            }
            Err(Failure::Store(error)) => Err(error),
        }
    }

    /// Removes `<data_root>/brand/<id>/<version>/` unless another row that is not refused shares it.
    async fn delete_folder_unless_used(&self, row: &BrandPack) -> Result<(), BrandError> {
        let all = store::all_newest_first(&self.inner.pool).await?;
        let shared = all.iter().any(|other| {
            other.id != row.id
                && other.brand_identifier == row.brand_identifier
                && other.version == row.version
                && other.state != BrandPackState::Refused
        });
        if !shared {
            let _ =
                tokio::fs::remove_dir_all(self.folder(&row.brand_identifier, &row.version)).await;
        }
        Ok(())
    }

    /// C1–C6 over `section`; on success the row as it will be stored in `contrast_checked`.
    pub(crate) async fn check(
        &self,
        row: &BrandPack,
        section: &BrandSection,
    ) -> Result<BrandPack, Failure> {
        check::check_identifier(&section.brand_identifier)?;
        check::check_version(&section.version)?;
        check::check_font_count(section.fonts.len())?;

        // C1: fetch every asset, in order.
        let slots = slots_of(section);
        let mut bodies = Vec::with_capacity(slots.len());
        for (slot, asset) in &slots {
            match fetch::fetch(&self.inner.http, &asset.url, *slot, &self.inner.timing).await {
                Ok(body) => bodies.push(body),
                Err(FetchError::Refused(refusal)) => return Err(Failure::Refused(refusal)),
                Err(FetchError::Failed {
                    http_status,
                    attempt,
                }) => {
                    return Err(Failure::Fetch {
                        slot: *slot,
                        http_status,
                        attempt,
                    })
                }
            }
        }

        // C2: verify each asset and write the accepted bodies.
        let folder = self.folder(&section.brand_identifier, &section.version);
        let mut paths: Vec<(AssetSlot, String)> = Vec::with_capacity(slots.len());
        for ((slot, asset), body) in slots.iter().zip(&bodies) {
            let extension = check::check_asset(*slot, &asset.sha256.0, &asset.media_type, body)?;
            let path = folder.join(format!("{}.{extension}", asset.sha256.0));
            write_atomic(&path, body).await.map_err(io_error)?;
            paths.push((*slot, path.to_string_lossy().into_owned()));
        }

        // C3–C5.
        let strings = check::merge_strings(&entries_of(section), &section.legal_text)?;
        check::check_tokens(&section.design_tokens)?;
        check::check_contrast(&section.design_tokens)?;

        // C6: links, `https` only.
        for (which, link) in [
            ("help", &section.links.help_url),
            ("support", &section.links.support_url),
            ("privacy", &section.links.privacy_url),
        ] {
            if check_https_url(link, false, LINK_CODES).is_err() {
                return Err(Failure::Refused(check::refusal(
                    RefusalKind::UrlInvalid,
                    format!("The brand's {which} link isn't a secure web address."),
                )));
            }
        }

        let path_of = |wanted: AssetSlot| {
            paths
                .iter()
                .find(|(slot, _)| *slot == wanted)
                .map(|(_, path)| path.clone())
                .unwrap_or_default()
        };
        Ok(BrandPack {
            strings: to_brand_strings(strings),
            logo_light_path: path_of(AssetSlot::LogoLight),
            logo_dark_path: path_of(AssetSlot::LogoDark),
            icon_set_light_path: path_of(AssetSlot::IconSetLight),
            icon_set_dark_path: path_of(AssetSlot::IconSetDark),
            design_tokens: section.design_tokens.clone(),
            font_file_paths: paths
                .iter()
                .filter(|(slot, _)| slot.is_font())
                .map(|(_, path)| path.clone())
                .collect(),
            help_url: section.links.help_url.clone(),
            support_url: section.links.support_url.clone(),
            privacy_url: section.links.privacy_url.clone(),
            state: BrandPackState::ContrastChecked,
            ..row.clone()
        })
    }

    // ---- E: activating a row

    async fn activate(&self, id: BrandPackId) -> Result<(), BrandError> {
        store::activate(&self.inner.pool, id, Timestamp::now()).await?;
        let pack = store::by_id(&self.inner.pool, id).await?;
        tracing::info!(
            component = COMPONENT,
            brand_identifier = pack.brand_identifier.as_str(),
            version = pack.version.as_str(),
            source = store::source_text(pack.source),
            "brand.activated"
        );
        self.publish(pack);
        self.prune().await?;
        self.refresh_refusal().await
    }

    /// E: rows beyond the 20 most recent, except the active row and the newest build row, are
    /// deleted with their asset folders.
    async fn prune(&self) -> Result<(), BrandError> {
        let all = store::all_newest_first(&self.inner.pool).await?;
        if all.len() <= KEEP_ROWS {
            return Ok(());
        }
        let newest_build = all
            .iter()
            .find(|row| row.source == BrandPackSource::Build)
            .map(|row| row.id);
        let (kept, beyond) = all.split_at(KEEP_ROWS);
        let mut kept: Vec<&BrandPack> = kept.iter().collect();
        let mut doomed = Vec::new();
        for row in beyond {
            if row.state == BrandPackState::Active || Some(row.id) == newest_build {
                kept.push(row);
            } else {
                doomed.push(row);
            }
        }
        for row in &doomed {
            store::delete(&self.inner.pool, row.id).await?;
            let shared = kept.iter().any(|other| {
                other.brand_identifier == row.brand_identifier && other.version == row.version
            });
            if !shared {
                let _ = tokio::fs::remove_dir_all(self.folder(&row.brand_identifier, &row.version))
                    .await;
            }
        }
        tracing::debug!(
            component = COMPONENT,
            rows_deleted = u64::try_from(doomed.len()).unwrap_or(u64::MAX),
            "brand.pruned"
        );
        Ok(())
    }

    // ---- D: following the locale

    /// D: the strings follow the new locale; the unchanged pack is published again so the program
    /// emits `brand_changed`.
    pub(crate) fn locale_changed(&self, locale: ActiveLocale) {
        *write(&self.inner.locale) = locale;
        self.inner.active.send_modify(|_| {});
    }
}

/// B, on its own task: each snapshot, and a retry after `recheck` while a check is left pending.
fn spawn_profile_follower(
    weak: Weak<Inner>,
    mut changes: watch::Receiver<Arc<ProfileSnapshot>>,
    recheck: std::time::Duration,
) {
    tokio::spawn(async move {
        let mut pending = false;
        loop {
            let snapshot = if pending {
                tokio::select! {
                    changed = changes.changed() => {
                        if changed.is_err() {
                            break;
                        }
                        Arc::clone(&changes.borrow_and_update())
                    }
                    () = tokio::time::sleep(recheck) => Arc::clone(&changes.borrow()),
                }
            } else {
                if changes.changed().await.is_err() {
                    break;
                }
                Arc::clone(&changes.borrow_and_update())
            };
            let Some(inner) = weak.upgrade() else { break };
            let brand = Brand { inner };
            // A retryable failure (fetch exhausted, store unavailable) is checked again after
            // `recheck` or at the next snapshot, whichever comes first (C1).
            pending = match brand.follow(&snapshot).await {
                Ok(()) => false,
                Err(error) => error.retryable(),
            };
        }
    });
}

/// D, on its own task.
fn spawn_locale_follower(weak: Weak<Inner>, mut changes: watch::Receiver<ActiveLocale>) {
    tokio::spawn(async move {
        while changes.changed().await.is_ok() {
            let locale = changes.borrow_and_update().clone();
            let Some(inner) = weak.upgrade() else { break };
            Brand { inner }.locale_changed(locale);
        }
    });
}
