//! The library: search and facets (§G), requirement matching (§I), part reads (§J) and the
//! lookups for the Package Catalogue (§J2).

use std::collections::{BTreeMap, HashMap};

use ordin_agent_model::{all_kinds, slug_from_id, YamlValue, BUNDLED_REPOSITORY_ID};
use ordin_types::domain::agents::{PartKind, PartStatus};
use ordin_types::paging::{Cursor, Page, PAGE_SIZE};
use ordin_types::time::Timestamp;
use ordin_types::{AgentRepositoryId, PartId};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use crate::db::{
    all_repos, enum_text, latest_non_draft, part_by_id, part_from_row, sort_parts, uses_of,
    visible_versions, PartRecord, RepoRow, PART_SELECT,
};
use crate::error::CatalogueError;
use crate::git;
use crate::service::CatalogueService;
use crate::types::{
    Coverage, LibraryEntry, LibraryFacets, LibraryQuery, MadeOf, MatchRequirementsInput,
    PartDocument, PartHistory, PartStateLabel, PartVersionLine, PartView, RequirementMatch, UsedBy,
};

/// A requirement is covered at this normalised score or above (CatalogueService.md §I6; conditional
/// on C-63, R-76: the constants stay in this one place).
// spec: ordin-desktop/components/CatalogueService#C-63
// spec: ordin-desktop/components/CatalogueService#R-76
pub(crate) const COVERED_MIN: f64 = 0.60;
/// A requirement is partly covered from this score up to `COVERED_MIN`.
pub(crate) const PARTLY_MIN: f64 = 0.15;

const MAX_TEXT_CHARS: usize = 500;
/// The bundled coordinator's identifier, left out of the library (CAT-LIB-1); the same text as
/// `BundledAgent::Ordin.identifier()` in `ordin-package-catalogue`.
const BUNDLED_COORDINATOR_IDENTIFIER: &str = "urn:ordin:default:workspaces:coordinator";

/// What a scored search leaves out besides what the query filters: one part (the part a "related" read
/// starts from) and, for the library's `search` and `facets`, the bundled coordinator (CAT-LIB-1).
#[derive(Default)]
struct LeftOut {
    part: Option<PartId>,
    coordinator: bool,
}

impl LeftOut {
    fn coordinator() -> Self {
        Self {
            part: None,
            coordinator: true,
        }
    }

    fn part(part: Option<PartId>) -> Self {
        Self {
            part,
            coordinator: false,
        }
    }
}

/// The keyset a library listing continues from: rank, name, part id.
#[derive(Serialize, Deserialize)]
struct Key(f64, String, String);

enum Bind {
    Text(String),
    Real(f64),
}

fn state_label(record: &PartRecord) -> PartStateLabel {
    if !record.offered {
        PartStateLabel::NotOffered
    } else if record.part.withdrawn {
        PartStateLabel::Withdrawn
    } else if record.part.status == PartStatus::Deprecated {
        PartStateLabel::NotForNewAgents
    } else {
        PartStateLabel::Current
    }
}

/// `"a" "b"` for a text, each token quoted so no operator passes through (§G); `any` joins the
/// tokens with `OR` (requirement matching).
fn fts_expression(text: &str, any: bool) -> Option<String> {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|t| {
            let trimmed = t.trim_matches(|c: char| ".,;:()[]\"'".contains(c));
            if trimmed.starts_with("urn:ordin:") {
                slug_from_id(trimmed)
            } else {
                trimmed.to_owned()
            }
        })
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(if any { " OR " } else { " " }))
    }
}

fn yaml_json(value: &YamlValue) -> serde_json::Value {
    match value {
        YamlValue::Null => serde_json::Value::Null,
        YamlValue::Bool(b) => serde_json::Value::Bool(*b),
        YamlValue::Int(i) => serde_json::Value::from(*i),
        YamlValue::Decimal(text) => serde_json::json!({ "$decimal": text }),
        YamlValue::Str(text) => serde_json::Value::String(text.clone()),
        YamlValue::Seq(items) => serde_json::Value::Array(items.iter().map(yaml_json).collect()),
        YamlValue::Map(map) => {
            serde_json::Value::Object(map.iter().map(|(k, v)| (k.clone(), yaml_json(v))).collect())
        }
    }
}

