//! Search over names and document text (FileService.md Internal Logic 8).

use std::collections::HashSet;

use ordin_types::PlaceId;
use sqlx::Row;

use crate::error::FileError;
use crate::service::{ts, FileService};
use crate::types::{SearchHit, SearchResults};
use crate::SNIPPET_CHARS;

/// Each word of the query as a quoted string token, the last with `*` appended (prefix match);
/// no FTS5 syntax from the person reaches the engine (Internal Logic 8.1).
pub(crate) fn fts_query(query: &str) -> String {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect();
    let mut joined = words.join(" ");
    if !joined.is_empty() {
        joined.push('*');
    }
    joined
}

impl FileService {
    /// `query` 1–200 chars; `limit` 1–200. Name matches first, then text matches by FTS5 rank.
    pub async fn search(
        &self,
        place_ids: &[PlaceId],
        query: &str,
        limit: u32,
    ) -> Result<SearchResults, FileError> {
        let query = query.trim();
        if query.is_empty() || query.chars().count() > 200 || !(1..=200).contains(&limit) {
            return Err(FileError::QueryInvalid);
        }
        let mut ids: Vec<String> = Vec::new();
        for id in place_ids {
            let text = id.to_string();
            if !ids.contains(&text) {
                ids.push(text);
            }
        }
        let coverage = self.coverage(place_ids).await?;
        if ids.is_empty() {
            return Ok(SearchResults {
                hits: Vec::new(),
                coverage,
            });
        }
        let marks = vec!["?"; ids.len()].join(",");
        let limit = i64::from(limit);

        // 8.2: name matches, newest change first. `instr` keeps `%` and `_` in a query literal.
        let sql = format!(
            "SELECT place_id, path, changed_at FROM file_index \
             WHERE is_folder = 0 AND place_id IN ({marks}) AND instr(lower(name), lower(?)) > 0 \
             ORDER BY changed_at DESC, path LIMIT ?"
        );
        let mut by_name = sqlx::query(sqlx::AssertSqlSafe(sql));
        for id in &ids {
            by_name = by_name.bind(id);
        }
        let rows = by_name
            .bind(query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
        let mut hits: Vec<SearchHit> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for row in &rows {
            let place: String = row.try_get("place_id")?;
            let path: String = row.try_get("path")?;
            let changed_at: String = row.try_get("changed_at")?;
            seen.insert((place.clone(), path.clone()));
            hits.push(SearchHit {
                place_id: PlaceId::parse(&place).map_err(|_| FileError::DatastoreUnavailable)?,
                path,
                name_match: true,
                snippet: None,
                changed_at: ts(&changed_at)?,
            });
        }

        // 8.3: text matches by rank, up to `limit` minus the name matches.
        let room = limit - i64::try_from(hits.len()).unwrap_or(limit);
        let match_query = fts_query(query);
        if room > 0 && !match_query.is_empty() {
            // Column 1 is the UNINDEXED `path`; the text is column 2 (`body`).
            let sql = format!(
                "SELECT f.place_id AS place_id, f.path AS path, f.changed_at AS changed_at, \
                        snippet(file_index_fts, 2, '', '', '…', 24) AS snip \
                 FROM file_index_fts JOIN file_index f \
                   ON f.place_id = file_index_fts.place_id AND f.path = file_index_fts.path \
                 WHERE file_index_fts MATCH ? AND file_index_fts.place_id IN ({marks}) \
                 ORDER BY bm25(file_index_fts) LIMIT ?"
            );
            let mut by_text = sqlx::query(sqlx::AssertSqlSafe(sql)).bind(&match_query);
            for id in &ids {
                by_text = by_text.bind(id);
            }
            // Name matches take part of the room; more rows are read so the de-duplication
            // below cannot leave the list short.
            let rows = by_text
                .bind(limit.saturating_mul(2))
                .fetch_all(&self.pool)
                .await?;
            for row in &rows {
                if i64::try_from(hits.len()).unwrap_or(limit) >= limit {
                    break;
                }
                let place: String = row.try_get("place_id")?;
                let path: String = row.try_get("path")?;
                if !seen.insert((place.clone(), path.clone())) {
                    continue;
                }
                let changed_at: String = row.try_get("changed_at")?;
                let snippet: String = row.try_get("snip")?;
                let snippet: String = snippet.chars().take(SNIPPET_CHARS).collect();
                hits.push(SearchHit {
                    place_id: PlaceId::parse(&place)
                        .map_err(|_| FileError::DatastoreUnavailable)?,
                    path,
                    name_match: false,
                    snippet: Some(snippet),
                    changed_at: ts(&changed_at)?,
                });
            }
        }
        Ok(SearchResults { hits, coverage })
    }
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn each_word_is_quoted_and_the_last_is_a_prefix() {
        assert_eq!(
            fts_query("Kestrel day rate"),
            "\"Kestrel\" \"day\" \"rate\"*"
        );
        assert_eq!(fts_query("a\"b"), "\"a\"\"b\"*");
        assert_eq!(fts_query("   "), "");
    }
}
