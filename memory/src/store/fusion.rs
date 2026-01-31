//! Stage-1 rank fusion (memory-store amendment January 2026, ADR-020): the lists of step 9′.2 and the
//! fusion, quota and cap of step 10′, written once so every storage applies them identically — the
//! SurrealDB store and the SQL storage alike (SA-COLL-05).
//!
//! A storage determines the collections in play (step 9′.1), runs one signal query per (list,
//! collection) pair (step 9′.3) and hands the ranked runs to [`fuse`]. Rank is comparable across
//! collections and across signals where raw scores are not: cosine, a BM25 score normalised by its own
//! run's maximum and a fixed graph score have three distributions, and one large collection would set
//! the BM25 maximum for everyone.

use std::collections::{BTreeMap, HashMap};

use crate::types::api::SubQueryKind;
use crate::types::ports::StageOneQuery;

/// Reciprocal-rank-fusion constant `k` (SA-COLL-04).
// spec: components/memory-store#SA-COLL-04
const RRF_K: f64 = 60.0;
/// Bonus to a fact ranked first in any run (SA-COLL-04).
const BONUS_FIRST: f64 = 0.05;
/// Bonus to a fact ranked second or third in any run, when it is first in none (SA-COLL-04).
const BONUS_PODIUM: f64 = 0.02;

/// The signal a list runs.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ListKind<'a> {
    /// A vector list over this query vector.
    Vector(&'a [f32]),
    /// A keyword list over these terms.
    Keyword(&'a [String]),
    /// The graph list: facts naming a seed entity (step 9″.3).
    Graph,
    /// A link list: facts naming an entity reached at this hop (1 or 2) through live links (step
    /// 9″.4).
    Link(u8),
}

/// The weight of the graph list (step 9″.3).
pub(crate) const GRAPH_WEIGHT: f64 = 1.0;

/// The weight of a link run at `hop` (step 9″.4): 0.5 at one hop, 0.25 at two.
pub(crate) fn link_weight(hop: u8) -> f64 {
    if hop <= 1 {
        0.5
    } else {
        0.25
    }
}

/// One list of step 9′.2: a signal and the weight its runs carry in the fusion.
#[derive(Clone, Copy, Debug)]
pub(crate) struct List<'a> {
    pub kind: ListKind<'a>,
    pub weight: f64,
}

/// Step 9′.2 without the graph list, which step 9″ replaces: a plain query (`q.searches` empty) gives
/// the vector list (when `q.query_vector` is non-empty) and the keyword list (when `q.keyword_terms` is
/// non-empty), each weighted 1.0. A typed query gives one list per sub-query in request order — a
/// vector list for `Vec`, a keyword list for `Lex` — with that sub-query's weight. The storage appends
/// the graph list and the link lists of step 9″ once it knows their seeds.
pub(crate) fn lists(q: &StageOneQuery) -> Vec<List<'_>> {
    let mut out = Vec::new();
    if q.searches.is_empty() {
        if !q.query_vector.is_empty() {
            out.push(List {
                kind: ListKind::Vector(&q.query_vector),
                weight: 1.0,
            });
        }
        if !q.keyword_terms.is_empty() {
            out.push(List {
                kind: ListKind::Keyword(&q.keyword_terms),
                weight: 1.0,
            });
        }
    } else {
        for s in &q.searches {
            let kind = match s.kind {
                SubQueryKind::Vec => ListKind::Vector(&s.vector),
                SubQueryKind::Lex => ListKind::Keyword(&s.terms),
            };
            out.push(List {
                kind,
                weight: s.weight,
            });
        }
    }
    out
}

/// The hits of a graph or link run in the order the run returned them (step 9″: `ingested_at`
/// descending, ties by fact id), as scores [`fuse`] re-sorts back into that same order.
pub(crate) fn ordered_hits(ids: Vec<String>) -> Vec<(String, f64)> {
    let n = ids.len() as f64;
    ids.into_iter()
        .enumerate()
        .map(|(i, id)| (id, (n - i as f64) / (n + 1.0)))
        .collect()
}