struct Scored {
    record: PartRecord,
    rank: f64,
}

impl CatalogueService {
    async fn repo_map(&self) -> Result<HashMap<AgentRepositoryId, RepoRow>, CatalogueError> {
        Ok(all_repos(self.pool()?)
            .await?
            .into_iter()
            .map(|r| (r.id, r))
            .collect())
    }

    pub(crate) fn view_of(
        &self,
        record: &PartRecord,
        repos: &HashMap<AgentRepositoryId, RepoRow>,
    ) -> PartView {
        PartView {
            part: record.part.clone(),
            summary: record.summary.clone(),
            offered: record.offered,
            state_label: state_label(record),
            from: repos
                .get(&record.part.agent_repository_id)
                .map(RepoRow::shown_as)
                .unwrap_or_default(),
        }
    }

    /// The filtered, ranked rows of a library query, `limit` at most, continuing after `after`.
    // spec: ordin-desktop/components/CatalogueService#ADR-088
    async fn scored_search(
        &self,
        query: &LibraryQuery,
        kinds: &[PartKind],
        any_token: bool,
        after: Option<Key>,
        limit: i64,
        left_out: LeftOut,
    ) -> Result<Vec<Scored>, CatalogueError> {
        let LeftOut {
            part: exclude,
            coordinator: hide_coordinator,
        } = left_out;
        let mut binds: Vec<Bind> = Vec::new();
        let fts = query
            .text
            .as_deref()
            .and_then(|t| fts_expression(t, any_token));
        let (from, rank) = if let Some(expression) = &fts {
            binds.push(Bind::Text(expression.clone()));
            (
                "FROM part_search JOIN parts p ON p.id = part_search.part_id JOIN part_index i ON i.part_id = p.id",
                "bm25(part_search, 4.0, 2.0, 1.0)",
            )
        } else {
            ("FROM parts p JOIN part_index i ON i.part_id = p.id", "0.0")
        };
        let mut sql = format!(
            "SELECT * FROM (SELECT p.id, p.agent_repository_id, p.kind, p.identifier, p.name, p.version, \
             p.status, p.supersedes, p.owner, p.file_path, p.\"commit\" AS commit_sha, p.content_digest, \
             p.vocabulary_terms_cited, p.withdrawn, i.at_head, i.offered, i.not_offered_reason, i.summary, \
             i.area, i.audience, i.publisher, i.files, {rank} AS rank {from} WHERE "
        );
        if fts.is_some() {
            sql.push_str("part_search MATCH ? AND ");
        }
        sql.push_str("i.at_head = 1 AND i.offered = 1 AND p.withdrawn = 0");
        if !query.include_not_for_new {
            sql.push_str(" AND p.status != 'deprecated'");
        }
        sql.push_str(&format!(
            " AND p.kind IN ({})",
            vec!["?"; kinds.len().max(1)].join(",")
        ));
        if kinds.is_empty() {
            binds.push(Bind::Text(String::new()));
        }
        for kind in kinds {
            binds.push(Bind::Text(enum_text(kind)));
        }
        if !query.repositories.is_empty() {
            sql.push_str(&format!(
                " AND p.agent_repository_id IN ({})",
                vec!["?"; query.repositories.len()].join(",")
            ));
            for repository in &query.repositories {
                binds.push(Bind::Text(repository.to_string()));
            }
        }
        if let Some(area) = &query.area {
            sql.push_str(" AND i.area = ?");
            binds.push(Bind::Text(area.clone()));
        }
        if let Some(audience) = &query.audience {
            sql.push_str(" AND EXISTS (SELECT 1 FROM json_each(i.audience) WHERE value = ?)");
            binds.push(Bind::Text(audience.clone()));
        }
        if let Some(publisher) = &query.publisher {
            sql.push_str(" AND i.publisher = ?");
            binds.push(Bind::Text(publisher.clone()));
        }
        if let Some(exclude) = exclude {
            sql.push_str(" AND p.id != ?");
            binds.push(Bind::Text(exclude.to_string()));
        }
        if hide_coordinator {
            // spec: ordin-desktop/components/CatalogueService#CAT-LIB-1
            sql.push_str(" AND NOT (p.agent_repository_id = ? AND p.identifier = ?)");
            binds.push(Bind::Text(BUNDLED_REPOSITORY_ID.to_string()));
            binds.push(Bind::Text(BUNDLED_COORDINATOR_IDENTIFIER.to_owned()));
        }
        sql.push(')');
        if let Some(Key(rank, name, id)) = after {
            sql.push_str(" WHERE (rank, name, id) > (?, ?, ?)");
            binds.push(Bind::Real(rank));
            binds.push(Bind::Text(name));
            binds.push(Bind::Text(id));
        }
        sql.push_str(&format!(" ORDER BY rank, name, id LIMIT {limit}"));
        let mut statement = sqlx::query(sqlx::AssertSqlSafe(sql));
        for bind in binds {
            statement = match bind {
                Bind::Text(text) => statement.bind(text),
                Bind::Real(real) => statement.bind(real),
            };
        }
        let rows: Vec<SqliteRow> = statement.fetch_all(self.pool()?).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut record = part_from_row(row)?;
            record.part.uses = uses_of(self.pool()?, record.part.id).await?;
            out.push(Scored {
                rank: row.try_get("rank")?,
                record,
            });
        }
        Ok(out)
    }

    fn check_query(query: &LibraryQuery) -> Result<(), CatalogueError> {
        if query
            .text
            .as_deref()
            .is_some_and(|t| t.chars().count() > MAX_TEXT_CHARS)
        {
            return Err(CatalogueError::permanent(
                "CAT_QUERY_INVALID",
                "the search text is over 500 characters",
            ));
        }
        Ok(())
    }

    fn kinds_of(query: &LibraryQuery) -> Vec<PartKind> {
        if query.kinds.is_empty() {
            vec![PartKind::WorkspaceProfile]
        } else {
            query.kinds.clone()
        }
    }

    /// §G.
    pub async fn search(&self, query: LibraryQuery) -> Result<Page<LibraryEntry>, CatalogueError> {
        Self::check_query(&query)?;
        let after = match &query.cursor {
            Some(cursor) => Some(
                cursor
                    .decode::<Key>()
                    .map_err(|_| CatalogueError::bad_cursor())?,
            ),
            None => None,
        };
        let kinds = Self::kinds_of(&query);
        let mut rows = self
            .scored_search(
                &query,
                &kinds,
                false,
                after,
                PAGE_SIZE as i64 + 1,
                LeftOut::coordinator(),
            )
            .await?;
        let next_cursor = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|last| {
                Cursor::encode(&Key(
                    last.rank,
                    last.record.part.name.clone(),
                    last.record.part.id.to_string(),
                ))
            })
        } else {
            None
        };
        let repos = self.repo_map().await?;
        let items = rows
            .iter()
            .map(|s| {
                let view = self.view_of(&s.record, &repos);
                LibraryEntry {
                    from: view.from.clone(),
                    part: view,
                }
            })
            .collect();
        Ok(Page { items, next_cursor })
    }

    /// §G: counts over the same filtered set.
    pub async fn facets(&self, query: LibraryQuery) -> Result<LibraryFacets, CatalogueError> {
        Self::check_query(&query)?;
        let kinds = Self::kinds_of(&query);
        let query = LibraryQuery {
            cursor: None,
            ..query
        };
        let rows = self
            .scored_search(&query, &kinds, false, None, 100_000, LeftOut::coordinator())
            .await?;
        let mut facets = LibraryFacets::default();
        let mut area: BTreeMap<String, u32> = BTreeMap::new();
        let mut audience: BTreeMap<String, u32> = BTreeMap::new();
        let mut publisher: BTreeMap<String, u32> = BTreeMap::new();
        let mut repository: BTreeMap<AgentRepositoryId, u32> = BTreeMap::new();
        for s in &rows {
            if let Some(a) = &s.record.area {
                *area.entry(a.clone()).or_default() += 1;
            }
            for r in &s.record.audience {
                *audience.entry(r.clone()).or_default() += 1;
            }
            *publisher.entry(s.record.publisher.clone()).or_default() += 1;
            *repository
                .entry(s.record.part.agent_repository_id)
                .or_default() += 1;
        }
        facets.area = area.into_iter().collect();
        facets.audience = audience.into_iter().collect();
        facets.publisher = publisher.into_iter().collect();
        facets.repository = repository.into_iter().collect();
        Ok(facets)
    }

    /// §I.
    pub async fn match_requirements(
        &self,
        input: MatchRequirementsInput,
    ) -> Result<Vec<RequirementMatch>, CatalogueError> {
        let started = std::time::Instant::now();
        let requirements = input.requirements;
        if requirements.is_empty()
            || requirements.len() > 50
            || requirements.iter().any(|r| r.chars().count() > 2000)
        {
            return Err(CatalogueError::permanent(
                "CAT_REQUIREMENTS_INVALID",
                "1-50 requirements of at most 2,000 characters",
            ));
        }
        let repos = self.repo_map().await?;
        let kinds: Vec<PartKind> = all_kinds().to_vec();
        let mut out = Vec::with_capacity(requirements.len());
        for requirement in requirements {
            let collapsed = requirement.split_whitespace().collect::<Vec<_>>().join(" ");
            if collapsed.is_empty() {
                out.push(RequirementMatch {
                    requirement,
                    coverage: Coverage::NotCovered,
                    matched: Vec::new(),
                    shortfall: None,
                });
                continue;
            }
            let query = LibraryQuery {
                text: Some(collapsed.clone()),
                ..LibraryQuery::default()
            };
            let results = self
                .scored_search(&query, &kinds, true, None, 20, LeftOut::default())
                .await
                .map_err(|e| CatalogueError::store(e.detail))?;
            // The identity arm: an identifier named in the text.
            let slugs: Vec<String> = collapsed
                .split_whitespace()
                .map(|t| t.trim_matches(|c: char| ".,;:()[]\"'".contains(c)))
                .filter(|t| !t.is_empty())
                .map(|t| {
                    if t.starts_with("urn:ordin:") {
                        slug_from_id(t)
                    } else {
                        t.to_owned()
                    }
                })
                .collect();
            let identified = results.iter().find(|s| {
                slugs
                    .iter()
                    .any(|slug| *slug == slug_from_id(&s.record.part.identifier))
            });
            if let Some(found) = identified {
                let mut matched = vec![self.view_of(&found.record, &repos)];
                matched.extend(
                    results
                        .iter()
                        .filter(|s| s.record.part.id != found.record.part.id)
                        .take(2)
                        .map(|s| self.view_of(&s.record, &repos)),
                );
                out.push(RequirementMatch {
                    requirement,
                    coverage: Coverage::Covered,
                    matched,
                    shortfall: None,
                });
                continue;
            }
            let score = |s: &Scored| {
                let r = -s.rank;
                if r <= 0.0 {
                    0.0
                } else {
                    r / (1.0 + r)
                }
            };
            let best = results.iter().map(score).fold(0.0_f64, f64::max);
            let matched: Vec<PartView> = results
                .iter()
                .take(3)
                .map(|s| self.view_of(&s.record, &repos))
                .collect();
            let (coverage, shortfall) = if best >= COVERED_MIN {
                (Coverage::Covered, None)
            } else if best >= PARTLY_MIN {
                let name = matched
                    .first()
                    .map(|m| m.part.name.clone())
                    .unwrap_or_default();
                (
                    Coverage::PartlyCovered,
                    Some(format!(
                        "{name} matches part of this requirement; it does not cover: {collapsed}"
                    )),
                )
            } else {
                (Coverage::NotCovered, None)
            };
            let matched = if coverage == Coverage::NotCovered {
                Vec::new()
            } else {
                matched
            };
            out.push(RequirementMatch {
                requirement,
                coverage,
                matched,
                shortfall,
            });
        }
        let count = |c: Coverage| out.iter().filter(|m| m.coverage == c).count();
        tracing::info!(
            component = "catalogue",
            requirements = out.len(),
            covered = count(Coverage::Covered),
            partly = count(Coverage::PartlyCovered),
            not_covered = count(Coverage::NotCovered),
            duration_ms = started.elapsed().as_millis() as u64,
            "catalogue.match"
        );
        Ok(out)
    }

    /// Look-alikes for a text within one kind (§H).
    pub async fn similar_parts(
        &self,
        kind: PartKind,
        text: &str,
        exclude: Option<PartId>,
    ) -> Result<Vec<PartView>, CatalogueError> {
        let query = LibraryQuery {
            text: Some(text.to_owned()),
            include_not_for_new: true,
            ..LibraryQuery::default()
        };
        let rows = self
            .scored_search(&query, &[kind], true, None, 6, LeftOut::part(exclude))
            .await?;
        let repos = self.repo_map().await?;
        Ok(rows
            .iter()
            .filter(|s| {
                let r = -s.rank;
                r > 0.0 && r / (1.0 + r) >= PARTLY_MIN
            })
            .map(|s| self.view_of(&s.record, &repos))
            .collect())
    }

    async fn record(&self, id: PartId) -> Result<PartRecord, CatalogueError> {
        part_by_id(self.pool()?, id)
            .await?
            .ok_or_else(CatalogueError::part_unknown)
    }

    pub async fn part(&self, id: PartId) -> Result<PartView, CatalogueError> {
        let record = self.record(id).await?;
        Ok(self.view_of(&record, &self.repo_map().await?))
    }

    /// §J: the Agent Model fields by kind and the file text.
    pub async fn read_part(&self, id: PartId) -> Result<PartDocument, CatalogueError> {
        let record = self.record(id).await?;
        let repos = self.repo_map().await?;
        let row = repos
            .get(&record.part.agent_repository_id)
            .ok_or_else(CatalogueError::repository_unknown)?;
        let bytes = self
            .read_file(row, &record.part.commit, &record.part.file_path)
            .await
            .map_err(CatalogueError::store)?
            .ok_or_else(CatalogueError::part_unknown)?;
        let manifest = ordin_agent_model::parse_manifest(&record.part.file_path, &bytes)
            .ok()
            .flatten()
            .ok_or_else(|| CatalogueError::store("a stored part's file does not parse"))?;
        let body = serde_json::Value::Object(
            manifest
                .body
                .iter()
                .map(|(k, v)| (k.clone(), yaml_json(v)))
                .collect(),
        );
        let clipped = &bytes[..bytes.len().min(100 * 1024)];
        Ok(PartDocument {
            view: self.view_of(&record, &repos),
            body,
            file_text: String::from_utf8_lossy(clipped).into_owned(),
        })
    }

    pub async fn used_by(&self, id: PartId) -> Result<UsedBy, CatalogueError> {
        self.record(id).await?;
        let repos = self.repo_map().await?;
        let mut agents = Vec::new();
        let mut parts = Vec::new();
        for (record, _) in self.reverse_closure(id).await? {
            let view = self.view_of(&record, &repos);
            if record.part.kind == PartKind::WorkspaceProfile {
                agents.push(view);
            } else {
                parts.push(view);
            }
        }
        Ok(UsedBy {
            part_id: id,
            agents,
            parts,
        })
    }

    /// The forward closure of a part grouped by kind, in the kinds' declaration order.
    pub async fn made_of(&self, agent: PartId) -> Result<MadeOf, CatalogueError> {
        let root = self.record(agent).await?;
        let sql = format!(
            "WITH RECURSIVE down(id, depth) AS (\
               SELECT uses_part_id, 1 FROM part_dependencies WHERE part_id = ?1 \
               UNION \
               SELECT d.uses_part_id, down.depth + 1 FROM part_dependencies d JOIN down ON d.part_id = down.id \
               WHERE down.depth < 32) \
             {PART_SELECT} WHERE p.id IN (SELECT id FROM down)"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(agent.to_string())
            .fetch_all(self.pool()?)
            .await?;
        let repos = self.repo_map().await?;
        let mut records: Vec<PartRecord> =
            rows.iter().map(part_from_row).collect::<Result<_, _>>()?;
        sort_parts(&mut records);
        let mut groups: BTreeMap<PartKind, Vec<PartView>> = BTreeMap::new();
        for record in &records {
            groups
                .entry(record.part.kind)
                .or_default()
                .push(self.view_of(record, &repos));
        }
        Ok(MadeOf {
            agent: self.view_of(&root, &repos),
            groups: groups.into_iter().collect(),
        })
    }

    pub async fn part_history(&self, id: PartId) -> Result<PartHistory, CatalogueError> {
        let record = self.record(id).await?;
        let repos = self.repo_map().await?;
        let row = repos
            .get(&record.part.agent_repository_id)
            .ok_or_else(CatalogueError::repository_unknown)?;
        let sql = format!(
            "{PART_SELECT} WHERE p.agent_repository_id = ?1 AND p.kind = ?2 AND p.identifier = ?3"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(row.id.to_string())
            .bind(enum_text(&record.part.kind))
            .bind(&record.part.identifier)
            .fetch_all(self.pool()?)
            .await?;
        let mut versions: Vec<PartRecord> =
            rows.iter().map(part_from_row).collect::<Result<_, _>>()?;
        versions.sort_by(|a, b| crate::db::version_order(&a.part.version, &b.part.version));
        let mut lines = Vec::new();
        for v in &versions {
            let at = if row.is_bundled() {
                Timestamp::now()
            } else {
                let clone = std::path::PathBuf::from(&row.clone_path);
                let commit = v.part.commit.clone();
                tokio::task::spawn_blocking(move || git::commit_time_millis(&clone, &commit))
                    .await
                    .ok()
                    .flatten()
                    .and_then(|ms| Timestamp::from_unix_millis(ms).ok())
                    .unwrap_or_else(Timestamp::now)
            };
            lines.push(PartVersionLine {
                version: v.part.version.clone(),
                commit: v.part.commit.clone(),
                merged_at: at,
            });
        }
        // The `supersedes` chain, both ways, within the repository and kind.
        let mut replaces = Vec::new();
        let mut seen = vec![record.part.identifier.clone()];
        let mut cursor = record.part.supersedes.clone();
        while let Some(target) = cursor.take() {
            if seen.contains(&target) {
                break;
            }
            seen.push(target.clone());
            let found = latest_non_draft(
                visible_versions(self.pool()?, row.id, Some(record.part.kind), &target).await?,
            );
            if let Some(found) = found {
                cursor = found.part.supersedes.clone();
                replaces.push(self.view_of(&found, &repos));
            }
        }
        let mut replaced_by = Vec::new();
        let mut frontier = vec![record.part.identifier.clone()];
        let mut seen = vec![record.part.identifier.clone()];
        while let Some(current) = frontier.pop() {
            let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
                "{PART_SELECT} WHERE p.agent_repository_id = ?1 AND p.kind = ?2 AND p.supersedes = ?3 AND i.at_head = 1"
            )))
            .bind(row.id.to_string())
            .bind(enum_text(&record.part.kind))
            .bind(&current)
            .fetch_all(self.pool()?)
            .await?;
            for r in &rows {
                let next = part_from_row(r)?;
                if !seen.contains(&next.part.identifier) {
                    seen.push(next.part.identifier.clone());
                    frontier.push(next.part.identifier.clone());
                    replaced_by.push(self.view_of(&next, &repos));
                }
            }
        }
        Ok(PartHistory {
            versions: lines,
            replaces,
            replaced_by,
        })
    }

    /// §J2.
    pub async fn find_part(
        &self,
        repository: AgentRepositoryId,
        kind: PartKind,
        identifier: &str,
        version: Option<&str>,
    ) -> Result<PartView, CatalogueError> {
        let row = self.repo(repository).await?;
        let versions = visible_versions(self.pool()?, row.id, Some(kind), identifier).await?;
        // `find_part` returns the part with its `offered` flag: the caller decides what a not-offered
        // part means, so the index is read without the `offered` filter.
        let sql = format!(
            "{PART_SELECT} WHERE p.agent_repository_id = ?1 AND p.kind = ?2 AND p.identifier = ?3 AND i.at_head = 1"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(row.id.to_string())
            .bind(enum_text(&kind))
            .bind(identifier)
            .fetch_all(self.pool()?)
            .await?;
        let _ = versions;
        let mut all: Vec<PartRecord> = rows.iter().map(part_from_row).collect::<Result<_, _>>()?;
        let chosen = match version {
            Some(version) => all.drain(..).find(|r| r.part.version == version),
            None => all
                .into_iter()
                .max_by(|a, b| crate::db::version_order(&a.part.version, &b.part.version)),
        };
        let mut record = chosen.ok_or_else(CatalogueError::part_unknown)?;
        record.part.uses = uses_of(self.pool()?, record.part.id).await?;
        let mut repos = HashMap::new();
        repos.insert(row.id, row);
        Ok(self.view_of(&record, &repos))
    }
}
