//! Unit tests for the C6 ranking, gating, and cursor arithmetic (table-driven where ≥3 cases share a
//! path). The end-to-end pipeline (embed → stage-1 → rerank → recency → gate → page → freshness) is
//! covered by `tests/features/retrieval.feature` against the real store + wiremock providers.

use super::*;

#[test]
fn recency_final_score_table() {
    // (rerank, age_days, w, tau, expected_final)
    let w = 0.15;
    let tau = 30.0;
    let cases = [
        // Fresh fact (age 0): full boost -> rerank * (1 + 0.15).
        (0.80_f64, 0.0_f64, 0.80 * 1.15),
        // Old fact (age >> tau): boost decays to ~0 -> ~= rerank.
        (0.80, 300.0, 0.80 * (1.0 + w * (-300.0_f64 / tau).exp())),
        // Zero score stays zero regardless of boost.
        (0.0, 0.0, 0.0),
    ];
    for (rerank, age, expected) in cases {
        let got = recency_final_score(rerank, age, w, tau);
        assert!(
            (got - expected).abs() < 1e-9,
            "rerank={rerank} age={age}: {got} != {expected}"
        );
    }
    // A fresher fact always outranks an older one with the same rerank score.
    let fresh = recency_final_score(0.5, 0.0, w, tau);
    let old = recency_final_score(0.5, 100.0, w, tau);
    assert!(fresh > old, "fresher fact must score higher");
}

#[test]
fn cursor_round_trips() {
    let cases = [
        (0.88_f64, "fact:018f9a2b-7c41-7e30-9d22-1a2b3c4d5e6f"),
        (0.0, "fact:zzz"),
        (0.123456789, "fact:01"),
    ];
    for (s, id) in cases {
        let encoded = Cursor {
            s,
            id: id.to_string(),
        }
        .encode()
        .expect("encode");
        // Opaque: base64url-no-pad carries no '=' padding and no '+'/'/'.
        assert!(!encoded.contains('='), "cursor must be unpadded");
        assert!(
            !encoded.contains('+') && !encoded.contains('/'),
            "cursor must be url-safe"
        );
        let decoded = Cursor::decode(&encoded).expect("decode");
        assert_eq!(decoded.s, s);
        assert_eq!(decoded.id, id);
    }
}

#[test]
fn cursor_decode_rejects_garbage() {
    // Not valid base64url.
    let err = Cursor::decode("!!!not-base64!!!").unwrap_err();
    assert!(matches!(
        err,
        AppError::Validation(ValidationKind::OutOfRange, _)
    ));
    // Valid base64url but not a Cursor JSON.
    let not_json = URL_SAFE_NO_PAD.encode(b"hello world");
    let err = Cursor::decode(&not_json).unwrap_err();
    assert!(matches!(
        err,
        AppError::Validation(ValidationKind::OutOfRange, _)
    ));
}

#[test]
fn keyword_terms_tokenises_lowercases_dedupes() {
    let terms = keyword_terms("Who Owns the Orders table, the ORDERS?");
    assert_eq!(
        terms,
        vec![
            "who".to_string(),
            "owns".into(),
            "the".into(),
            "orders".into(),
            "table".into()
        ],
        "split on non-alphanumerics, lowercase, de-duplicate in first-seen order"
    );
    assert!(
        keyword_terms("   ,. ").is_empty(),
        "punctuation-only yields no terms"
    );
}

