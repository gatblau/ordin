//! The test kit (feature `testkit`): an in-memory `PartSource` and the contract every `PartSource`
//! implementation passes (AgentModel.md *Test kit*, *Internal Logic* 12).
//!
//! The contract test panics with the name of the failing rule; it is test support, built only with
//! the `testkit` feature.

use std::collections::{BTreeMap, BTreeSet};

use ordin_types::domain::agents::{Part, PartKind, VocabularyAxis, WithdrawnListEntry};
use ordin_types::time::Timestamp;
use ordin_types::AgentRepositoryId;
use ordin_types::{PartId, VocabularyTermId, WithdrawnListEntryId};

use crate::archive::content_digest;
use crate::draft::finish;
use crate::error::PartSourceError;
use crate::kinds::{all_kinds, kind_index, kind_info, KindFamily};
use crate::manifest::{Manifest, Metadata};
use crate::plan::compare_versions;
use crate::source::{PartSource, RepositoryRef, Resolution, SourceScope, SourcedPart};
use crate::tree::{ModelPart, PartFiles};
use crate::value::YamlValue;
use crate::vocabulary::{axis_str, VocabularySet};
use crate::{API_VERSION, BUNDLED_REPOSITORY_ID};

/// The fixed repositories the contract test uses: a home repository, two profile
/// repositories sharing one identifier, the bundled catalogue, a withdrawn part, a
/// draft-only part, a vocabulary with one cited term, and one part the ingest refused.
///
/// `repositories` holds, in this order: the home repository, the first and the second profile
/// repository (in the organisation profile's listed order), the bundled catalogue, and a repository
/// the person added (searched only as `home` of its own agents).
pub struct ContractFixture {
    pub repositories: Vec<(RepositoryRef, String, Vec<ModelPart>)>,
    pub withdrawn: Vec<WithdrawnListEntry>,
    pub refused: Vec<(RepositoryRef, String)>,
}

/// The identifiers of `ContractFixture::standard()`.
mod ids {
    pub const HELPER: &str = "urn:ordin:acme:workspaces:helper";
    pub const WRITING: &str = "urn:ordin:acme:capabilities:writing";
    pub const SUMMARISE: &str = "urn:ordin:acme:skills:summarise";
    pub const WIP: &str = "urn:ordin:acme:skills:wip";
    pub const OLD: &str = "urn:ordin:acme:skills:old";
    pub const BROKEN: &str = "urn:ordin:acme:skills:broken";
    pub const SHARED: &str = "urn:ordin:acme:rules:shared";
    pub const TONE: &str = "urn:ordin:acme:rules:tone";
    pub const POLICY_ONLY: &str = "urn:ordin:acme:rules:policy-only";
    pub const LEGAL_ONLY: &str = "urn:ordin:acme:rules:legal-only";
    pub const BUNDLED_ONLY: &str = "urn:ordin:default:rules:bundled-only";
    pub const GO_PACK: &str = "urn:ordin:default:practice-packs:stack:go";
    pub const PLAIN_STYLE: &str = "urn:ordin:default:overlays:style:plain";
    pub const PRIVATE: &str = "urn:ordin:someone:rules:private";
    pub const VOCABULARY: &str = "urn:ordin:acme:vocabulary:categories";
}

const HOME: usize = 0;
const PROFILE_A: usize = 1;
const PROFILE_B: usize = 2;
const BUNDLED: usize = 3;
const ADDED: usize = 4;

fn repository_id(n: u128) -> AgentRepositoryId {
    AgentRepositoryId(uuid::Uuid::from_u128(
        0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
    ))
}

fn str_list(items: &[&str]) -> YamlValue {
    YamlValue::Seq(
        items
            .iter()
            .map(|s| YamlValue::Str((*s).to_owned()))
            .collect(),
    )
}