/// Step 9″.5: the graph and link runs (those of `lists` whose kind is `Graph` or `Link`) together keep
/// at most `cap` distinct facts; above it the lowest-ranked link-run facts are dropped first — the
/// deepest hop's runs before the nearer, each from its bottom — then, if still above, the graph runs'.
// spec: components/memory-store#SA-GRAPH-05
pub(crate) fn cap_graph_runs(lists: &[List<'_>], runs: &mut [Run], cap: u32) {
    use std::collections::HashMap as Map;
    let is_graph = |l: usize| matches!(lists.get(l).map(|x| x.kind), Some(ListKind::Graph));
    let link_hop = |l: usize| match lists.get(l).map(|x| x.kind) {
        Some(ListKind::Link(h)) => Some(h),
        _ => None,
    };
    let mut seen: Map<String, usize> = Map::new();
    for r in runs.iter() {
        if is_graph(r.list) || link_hop(r.list).is_some() {
            for (id, _) in &r.hits {
                *seen.entry(id.clone()).or_default() += 1;
            }
        }
    }
    let cap = cap as usize;
    if seen.len() <= cap {
        return;
    }
    // Drop order: link runs by hop descending, then graph runs; within each, bottom-up.
    let mut order: Vec<usize> = (0..runs.len())
        .filter(|&i| link_hop(runs[i].list).is_some())
        .collect();
    order.sort_by_key(|&i| std::cmp::Reverse(link_hop(runs[i].list).unwrap_or(0)));
    order.extend((0..runs.len()).filter(|&i| is_graph(runs[i].list)));
    for i in order {
        while seen.len() > cap {
            let Some((id, _)) = runs[i].hits.pop() else {
                break;
            };
            if let Some(c) = seen.get_mut(&id) {
                *c -= 1;
                if *c == 0 {
                    seen.remove(&id);
                }
            }
        }
        if seen.len() <= cap {
            return;
        }
    }
}

/// One run of step 9′.3: list `list` (an index into the lists of [`lists`]) over collection
/// `collection`, with the facts it returned and each one's raw score — cosine similarity for a vector
/// run, the raw BM25 / `ts_rank_cd` score for a keyword run, anything for the graph run.
pub(crate) struct Run {
    pub list: usize,
    pub collection: String,
    pub hits: Vec<(String, f64)>,
}

/// A fact chosen by step 10′, with its per-signal scores and its fused scores.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Fused {
    pub id: String,
    pub collection: String,
    pub semantic_score: f64,
    pub keyword_score: f64,
    pub graph_score: f64,
    pub fused_score: f64,
    pub fused_norm: f64,
}