/// C6 step 4′ (ADR-020): one search per sub-query in request order — `vec` with its vector, `lex` with
/// its keyword terms — the first weighted 2.0 and the others 1.0, after one embedding call carrying
/// only the `vec` texts; a request with no `vec` sub-query embeds nothing.
#[cfg(feature = "surreal")]
#[tokio::test]
async fn the_stage_one_query_carries_one_search_per_sub_query_the_first_weighted_double() {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    use crate::types::ports::ProviderError;

    #[derive(Default)]
    struct Embedder(Mutex<Vec<Vec<String>>>);
    #[async_trait]
    impl EmbeddingClient for Embedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, ProviderError> {
            self.0.lock().unwrap().push(texts.to_vec());
            Ok(texts
                .iter()
                .enumerate()
                .map(|(i, _)| vec![i as f32 + 1.0; 4])
                .collect())
        }
    }
    struct Reranker;
    #[async_trait]
    impl RerankClient for Reranker {
        async fn rerank(&self, _q: &str, docs: &[String]) -> Result<Vec<f64>, ProviderError> {
            Ok(vec![0.5; docs.len()])
        }
    }

    let store = crate::store::surreal::Store::new_in_memory(4)
        .await
        .unwrap();
    let embedder = Arc::new(Embedder::default());
    let engine = RetrievalEngine::new(
        Arc::new(store),
        embedder.clone(),
        Arc::new(Reranker),
        RetrievalConfig {
            embed_dim: 4,
            stage1_k: 50,
            result_cap_max: 50,
            abstain_threshold: 0.2,
            recency_weight: 0.15,
            recency_tau_days: 30.0,
            reformulation_enabled: false,
            graph_hops: 1,
            graph_fanout: 32,
            graph_cap: 200,
            trust_weight: 0.1,
            time_limit: std::time::Duration::from_secs(10),
            decay: crate::maintenance::DecayConfig {
                decay_k: 10.0,
                prune_retrievability: 0.05,
            },
        },
    );
    let sub = |kind, q: &str| SubQuery {
        kind,
        query: q.into(),
    };
    let searches = engine
        .typed_searches(&[
            sub(SubQueryKind::Lex, "orders table"),
            sub(SubQueryKind::Vec, "who owns order data"),
            sub(SubQueryKind::Vec, "ledger owner"),
        ])
        .await
        .unwrap();
    assert_eq!(
        *embedder.0.lock().unwrap(),
        vec![vec![
            "who owns order data".to_string(),
            "ledger owner".to_string()
        ]]
    );
    let weights: Vec<f64> = searches.iter().map(|s| s.weight).collect();
    assert_eq!(weights, vec![2.0, 1.0, 1.0]);
    assert_eq!(searches[0].kind, SubQueryKind::Lex);
    assert_eq!(searches[0].terms, vec!["orders", "table"]);
    assert!(searches[0].vector.is_empty());
    assert_eq!(searches[1].vector, vec![1.0; 4]);
    assert_eq!(searches[2].vector, vec![2.0; 4]);

    let lex_only = engine
        .typed_searches(&[sub(SubQueryKind::Lex, "orders")])
        .await
        .unwrap();
    assert_eq!(lex_only.len(), 1);
    assert_eq!(embedder.0.lock().unwrap().len(), 1, "no embedding call");
}

/// C6 step 1′: the shapes that pass.
#[test]
fn well_formed_requests_pass_the_shape_check() {
    for body in [
        serde_json::json!({"query": "who owns the orders table"}),
        serde_json::json!({"searches": [{"type": "lex", "query": "orders"}]}),
        serde_json::json!({"query": "x", "filters": {"collections": ["", "tk:b"]}}),
        serde_json::json!({"searches": [
            {"type": "lex", "query": "a"}, {"type": "vec", "query": "b"},
            {"type": "lex", "query": "c"}, {"type": "vec", "query": "d"}
        ], "filters": {"collections": (0..32).map(|i| format!("tk:{i}")).collect::<Vec<_>>()}}),
    ] {
        let req: RecallRequest = serde_json::from_value(body.clone()).unwrap();
        assert!(validate_request_shape(&req).is_ok(), "{body}");
    }
}

/// SA-GRAPH-08: weight 0.1 lifts a fact of trust 1.0 by 5% and leaves one of trust 0.5 as it is; the
/// factor stays within [0.75, 1.25] for weights up to 0.5.
#[test]
fn the_trust_factor_follows_sa_graph_08() {
    let cases = [
        (0.1, 1.0, 1.05),
        (0.1, 0.5, 1.0),
        (0.5, 0.0, 0.75),
        (0.5, 1.0, 1.25),
        (0.0, 0.2, 1.0),
    ];
    for (w, t, want) in cases {
        assert!((trust_factor(w, t) - want).abs() < 1e-12, "{w} {t}");
    }
}