/// One fixture part, rendered and digested as the ordin format writes it.
fn part(
    kind: PartKind,
    id: &str,
    version: &str,
    status: Option<&str>,
    category: &str,
    body: Vec<(&str, YamlValue)>,
) -> ModelPart {
    let mut metadata = Metadata::with_id(id);
    metadata.name = crate::ident::slug_from_id(id);
    metadata.version = version.to_owned();
    metadata.owner = "acme".to_owned();
    metadata.category = category.to_owned();
    metadata.status = status.map(str::to_owned);
    let manifest = Manifest {
        api_version: API_VERSION.to_owned(),
        authored_kind: kind_info(kind)
            .authored_spellings
            .first()
            .map(|s| (*s).to_owned())
            .unwrap_or_default(),
        metadata,
        body: body.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
    };
    let text = matches!(
        kind,
        PartKind::Rule | PartKind::Skill | PartKind::StyleOverlay
    )
    .then_some("Text.\n");
    // Every fixture identifier has a file convention and a URN; the fallback is never taken.
    finish(kind, manifest, text).unwrap_or_else(|_| panic!("fixture part {id} renders"))
}

fn rule(id: &str, category: &str) -> ModelPart {
    part(PartKind::Rule, id, "1.0.0", None, category, Vec::new())
}

fn skill(id: &str, version: &str, status: Option<&str>) -> ModelPart {
    part(PartKind::Skill, id, version, status, "", Vec::new())
}

fn timestamp(ms: i64) -> Timestamp {
    Timestamp::from_unix_millis(ms).unwrap_or_else(|_| Timestamp::now())
}

fn withdrawal(repository: AgentRepositoryId, id: &str, n: u128) -> WithdrawnListEntry {
    WithdrawnListEntry {
        id: WithdrawnListEntryId(uuid::Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0100 | n,
        )),
        agent_repository_id: repository,
        part_kind: PartKind::Skill,
        part_identifier: id.to_owned(),
        part_version: None,
        part_content_digest: None,
        reason: "Replaced.".to_owned(),
        withdrawn_at: timestamp(1_760_000_000_000),
        replacement_identifier: None,
        replacement_version: None,
        removed_by_hand_seen_at: None,
        read_at: timestamp(1_760_000_000_000),
    }
}

impl ContractFixture {
    pub fn standard() -> ContractFixture {
        use ids::*;
        let home = RepositoryRef::Repository(repository_id(0xa1));
        let profile_a = RepositoryRef::Repository(repository_id(0xa2));
        let profile_b = RepositoryRef::Repository(repository_id(0xa3));
        let added = RepositoryRef::Repository(repository_id(0xa4));
        let vocabulary = part(
            PartKind::Vocabulary,
            VOCABULARY,
            "1.0.0",
            None,
            "",
            vec![
                ("axis", YamlValue::Str("category".into())),
                (
                    "terms",
                    YamlValue::Seq(vec![YamlValue::Map(BTreeMap::from([
                        ("code".to_owned(), YamlValue::Str("finance".into())),
                        ("label".to_owned(), YamlValue::Str("Finance".into())),
                        (
                            "description".to_owned(),
                            YamlValue::Str("Money matters.".into()),
                        ),
                    ]))]),
                ),
            ],
        );
        let home_parts = vec![
            part(
                PartKind::WorkspaceProfile,
                HELPER,
                "1.0.0",
                None,
                "",
                vec![
                    ("defaultRules", str_list(&[SHARED])),
                    ("capabilities", str_list(&[WRITING])),
                ],
            ),
            part(
                PartKind::Capability,
                WRITING,
                "1.0.0",
                None,
                "",
                vec![("skills", str_list(&[SUMMARISE]))],
            ),
            skill(SUMMARISE, "1.0.0", None),
            skill(SUMMARISE, "1.1.0", Some("approved")),
            skill(SUMMARISE, "2.0.0", Some("draft")),
            skill(WIP, "0.1.0", Some("draft")),
            skill(OLD, "1.0.0", None),
            skill(BROKEN, "1.0.0", None),
            rule(SHARED, ""),
            vocabulary,
        ];
        let bundled_parts = vec![
            rule(SHARED, ""),
            rule(POLICY_ONLY, ""),
            rule(LEGAL_ONLY, ""),
            rule(BUNDLED_ONLY, ""),
            part(
                PartKind::PracticePack,
                GO_PACK,
                "1.0.0",
                None,
                "",
                Vec::new(),
            ),
            part(
                PartKind::PracticePack,
                GO_PACK,
                "1.1.0",
                None,
                "",
                Vec::new(),
            ),
            part(
                PartKind::PracticePack,
                GO_PACK,
                "2.0.0",
                Some("draft"),
                "",
                Vec::new(),
            ),
            part(
                PartKind::StyleOverlay,
                PLAIN_STYLE,
                "1.0.0",
                None,
                "",
                Vec::new(),
            ),
        ];
        let repository_of = |r: &RepositoryRef| match r {
            RepositoryRef::Repository(id) => *id,
            RepositoryRef::Bundled => BUNDLED_REPOSITORY_ID,
        };
        ContractFixture {
            withdrawn: vec![
                withdrawal(repository_of(&home), OLD, 1),
                withdrawal(repository_of(&added), PRIVATE, 2),
            ],
            refused: vec![(home.clone(), BROKEN.to_owned())],
            repositories: vec![
                (home, "github.com/acme/home".to_owned(), home_parts),
                (
                    profile_a,
                    "github.com/acme/policies".to_owned(),
                    vec![
                        rule(TONE, "finance"),
                        rule(SHARED, ""),
                        rule(POLICY_ONLY, ""),
                    ],
                ),
                (
                    profile_b,
                    "github.com/acme/legal".to_owned(),
                    vec![rule(TONE, ""), rule(LEGAL_ONLY, "")],
                ),
                (RepositoryRef::Bundled, "bundled".to_owned(), bundled_parts),
                (
                    added,
                    "github.com/someone/own".to_owned(),
                    vec![rule(PRIVATE, "")],
                ),
            ],
        }
    }
}