/// `Σ_{lists ℓ} w_ℓ / 61 + 0.05` — the fused score of a fact ranked first in every list (step 10′.2).
pub(crate) fn fused_max(lists: &[List<'_>]) -> f64 {
    lists.iter().map(|l| l.weight / (RRF_K + 1.0)).sum::<f64>() + BONUS_FIRST
}

/// Steps 10′.1–10′.4 over `runs`, whose lists are `lists`: fuse by reciprocal rank, normalise, keep the
/// per-signal scores, then select at most `stage1_k` facts with the per-collection quota. Returned
/// ordered by `fused_score` descending, ties by fact id ascending (step 10′.5's order).
///
/// Each run is re-ordered here by its own score descending, ties by fact id ascending, and cut at
/// `stage1_k`, so a storage need not guarantee the tie order its engine returns.
pub(crate) fn fuse(lists: &[List<'_>], runs: Vec<Run>, stage1_k: u16) -> Vec<Fused> {
    let cap = usize::from(stage1_k.max(1));

    struct Acc {
        collection: String,
        rrf: f64,
        best_rank: usize,
        semantic: f64,
        keyword: f64,
        graph: f64,
    }
    let mut acc: HashMap<String, Acc> = HashMap::new();

    for mut run in runs {
        let Some(list) = lists.get(run.list) else {
            continue;
        };
        run.hits
            .sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        run.hits.truncate(cap);
        // A keyword run's scores are normalised by that run's own maximum (step 10′.3).
        let run_max = run.hits.iter().map(|(_, s)| *s).fold(0.0_f64, f64::max);
        for (pos, (id, raw)) in run.hits.into_iter().enumerate() {
            let rank = pos + 1;
            let a = acc.entry(id).or_insert_with(|| Acc {
                // A fact belongs to one collection, so every run it is in is a run of that
                // collection (step 10′.1); the first run seen names it.
                collection: run.collection.clone(),
                rrf: 0.0,
                best_rank: usize::MAX,
                semantic: 0.0,
                keyword: 0.0,
                graph: 0.0,
            });
            a.rrf += list.weight / (RRF_K + rank as f64);
            a.best_rank = a.best_rank.min(rank);
            match list.kind {
                ListKind::Vector(_) => a.semantic = a.semantic.max(raw.clamp(0.0, 1.0)),
                ListKind::Keyword(_) => {
                    let norm = if run_max > 0.0 {
                        (raw / run_max).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    a.keyword = a.keyword.max(norm);
                }
                ListKind::Graph => a.graph = 1.0,
                ListKind::Link(h) => a.graph = a.graph.max(link_weight(h)),
            }
        }
    }

    let max = fused_max(lists);
    let mut all: Vec<Fused> = acc
        .into_iter()
        .map(|(id, a)| {
            let bonus = match a.best_rank {
                1 => BONUS_FIRST,
                2 | 3 => BONUS_PODIUM,
                _ => 0.0,
            };
            let fused_score = a.rrf + bonus;
            let fused_norm = if max > 0.0 {
                (fused_score / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            Fused {
                id,
                collection: a.collection,
                semantic_score: a.semantic,
                keyword_score: a.keyword,
                graph_score: a.graph,
                fused_score,
                fused_norm,
            }
        })
        .collect();
    all.sort_by(by_fused);

    select(all, cap)
}

/// `fused_score` descending, ties by fact id ascending.
fn by_fused(a: &Fused, b: &Fused) -> std::cmp::Ordering {
    b.fused_score
        .total_cmp(&a.fused_score)
        .then_with(|| a.id.cmp(&b.id))
}

/// Step 10′.4 — selection with the per-collection quota, over `all` already in fused order.
fn select(all: Vec<Fused>, cap: usize) -> Vec<Fused> {
    // C⁺: the collections with at least one fact in a run, each with its facts in fused order.
    let mut per: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, f) in all.iter().enumerate() {
        per.entry(f.collection.clone()).or_default().push(i);
    }
    let mut chosen = vec![false; all.len()];
    let mut taken = 0usize;

    if per.len() > cap {
        // Only the `cap` collections whose best fused_score is highest (ties by label ascending)
        // contribute, one fact each.
        let mut best: Vec<(&String, usize)> = per.iter().map(|(c, idx)| (c, idx[0])).collect();
        best.sort_by(|a, b| {
            all[b.1]
                .fused_score
                .total_cmp(&all[a.1].fused_score)
                .then_with(|| a.0.cmp(b.0))
        });
        for (_, i) in best.into_iter().take(cap) {
            chosen[i] = true;
            taken += 1;
        }
    } else {
        let quota = (cap / (2 * per.len().max(1))).max(1);
        for idx in per.values() {
            for &i in idx.iter().take(quota) {
                chosen[i] = true;
                taken += 1;
            }
        }
    }
    // Fill the remaining places by fused_score descending, ties by fact id ascending.
    for c in chosen.iter_mut() {
        if taken >= cap {
            break;
        }
        if !*c {
            *c = true;
            taken += 1;
        }
    }
    all.into_iter()
        .zip(chosen)
        .filter_map(|(f, c)| c.then_some(f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(list: usize, collection: &str, hits: &[(&str, f64)]) -> Run {
        Run {
            list,
            collection: collection.into(),
            hits: hits.iter().map(|(i, s)| (i.to_string(), *s)).collect(),
        }
    }

    const V: [f32; 1] = [1.0];

    #[test]
    fn rrf_bonus_and_norm_follow_sa_coll_04() {
        let terms = vec!["t".to_string()];
        let lists = [
            List {
                kind: ListKind::Keyword(&terms),
                weight: 2.0,
            },
            List {
                kind: ListKind::Vector(&V),
                weight: 1.0,
            },
        ];
        assert!((fused_max(&lists) - (3.0 / 61.0 + 0.05)).abs() < 1e-12);
        let out = fuse(
            &lists,
            vec![
                run(0, "", &[("a", 9.0), ("b", 3.0), ("c", 1.0), ("d", 0.5)]),
                run(1, "", &[("a", 0.9), ("d", 0.8)]),
            ],
            50,
        );
        let get = |id: &str| out.iter().find(|f| f.id == id).unwrap().clone();
        let a = get("a");
        assert!((a.fused_norm - 1.0).abs() < 1e-12, "first in both lists");
        assert!((a.fused_score - (2.0 / 61.0 + 1.0 / 61.0 + 0.05)).abs() < 1e-12);
        let b = get("b");
        assert!((b.fused_score - (2.0 / 62.0 + 0.02)).abs() < 1e-12);
        let d = get("d");
        // Rank 4 in the keyword run, rank 2 in the vector run: the bonus is applied once.
        assert!((d.fused_score - (2.0 / 64.0 + 1.0 / 62.0 + 0.02)).abs() < 1e-12);
        assert!((d.keyword_score - 0.5 / 9.0).abs() < 1e-12);
        assert!((d.semantic_score - 0.8).abs() < 1e-12);
        assert_eq!(out[0].id, "a");
    }

    #[test]
    fn the_quota_keeps_a_small_collection_in_the_candidates() {
        let lists = [List {
            kind: ListKind::Vector(&V),
            weight: 1.0,
        }];
        let big: Vec<(String, f64)> = (0..50)
            .map(|i| (format!("a{i:03}"), 0.9 - i as f64 * 0.001))
            .collect();
        let small: Vec<(String, f64)> = (0..30)
            .map(|i| (format!("b{i:03}"), 0.4 - i as f64 * 0.001))
            .collect();
        let out = fuse(
            &lists,
            vec![
                Run {
                    list: 0,
                    collection: "tk:a".into(),
                    hits: big,
                },
                Run {
                    list: 0,
                    collection: "tk:b".into(),
                    hits: small,
                },
            ],
            50,
        );
        assert_eq!(out.len(), 50);
        // Both collections' runs rank the same, so the fill takes the two alternately; the quota
        // alone guarantees ⌊50 / 4⌋ = 12.
        assert!(out.iter().filter(|f| f.collection == "tk:b").count() >= 12);
        assert!(out.iter().any(|f| f.id == "b000"));
    }

    #[test]
    fn more_collections_than_places_take_one_each_from_the_best() {
        let lists = [List {
            kind: ListKind::Vector(&V),
            weight: 1.0,
        }];
        let runs = (0..5)
            .map(|c| run(0, &format!("c{c}"), &[(&format!("f{c}"), 0.1 * c as f64)]))
            .collect();
        let out = fuse(&lists, runs, 3);
        // Every run's single fact is rank 1, so every fused score ties; labels ascending decide.
        let ids: Vec<&str> = out.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, vec!["f0", "f1", "f2"]);
    }

    #[test]
    fn the_graph_cap_drops_the_deepest_link_facts_first() {
        let lists = [
            List {
                kind: ListKind::Graph,
                weight: GRAPH_WEIGHT,
            },
            List {
                kind: ListKind::Link(1),
                weight: link_weight(1),
            },
            List {
                kind: ListKind::Link(2),
                weight: link_weight(2),
            },
        ];
        let mut runs = vec![
            Run {
                list: 0,
                collection: String::new(),
                hits: ordered_hits(vec!["g1".into(), "g2".into()]),
            },
            Run {
                list: 1,
                collection: String::new(),
                hits: ordered_hits(vec!["h1".into(), "g1".into(), "h2".into()]),
            },
            Run {
                list: 2,
                collection: String::new(),
                hits: ordered_hits(vec!["d1".into(), "d2".into()]),
            },
        ];
        cap_graph_runs(&lists, &mut runs, 4);
        let ids: Vec<Vec<&str>> = runs
            .iter()
            .map(|r| r.hits.iter().map(|(i, _)| i.as_str()).collect())
            .collect();
        assert_eq!(ids[0], vec!["g1", "g2"]);
        assert_eq!(ids[1], vec!["h1", "g1", "h2"]);
        assert!(ids[2].is_empty());
        // Ordered hits keep their order through fusion's re-sort.
        let h = ordered_hits(vec!["z".into(), "a".into()]);
        assert!(h[0].1 > h[1].1);
    }

    #[test]
    fn the_cap_keeps_the_best_not_the_lowest_ids() {
        let lists = [List {
            kind: ListKind::Vector(&V),
            weight: 1.0,
        }];
        // Ids ascending carry ascending scores: the as-built cap would keep the worst.
        let hits: Vec<(String, f64)> = (0..140)
            .map(|i| (format!("f{i:03}"), i as f64 / 1000.0))
            .collect();
        let out = fuse(
            &lists,
            vec![Run {
                list: 0,
                collection: String::new(),
                hits,
            }],
            50,
        );
        assert_eq!(out.len(), 50);
        assert_eq!(out[0].id, "f139");
        assert!(out.iter().all(|f| f.id.as_str() >= "f090"));
    }
}
