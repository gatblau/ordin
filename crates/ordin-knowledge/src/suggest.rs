//! Suggestions (amendment January 2026, ADR-104 §U): link suggestions where a page names another
//! without linking it (SA-KLINK-08), likely duplicates (SA-KLINK-07). In application code, with
//! no model; nothing changes until the person accepts.

use std::collections::HashSet;
use std::ops::Range;

use ordin_files::FileVersionOrigin;
use ordin_types::domain::places::KnowledgePageState;
use ordin_types::time::Timestamp;
use ordin_types::{KnowledgePageId, KnowledgeSuggestionId};
use serde_json::json;
use sqlx::Row;

use crate::error::KnowledgeError;
use crate::markdown::{
    code_ranges, fts_any_word, fts_phrase, heading_of, lines_with_offsets, scan_links, tokens,
};
use crate::model::{DuplicateReason, PageRef, SuggestionKind, SuggestionView};
use crate::names::{below_knowledge, folded_key, name_key};
use crate::okf;
use crate::pagefile::sha256_hex;
use crate::pages::Commit;
use crate::service::{KnowledgeService, MAX_LINK_SUGGESTIONS};
use crate::store::{self, Conn, PageRow};

const CONTEXT_CHARS: usize = 60;
const SHINGLE: usize = 5;
const MIN_WORDS: usize = 50;
const JACCARD: f64 = 0.6;

/// One searchable character of a body: lower-cased, its byte range; white-space runs are one ' '.
struct Stream {
    chars: Vec<(char, usize, usize)>,
}

fn excluded(body: &str) -> Vec<Range<usize>> {
    let mut ranges = code_ranges(body);
    for (start, line) in lines_with_offsets(body) {
        if heading_of(line).is_some() {
            ranges.push(start..start + line.len());
        }
    }
    for l in scan_links(body) {
        ranges.push(l.span);
    }
    ranges
}

fn stream(body: &str, skip: &[Range<usize>]) -> Stream {
    let mut chars: Vec<(char, usize, usize)> = Vec::new();
    for (i, c) in body.char_indices() {
        let end = i + c.len_utf8();
        if skip.iter().any(|r| r.contains(&i)) {
            if chars.last().is_some_and(|(x, _, _)| *x != '\u{0}') {
                chars.push(('\u{0}', i, end));
            }
            continue;
        }
        if c.is_whitespace() {
            if chars.last().is_some_and(|(x, _, _)| *x == ' ') {
                if let Some(last) = chars.last_mut() {
                    last.2 = end;
                }
                continue;
            }
            chars.push((' ', i, end));
            continue;
        }
        let lower = c.to_lowercase().next().unwrap_or(c);
        chars.push((lower, i, end));
    }
    Stream { chars }
}

/// The first whole-word occurrence of a name key in the stream: its byte range in the body.
fn first_occurrence(s: &Stream, key: &[char]) -> Option<Range<usize>> {
    let n = key.len();
    if n == 0 || s.chars.len() < n {
        return None;
    }
    'outer: for i in 0..=s.chars.len() - n {
        if i > 0 && s.chars[i - 1].0.is_alphanumeric() {
            continue;
        }
        for (j, k) in key.iter().enumerate() {
            if s.chars[i + j].0 != *k {
                continue 'outer;
            }
        }
        if s.chars
            .get(i + n)
            .is_some_and(|(c, _, _)| c.is_alphanumeric())
        {
            continue;
        }
        return Some(s.chars[i].1..s.chars[i + n - 1].2);
    }
    None
}