/// One part as the in-memory source holds it.
struct Entry {
    repository: usize,
    sourced: SourcedPart,
    model: ModelPart,
    refused: bool,
}

/// An in-memory PartSource over fixed repositories, keeping every contract rule.
pub struct InMemorySource {
    repositories: Vec<(RepositoryRef, String)>,
    entries: Vec<Entry>,
    withdrawn: Vec<WithdrawnListEntry>,
    /// (repository index, axis, code) → term id.
    terms: BTreeMap<(usize, &'static str, String), VocabularyTermId>,
}

/// The codes a part's metadata holds on `axis`.
fn metadata_codes(metadata: &Metadata, axis: VocabularyAxis) -> Vec<String> {
    match axis {
        VocabularyAxis::Category => vec![metadata.category.clone()],
        VocabularyAxis::PackClass => vec![metadata.class.clone()],
        VocabularyAxis::AudienceRole => metadata.audience.roles.clone(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|code| !code.is_empty())
    .collect()
}

const CITED_AXES: [VocabularyAxis; 3] = [
    VocabularyAxis::Category,
    VocabularyAxis::PackClass,
    VocabularyAxis::AudienceRole,
];

impl InMemorySource {
    pub fn new(fixture: &ContractFixture) -> InMemorySource {
        let mut terms = BTreeMap::new();
        for (index, (_, _, parts)) in fixture.repositories.iter().enumerate() {
            for model in parts.iter().filter(|p| p.kind == PartKind::Vocabulary) {
                if let Ok(Some(body)) = model.manifest.vocabulary() {
                    for term in body.terms {
                        terms
                            .entry((index, axis_str(body.axis), term.code))
                            .or_insert_with(VocabularyTermId::new);
                    }
                }
            }
        }
        let mut entries = Vec::new();
        for (index, (repository, label, parts)) in fixture.repositories.iter().enumerate() {
            let repository_id = match repository {
                RepositoryRef::Repository(id) => *id,
                RepositoryRef::Bundled => BUNDLED_REPOSITORY_ID,
            };
            for model in parts {
                let metadata = &model.manifest.metadata;
                let withdrawn = fixture.withdrawn.iter().any(|w| {
                    w.agent_repository_id == repository_id
                        && w.part_kind == model.kind
                        && w.part_identifier == metadata.id
                        && w.part_version
                            .as_ref()
                            .is_none_or(|v| *v == metadata.version)
                });
                let cited = CITED_AXES
                    .iter()
                    .flat_map(|axis| {
                        metadata_codes(metadata, *axis)
                            .into_iter()
                            .map(move |code| (axis_str(*axis), code))
                    })
                    .flat_map(|(axis, code)| {
                        terms
                            .iter()
                            .filter(move |((_, a, c), _)| *a == axis && *c == code)
                            .map(|(_, id)| *id)
                    })
                    .collect();
                let part = Part {
                    id: PartId::new(),
                    agent_repository_id: repository_id,
                    kind: model.kind,
                    identifier: metadata.id.clone(),
                    name: metadata.name.clone(),
                    version: metadata.version.clone(),
                    status: model
                        .manifest
                        .status_or_default()
                        .unwrap_or(ordin_types::domain::agents::PartStatus::Approved),
                    supersedes: metadata.supersedes.clone(),
                    owner: metadata.owner.clone(),
                    file_path: model.manifest_path.clone(),
                    commit: "0".repeat(40),
                    content_digest: model.content_digest.clone(),
                    vocabulary_terms_cited: cited,
                    uses: Vec::new(),
                    withdrawn,
                };
                entries.push(Entry {
                    repository: index,
                    sourced: SourcedPart {
                        part,
                        repository: repository.clone(),
                        repository_label: label.clone(),
                        repository_display_name: if *repository == RepositoryRef::Bundled {
                            "ordin".to_owned()
                        } else {
                            label.clone()
                        },
                        manifest: model.manifest.clone(),
                    },
                    model: model.clone(),
                    refused: fixture
                        .refused
                        .iter()
                        .any(|(r, id)| r == repository && *id == metadata.id),
                });
            }
        }
        InMemorySource {
            repositories: fixture
                .repositories
                .iter()
                .map(|(r, label, _)| (r.clone(), label.clone()))
                .collect(),
            entries,
            withdrawn: fixture.withdrawn.clone(),
            terms,
        }
    }

    fn index_of(&self, repository: &RepositoryRef) -> Option<usize> {
        self.repositories.iter().position(|(r, _)| r == repository)
    }

    /// The steps of the resolution order: home, the profile repositories together, bundled.
    fn steps(&self, scope: &SourceScope) -> Vec<Vec<usize>> {
        let mut steps = vec![self.index_of(&scope.home).into_iter().collect::<Vec<_>>()];
        steps.push(
            scope
                .profile
                .iter()
                .filter_map(|r| self.index_of(r))
                .collect(),
        );
        if scope.include_bundled {
            steps.push(self.index_of(&RepositoryRef::Bundled).into_iter().collect());
        }
        steps
    }

    fn visible(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.refused)
    }

    /// Within one repository: the exact version, or the highest non-draft one.
    fn pick(
        &self,
        repository: usize,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Option<&Entry> {
        let candidates = self.visible().filter(|e| {
            e.repository == repository
                && e.sourced.part.kind == kind
                && e.sourced.part.identifier == id
        });
        match version {
            Some(version) => candidates
                .into_iter()
                .find(|e| e.sourced.part.version == version),
            None => candidates
                .filter(|e| e.sourced.part.status != ordin_types::domain::agents::PartStatus::Draft)
                .max_by(|a, b| {
                    compare_versions(&a.sourced.part.version, &b.sourced.part.version)
                        .unwrap_or(std::cmp::Ordering::Equal)
                }),
        }
    }

    fn lookup(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Resolution {
        for step in self.steps(scope) {
            let holders: Vec<usize> = step
                .into_iter()
                .filter(|r| {
                    self.visible().any(|e| {
                        e.repository == *r
                            && e.sourced.part.kind == kind
                            && e.sourced.part.identifier == id
                    })
                })
                .collect();
            match holders.as_slice() {
                [] => continue,
                [one] => {
                    return match self.pick(*one, kind, id, version) {
                        Some(entry) => Resolution::Found(entry.sourced.clone()),
                        None => Resolution::NotFound,
                    }
                }
                many => {
                    let mut repositories: Vec<String> = many
                        .iter()
                        .map(|r| self.repositories[*r].1.clone())
                        .collect();
                    repositories.sort();
                    return Resolution::Ambiguous { repositories };
                }
            }
        }
        Resolution::NotFound
    }

    /// The identifiers a part's references reach, following references through every repository.
    fn closure(&self, start: &Manifest) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut queue: Vec<String> = start.references().into_iter().map(|r| r.id).collect();
        while let Some(id) = queue.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            for entry in self.visible().filter(|e| e.sourced.part.identifier == id) {
                queue.extend(
                    entry
                        .sourced
                        .manifest
                        .references()
                        .into_iter()
                        .map(|r| r.id),
                );
            }
        }
        seen
    }

    /// The head of each identifier of one repository: its highest non-draft version.
    fn is_head(&self, entry: &Entry) -> bool {
        self.pick(
            entry.repository,
            entry.sourced.part.kind,
            &entry.sourced.part.identifier,
            None,
        )
        .is_some_and(|head| head.sourced.part.version == entry.sourced.part.version)
    }
}

fn sorted(mut parts: Vec<SourcedPart>) -> Vec<SourcedPart> {
    parts.sort_by(|a, b| {
        a.repository_label
            .cmp(&b.repository_label)
            .then_with(|| kind_index(a.part.kind).cmp(&kind_index(b.part.kind)))
            .then_with(|| a.part.identifier.cmp(&b.part.identifier))
            .then_with(|| a.part.version.cmp(&b.part.version))
    });
    parts
}

#[async_trait::async_trait]
impl PartSource for InMemorySource {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        Ok(self.lookup(scope, kind, id, version))
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        let mut found = Vec::new();
        for kind in all_kinds() {
            match self.lookup(scope, *kind, id, None) {
                Resolution::Found(part) => found.push(part),
                ambiguous @ Resolution::Ambiguous { .. } => return Ok(ambiguous),
                Resolution::NotFound => {}
            }
        }
        Ok(match found.len() {
            0 => Resolution::NotFound,
            1 => Resolution::Found(found.remove(0)),
            _ => {
                let mut repositories: Vec<String> =
                    found.into_iter().map(|p| p.repository_label).collect();
                repositories.sort();
                repositories.dedup();
                Resolution::Ambiguous { repositories }
            }
        })
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        self.visible()
            .find(|e| {
                e.sourced.repository == part.repository
                    && e.sourced.part.kind == part.part.kind
                    && e.sourced.part.identifier == part.part.identifier
                    && e.sourced.part.version == part.part.version
            })
            .map(|e| e.model.files.clone())
            .ok_or_else(|| PartSourceError::Corrupt {
                part_identifier: part.part.identifier.clone(),
            })
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        let in_scope: BTreeSet<usize> = self.steps(scope).into_iter().flatten().collect();
        let mut keys: Vec<(PartKind, String)> = Vec::new();
        for entry in self.visible().filter(|e| in_scope.contains(&e.repository)) {
            let kind = entry.sourced.part.kind;
            let family = kind_info(kind).family;
            let key = (kind, entry.sourced.part.identifier.clone());
            if matches!(family, KindFamily::Ingredient | KindFamily::Overlay)
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
        let parts = keys
            .into_iter()
            .filter_map(|(kind, id)| match self.lookup(scope, kind, &id, None) {
                Resolution::Found(part) => Some(part),
                _ => None,
            })
            .collect();
        Ok(sorted(parts))
    }

    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        let in_scope: Vec<AgentRepositoryId> = self
            .steps(scope)
            .into_iter()
            .flatten()
            .map(|i| match &self.repositories[i].0 {
                RepositoryRef::Repository(id) => *id,
                RepositoryRef::Bundled => BUNDLED_REPOSITORY_ID,
            })
            .collect();
        Ok(self
            .withdrawn
            .iter()
            .filter(|w| in_scope.contains(&w.agent_repository_id))
            .cloned()
            .collect())
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        Ok(sorted(
            self.visible()
                .filter(|e| e.sourced.repository == *repository)
                .map(|e| e.sourced.clone())
                .collect(),
        ))
    }

    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        let mut chosen: Vec<&ModelPart> = Vec::new();
        let mut axes: Vec<VocabularyAxis> = Vec::new();
        for repository in self.steps(scope).into_iter().flatten() {
            for entry in self
                .visible()
                .filter(|e| e.repository == repository && e.model.kind == PartKind::Vocabulary)
            {
                if let Ok(Some(body)) = entry.model.manifest.vocabulary() {
                    if !axes.contains(&body.axis) {
                        axes.push(body.axis);
                        chosen.push(&entry.model);
                    }
                }
            }
        }
        VocabularySet::from_parts(&chosen).map_err(|_| PartSourceError::Corrupt {
            part_identifier: chosen
                .last()
                .map(|p| p.manifest.metadata.id.clone())
                .unwrap_or_default(),
        })
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        let target = &part.part.identifier;
        Ok(sorted(
            self.visible()
                .filter(|e| e.sourced.part.kind == PartKind::WorkspaceProfile)
                .filter(|e| self.closure(&e.sourced.manifest).contains(target))
                .map(|e| e.sourced.clone())
                .collect(),
        ))
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        let Some(index) = self.index_of(repository) else {
            return Ok(Vec::new());
        };
        let Some(term) = self.terms.get(&(index, axis_str(axis), code.to_owned())) else {
            return Ok(Vec::new());
        };
        Ok(sorted(
            self.visible()
                .filter(|e| e.sourced.part.vocabulary_terms_cited.contains(term))
                .filter(|e| self.is_head(e))
                .map(|e| e.sourced.clone())
                .collect(),
        ))
    }
}

// ---- the contract ----

fn found(resolution: Resolution, rule: &str) -> SourcedPart {
    match resolution {
        Resolution::Found(part) => part,
        other => panic!("PartSource contract {rule}: expected a part, got {other:?}"),
    }
}

fn not_found(resolution: Resolution, rule: &str) {
    assert!(
        matches!(resolution, Resolution::NotFound),
        "PartSource contract {rule}: expected NotFound, got {resolution:?}"
    );
}

fn call<T>(result: Result<T, PartSourceError>, rule: &str) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("PartSource contract {rule}: the source failed with {error}"),
    }
}

fn identifiers(parts: &[SourcedPart]) -> Vec<(String, String)> {
    parts
        .iter()
        .map(|p| (p.part.identifier.clone(), p.part.version.clone()))
        .collect()
}

/// Runs every contract assertion against `source`, which the implementer has loaded with
/// `ContractFixture::standard()`. Panics with the failing rule's name.
/// Called by: Catalogue Service's test suite; Agent Model's own tests over InMemorySource.
pub async fn assert_part_source_contract(source: &dyn PartSource, fixture: &ContractFixture) {
    use ids::*;
    let repo = |i: usize| fixture.repositories[i].0.clone();
    let label = |i: usize| fixture.repositories[i].1.clone();
    let scope = SourceScope {
        home: repo(HOME),
        profile: vec![repo(PROFILE_A), repo(PROFILE_B)],
        include_bundled: true,
    };
    let resolve = |kind: PartKind, id: &'static str, version: Option<&'static str>| {
        source.resolve(&scope, kind, id, version)
    };

    let rule = "order_home_first";
    let part = found(
        call(resolve(PartKind::Rule, SHARED, None).await, rule),
        rule,
    );
    assert_eq!(part.repository, repo(HOME), "PartSource contract {rule}");

    let rule = "order_profile_listed";
    for (id, index) in [(POLICY_ONLY, PROFILE_A), (LEGAL_ONLY, PROFILE_B)] {
        let part = found(call(resolve(PartKind::Rule, id, None).await, rule), rule);
        assert_eq!(
            part.repository,
            repo(index),
            "PartSource contract {rule}: {id}"
        );
    }

    let rule = "order_bundled_last";
    let part = found(
        call(resolve(PartKind::Rule, BUNDLED_ONLY, None).await, rule),
        rule,
    );
    assert_eq!(part.repository, repo(BUNDLED), "PartSource contract {rule}");
    let without_bundled = SourceScope {
        include_bundled: false,
        ..scope.clone()
    };
    not_found(
        call(
            source
                .resolve(&without_bundled, PartKind::Rule, BUNDLED_ONLY, None)
                .await,
            rule,
        ),
        rule,
    );

    let rule = "ambiguous_same_step";
    match call(resolve(PartKind::Rule, TONE, None).await, rule) {
        Resolution::Ambiguous { mut repositories } => {
            repositories.sort();
            let mut expected = vec![label(PROFILE_A), label(PROFILE_B)];
            expected.sort();
            assert_eq!(repositories, expected, "PartSource contract {rule}");
        }
        other => panic!("PartSource contract {rule}: expected Ambiguous, got {other:?}"),
    }

    let rule = "added_repository_isolated";
    not_found(
        call(resolve(PartKind::Rule, PRIVATE, None).await, rule),
        rule,
    );
    let own = SourceScope {
        home: repo(ADDED),
        profile: vec![repo(PROFILE_A), repo(PROFILE_B)],
        include_bundled: true,
    };
    let part = found(
        call(
            source.resolve(&own, PartKind::Rule, PRIVATE, None).await,
            rule,
        ),
        rule,
    );
    assert_eq!(part.repository, repo(ADDED), "PartSource contract {rule}");

    let rule = "latest_non_draft";
    let summarise = found(
        call(resolve(PartKind::Skill, SUMMARISE, None).await, rule),
        rule,
    );
    assert_eq!(
        summarise.part.version, "1.1.0",
        "PartSource contract {rule}"
    );
    not_found(call(resolve(PartKind::Skill, WIP, None).await, rule), rule);

    let rule = "explicit_version";
    let draft = found(
        call(
            resolve(PartKind::Skill, SUMMARISE, Some("2.0.0")).await,
            rule,
        ),
        rule,
    );
    assert_eq!(draft.part.version, "2.0.0", "PartSource contract {rule}");
    not_found(
        call(
            resolve(PartKind::Skill, SUMMARISE, Some("9.9.9")).await,
            rule,
        ),
        rule,
    );

    let rule = "withdrawn_flagged";
    let old = found(call(resolve(PartKind::Skill, OLD, None).await, rule), rule);
    assert!(
        old.part.withdrawn,
        "PartSource contract {rule}: withdrawn part not flagged"
    );
    assert!(
        !summarise.part.withdrawn,
        "PartSource contract {rule}: part flagged wrongly"
    );

    let rule = "refused_not_found";
    not_found(
        call(resolve(PartKind::Skill, BROKEN, None).await, rule),
        rule,
    );
    not_found(
        call(source.resolve_any_kind(&scope, BROKEN).await, rule),
        rule,
    );
    let home_parts = call(source.repository_parts(&repo(HOME)).await, rule);
    assert!(
        home_parts.iter().all(|p| p.part.identifier != BROKEN),
        "PartSource contract {rule}: a refused part is listed"
    );

    let rule = "files_committed_bytes";
    let files = call(source.files(&summarise).await, rule);
    assert_eq!(
        content_digest(&files),
        summarise.part.content_digest,
        "PartSource contract {rule}: digest"
    );
    let committed = fixture.repositories[HOME]
        .2
        .iter()
        .find(|p| p.manifest.metadata.id == SUMMARISE && p.manifest.metadata.version == "1.1.0")
        .map(|p| p.files.clone());
    assert_eq!(Some(files), committed, "PartSource contract {rule}: bytes");

    let rule = "ingredients_latest";
    let ingredients = identifiers(&call(source.ingredients(&scope).await, rule));
    assert_eq!(
        ingredients,
        vec![
            (GO_PACK.to_owned(), "1.1.0".to_owned()),
            (PLAIN_STYLE.to_owned(), "1.0.0".to_owned())
        ],
        "PartSource contract {rule}"
    );

    let rule = "withdrawn_entries";
    let entries = call(source.withdrawn(&scope).await, rule);
    let withdrawn: Vec<&str> = entries.iter().map(|w| w.part_identifier.as_str()).collect();
    assert_eq!(withdrawn, vec![OLD], "PartSource contract {rule}");

    let rule = "used_by_reverse_closure";
    let users = identifiers(&call(source.used_by(&summarise).await, rule));
    assert_eq!(
        users,
        vec![(HELPER.to_owned(), "1.0.0".to_owned())],
        "PartSource contract {rule}"
    );

    let rule = "citing_parts_by_code";
    let citing = call(
        source
            .citing_parts(&repo(HOME), VocabularyAxis::Category, "finance")
            .await,
        rule,
    );
    assert_eq!(
        identifiers(&citing),
        vec![(TONE.to_owned(), "1.0.0".to_owned())],
        "PartSource contract {rule}"
    );
    assert_eq!(
        citing[0].repository,
        repo(PROFILE_A),
        "PartSource contract {rule}"
    );
    let vocabulary = call(source.vocabulary(&scope).await, rule);
    assert!(
        vocabulary.has(VocabularyAxis::Category, "finance"),
        "PartSource contract {rule}: the cited term is in the vocabulary"
    );

    let rule = "bundled_repository_id";
    let part = found(
        call(resolve(PartKind::Rule, BUNDLED_ONLY, None).await, rule),
        rule,
    );
    assert_eq!(
        part.part.agent_repository_id, BUNDLED_REPOSITORY_ID,
        "PartSource contract {rule}"
    );
    assert_eq!(
        part.repository_label, "bundled",
        "PartSource contract {rule}"
    );
    assert_eq!(
        part.repository_display_name, "ordin",
        "PartSource contract {rule}"
    );
}