fn context(body: &str, at: &Range<usize>) -> String {
    let before: String = body[..at.start]
        .chars()
        .rev()
        .take(CONTEXT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = body[at.end..].chars().take(CONTEXT_CHARS).collect();
    crate::markdown::plain_text(&format!("{before}{}{after}", &body[at.clone()]))
}

fn shingles(body: &str) -> (usize, HashSet<String>) {
    let words = tokens(body);
    let mut set = HashSet::new();
    if words.len() >= SHINGLE {
        for w in words.windows(SHINGLE) {
            set.insert(w.join(" "));
        }
    }
    (words.len(), set)
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    inter / union
}

fn kind_of(raw: &str) -> SuggestionKind {
    if raw == "duplicate" {
        SuggestionKind::Duplicate
    } else {
        SuggestionKind::Link
    }
}

/// The page's suggestions not dismissed (T-1).
pub(crate) async fn suggestions_of(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<Vec<SuggestionView>, KnowledgeError> {
    let rows = sqlx::query(
        "SELECT s.id, s.kind, s.other_page_id, p.title AS other_title, s.matched_text, s.context, s.reason \
         FROM knowledge_suggestions s JOIN knowledge_pages p ON p.id = s.other_page_id AND p.state <> 'removed' \
         WHERE s.page_id = ?1 ORDER BY s.kind DESC, s.start_byte, s.created_at",
    )
    .bind(page.to_string())
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let id: String = r.try_get("id")?;
            let other: String = r.try_get("other_page_id")?;
            let reason: Option<String> = r.try_get("reason")?;
            Ok(SuggestionView {
                id: KnowledgeSuggestionId::parse(&id).map_err(|_| store::corrupt())?,
                kind: kind_of(&r.try_get::<String, _>("kind")?),
                other: PageRef {
                    page_id: KnowledgePageId::parse(&other).map_err(|_| store::corrupt())?,
                    title: r.try_get("other_title")?,
                },
                matched_text: r.try_get("matched_text")?,
                context: r.try_get("context")?,
                reason: reason.map(|r| {
                    if r == "text" {
                        DuplicateReason::Text
                    } else {
                        DuplicateReason::Names
                    }
                }),
            })
        })
        .collect()
}

/// E-1 / V: suggestions on a removed page and naming it.
pub(crate) async fn delete_for_page(
    conn: &mut Conn,
    page: KnowledgePageId,
) -> Result<(), KnowledgeError> {
    sqlx::query("DELETE FROM knowledge_suggestions WHERE page_id = ?1 OR other_page_id = ?1")
        .bind(page.to_string())
        .execute(&mut *conn)
        .await?;
    Ok(())
}

async fn dismissed(
    conn: &mut Conn,
    page: KnowledgePageId,
    kind: &str,
    other: KnowledgePageId,
    key: &str,
) -> Result<bool, KnowledgeError> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM knowledge_suggestion_dismissals WHERE page_id = ?1 AND kind = ?2 \
         AND other_page_id = ?3 AND matched_key = ?4",
    )
    .bind(page.to_string())
    .bind(kind)
    .bind(other.to_string())
    .bind(key)
    .fetch_one(&mut *conn)
    .await?;
    Ok(n > 0)
}

/// The names of the workspace's other live pages: (page, title, name text, key).
async fn other_names(
    conn: &mut Conn,
    row: &PageRow,
) -> Result<Vec<(KnowledgePageId, String, String, String)>, KnowledgeError> {
    let rows = sqlx::query(
        "SELECT p.id, p.title, p.aliases, n.name_key, n.kind FROM knowledge_page_names n \
         JOIN knowledge_pages p ON p.id = n.page_id WHERE n.workspace_id = ?1 AND p.id <> ?2 AND p.state <> 'removed'",
    )
    .bind(row.workspace_id.to_string())
    .bind(row.id.to_string())
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        let id: String = r.try_get("id")?;
        let title: String = r.try_get("title")?;
        let key: String = r.try_get("name_key")?;
        let aliases: Vec<String> =
            serde_json::from_str(&r.try_get::<String, _>("aliases")?).unwrap_or_default();
        let text = if r.try_get::<String, _>("kind")? == "title" {
            title.clone()
        } else {
            aliases
                .iter()
                .find(|a| name_key(a) == key)
                .cloned()
                .unwrap_or_else(|| key.clone())
        };
        out.push((
            KnowledgePageId::parse(&id).map_err(|_| store::corrupt())?,
            title,
            text,
            key,
        ));
    }
    Ok(out)
}

impl KnowledgeService {
    /// U-1 for one page: its link suggestions, replaced.
    async fn link_suggestions(&self, conn: &mut Conn, row: &PageRow) -> Result<(), KnowledgeError> {
        sqlx::query("DELETE FROM knowledge_suggestions WHERE page_id = ?1 AND kind = 'link'")
            .bind(row.id.to_string())
            .execute(&mut *conn)
            .await?;
        if row.state == KnowledgePageState::Removed {
            return Ok(());
        }
        let linked: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT target_page_id FROM knowledge_links WHERE page_id = ?1 AND target_page_id IS NOT NULL",
        )
        .bind(row.id.to_string())
        .fetch_all(&mut *conn)
        .await?;
        let mut names = other_names(conn, row).await?;
        names.retain(|(id, _, _, key)| {
            key.chars().count() >= 4 && !linked.contains(&id.to_string())
        });
        // Longest names first.
        names.sort_by(|a, b| {
            b.3.chars()
                .count()
                .cmp(&a.3.chars().count())
                .then_with(|| a.3.cmp(&b.3))
        });
        let skip = excluded(&row.body);
        let s = stream(&row.body, &skip);
        let body_hash = sha256_hex(row.body.as_bytes());
        let mut used: Vec<Range<usize>> = Vec::new();
        let mut made = 0usize;
        let now = Timestamp::now().to_wire();
        for (other, _title, _text, key) in names {
            if made >= MAX_LINK_SUGGESTIONS {
                break;
            }
            let key_chars: Vec<char> = key.chars().collect();
            let Some(at) = first_occurrence(&s, &key_chars) else {
                continue;
            };
            if used.iter().any(|u| u.start < at.end && at.start < u.end) {
                continue;
            }
            if dismissed(conn, row.id, "link", other, &key).await? {
                continue;
            }
            let done = sqlx::query(
                "INSERT OR IGNORE INTO knowledge_suggestions (id, page_id, kind, other_page_id, matched_key, matched_text, \
                 context, start_byte, end_byte, body_hash, reason, created_at) \
                 VALUES (?1, ?2, 'link', ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)",
            )
            .bind(KnowledgeSuggestionId::new().to_string())
            .bind(row.id.to_string())
            .bind(other.to_string())
            .bind(&key)
            .bind(&row.body[at.clone()])
            .bind(context(&row.body, &at))
            .bind(i64::try_from(at.start).unwrap_or(0))
            .bind(i64::try_from(at.end).unwrap_or(0))
            .bind(&body_hash)
            .bind(&now)
            .execute(&mut *conn)
            .await?;
            if done.rows_affected() > 0 {
                used.push(at);
                made += 1;
            }
        }
        Ok(())
    }

    /// U-2: duplicate suggestions of one page against the workspace's pages, stored on both.
    async fn duplicate_suggestions(
        &self,
        conn: &mut Conn,
        row: &PageRow,
    ) -> Result<(), KnowledgeError> {
        sqlx::query(
            "DELETE FROM knowledge_suggestions WHERE kind = 'duplicate' AND (page_id = ?1 OR other_page_id = ?1)",
        )
        .bind(row.id.to_string())
        .execute(&mut *conn)
        .await?;
        if row.state == KnowledgePageState::Removed {
            return Ok(());
        }
        let mine: HashSet<String> = std::iter::once(&row.title)
            .chain(row.aliases.iter())
            .map(|n| folded_key(n))
            .filter(|k| !k.is_empty())
            .collect();
        let mut found: Vec<(KnowledgePageId, &'static str)> = Vec::new();
        let others = store::workspace_pages(conn, row.workspace_id).await?;
        for o in others.iter().filter(|o| o.id != row.id) {
            let theirs = std::iter::once(&o.title)
                .chain(o.aliases.iter())
                .map(|n| folded_key(n));
            if theirs.into_iter().any(|k| mine.contains(&k)) {
                found.push((o.id, "names"));
            }
        }
        let (words, mine_sh) = shingles(&row.body);
        if words >= MIN_WORDS {
            let first: Vec<String> = tokens(&row.body).into_iter().take(200).collect();
            let query = fts_any_word(&format!("{} {}", row.title, first.join(" ")));
            if let Some(q) = query {
                let ids: Vec<String> = sqlx::query_scalar(
                    "SELECT p.id FROM knowledge_pages_fts JOIN knowledge_pages p ON p.rowid = knowledge_pages_fts.rowid \
                     WHERE knowledge_pages_fts MATCH ?1 AND p.workspace_id = ?2 AND p.id <> ?3 AND p.state <> 'removed' \
                     ORDER BY bm25(knowledge_pages_fts, 2.0, 1.0) LIMIT 5",
                )
                .bind(q)
                .bind(row.workspace_id.to_string())
                .bind(row.id.to_string())
                .fetch_all(&mut *conn)
                .await?;
                for id in ids {
                    let Some(o) = others.iter().find(|o| o.id.to_string() == id) else {
                        continue;
                    };
                    if found.iter().any(|(f, _)| *f == o.id) {
                        continue;
                    }
                    let (w, sh) = shingles(&o.body);
                    if w >= MIN_WORDS && jaccard(&mine_sh, &sh) >= JACCARD {
                        found.push((o.id, "text"));
                    }
                }
            }
        }
        let now = Timestamp::now().to_wire();
        for (other, reason) in found {
            if dismissed(conn, row.id, "duplicate", other, "").await?
                || dismissed(conn, other, "duplicate", row.id, "").await?
            {
                continue;
            }
            for (a, b) in [(row.id, other), (other, row.id)] {
                sqlx::query(
                    "INSERT OR IGNORE INTO knowledge_suggestions (id, page_id, kind, other_page_id, matched_key, reason, created_at) \
                     VALUES (?1, ?2, 'duplicate', ?3, '', ?4, ?5)",
                )
                .bind(KnowledgeSuggestionId::new().to_string())
                .bind(a.to_string())
                .bind(b.to_string())
                .bind(reason)
                .bind(&now)
                .execute(&mut *conn)
                .await?;
            }
        }
        Ok(())
    }

    /// U-1/U-2 after a body or name change: the page itself, and every page whose text matches
    /// one of its names (a phrase query).
    pub(crate) async fn refresh_suggestions(&self, row: &PageRow) -> Result<(), KnowledgeError> {
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let Some(current) = store::page(&mut tx, row.id).await? else {
            return Ok(());
        };
        self.link_suggestions(&mut tx, &current).await?;
        self.duplicate_suggestions(&mut tx, &current).await?;
        let mut others: HashSet<String> = HashSet::new();
        for name in std::iter::once(&current.title).chain(current.aliases.iter()) {
            let Some(q) = fts_phrase(name) else { continue };
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT p.id FROM knowledge_pages_fts JOIN knowledge_pages p ON p.rowid = knowledge_pages_fts.rowid \
                 WHERE knowledge_pages_fts MATCH ?1 AND p.workspace_id = ?2 AND p.id <> ?3 AND p.state <> 'removed'",
            )
            .bind(q)
            .bind(current.workspace_id.to_string())
            .bind(current.id.to_string())
            .fetch_all(&mut *tx)
            .await?;
            others.extend(ids);
        }
        for id in others {
            let Ok(pid) = KnowledgePageId::parse(&id) else {
                continue;
            };
            if let Some(o) = store::page(&mut tx, pid).await? {
                self.link_suggestions(&mut tx, &o).await?;
            }
        }
        tx.commit().await?;
        Ok(())
    }

    /// U-3.
    pub async fn accept_suggestion(
        &self,
        page_id: KnowledgePageId,
        id: KnowledgeSuggestionId,
    ) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let mut conn = self.conn().await?;
        let old = self.own_row(&mut conn, page_id).await?;
        let row = sqlx::query(
            "SELECT kind, other_page_id, start_byte, end_byte, body_hash FROM knowledge_suggestions WHERE id = ?1 AND page_id = ?2",
        )
        .bind(id.to_string())
        .bind(page_id.to_string())
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(KnowledgeError::SuggestionNotFound)?;
        if row.try_get::<String, _>("kind")? != "link" {
            return Err(KnowledgeError::SuggestionNotFound);
        }
        let hash: Option<String> = row.try_get("body_hash")?;
        if hash.as_deref() != Some(sha256_hex(old.body.as_bytes()).as_str()) {
            self.link_suggestions(&mut conn, &old).await?;
            return Err(KnowledgeError::SuggestionStale);
        }
        let other: String = row.try_get("other_page_id")?;
        let other = store::page(
            &mut conn,
            KnowledgePageId::parse(&other).map_err(|_| store::corrupt())?,
        )
        .await?
        .filter(|o| o.state != KnowledgePageState::Removed)
        .ok_or(KnowledgeError::SuggestionNotFound)?;
        let start = usize::try_from(row.try_get::<i64, _>("start_byte")?).unwrap_or(0);
        let end = usize::try_from(row.try_get::<i64, _>("end_byte")?).unwrap_or(0);
        let occurrence = old
            .body
            .get(start..end)
            .ok_or(KnowledgeError::SuggestionStale)?
            .to_owned();
        drop(conn);
        let mut new = old.clone();
        // ADR-112: links ordin writes are OKF links.
        let link = okf::link_to(&below_knowledge(&other.relative_path), None, &occurrence);
        new.body.replace_range(start..end, &link);
        new.last_changed_at = Timestamp::now();
        new.edited_by_person = true;
        self.commit_page(Commit {
            row: new,
            old: Some(old.clone()),
            origin: FileVersionOrigin::Person,
            expected_hash: None,
            verify: false,
            run_id: None,
            type_set: false,
        })
        .await?;
        self.journal(
            "suggestion_accepted",
            Some(old.workspace_id),
            None,
            "A link suggestion was accepted.",
            json!({ "page_id": page_id }),
        )
        .await;
        Ok(())
    }

    /// U-4.
    pub async fn dismiss_suggestion(
        &self,
        page_id: KnowledgePageId,
        id: KnowledgeSuggestionId,
    ) -> Result<(), KnowledgeError> {
        self.desktop_only()?;
        let _guard = self.write_lock.lock().await;
        let pool = self.pool()?;
        let mut tx = pool.begin().await?;
        let row = sqlx::query(
            "SELECT kind, other_page_id, matched_key FROM knowledge_suggestions WHERE id = ?1 AND page_id = ?2",
        )
        .bind(id.to_string())
        .bind(page_id.to_string())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(KnowledgeError::SuggestionNotFound)?;
        let kind: String = row.try_get("kind")?;
        let other: String = row.try_get("other_page_id")?;
        let key: String = row.try_get("matched_key")?;
        let now = Timestamp::now().to_wire();
        let pairs: Vec<(String, String)> = if kind == "duplicate" {
            vec![
                (page_id.to_string(), other.clone()),
                (other.clone(), page_id.to_string()),
            ]
        } else {
            vec![(page_id.to_string(), other.clone())]
        };
        for (a, b) in pairs {
            sqlx::query(
                "INSERT OR IGNORE INTO knowledge_suggestion_dismissals (page_id, kind, other_page_id, matched_key, dismissed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(&a)
            .bind(&kind)
            .bind(&b)
            .bind(&key)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "DELETE FROM knowledge_suggestions WHERE page_id = ?1 AND kind = ?2 AND other_page_id = ?3 AND matched_key = ?4",
            )
            .bind(&a)
            .bind(&kind)
            .bind(&b)
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        }
        let workspace: String =
            sqlx::query_scalar("SELECT workspace_id FROM knowledge_pages WHERE id = ?1")
                .bind(page_id.to_string())
                .fetch_one(&mut *tx)
                .await?;
        tx.commit().await?;
        self.journal(
            "suggestion_dismissed",
            ordin_types::WorkspaceId::parse(&workspace).ok(),
            None,
            "A suggestion was dismissed.",
            json!({ "page_id": page_id, "kind": kind }),
        )
        .await;
        Ok(())
    }
}
