//! Remote-engine round-trip (NFR-MA1) — exercises the embedded↔remote SurrealDB abstraction by
//! connecting over the network to a real `surrealdb/surrealdb` 3.x server started via
//! testcontainers, applying the C1 schema DDL, and doing a `put_fact`→`get_fact`-shaped round-trip.
//!
//! Docker may be absent in CI; if the container cannot start, the test prints a notice and skips
//! gracefully rather than failing the suite.

use std::time::Duration;

use serde_json::Value as Json;
use surrealdb::engine::remote::ws::Ws;
use surrealdb::opt::auth::Root;
use surrealdb::types::Value;
use surrealdb::Surreal;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

mod support;

/// The pinned SurrealDB server image tag (3.x), matching the embedded `surrealdb` crate major.
const SURREALDB_IMAGE: &str = "surrealdb/surrealdb";
const SURREALDB_TAG: &str = "v3.1.5";

#[tokio::test]
async fn remote_put_get_round_trip() {
    // Start a SurrealDB server in in-memory mode with root credentials.
    let image = GenericImage::new(SURREALDB_IMAGE, SURREALDB_TAG)
        .with_exposed_port(8000.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Started web server"))
        .with_cmd([
            "start",
            "--user",
            "root",
            "--pass",
            "root",
            "--bind",
            "0.0.0.0:8000",
            "memory",
        ]);

    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP remote_put_get_round_trip: could not start SurrealDB container ({e}); docker may be absent");
            return;
        }
    };

    let host_port = match container.get_host_port_ipv4(8000).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIP remote_put_get_round_trip: could not resolve host port ({e})");
            return;
        }
    };

    let endpoint = format!("127.0.0.1:{host_port}");
    let db: Surreal<surrealdb::engine::remote::ws::Client> =
        match connect_with_retry(&endpoint).await {
            Ok(db) => db,
            Err(e) => {
                eprintln!("SKIP remote_put_get_round_trip: could not connect to {endpoint} ({e})");
                return;
            }
        };

    db.signin(Root {
        username: "root".into(),
        password: "root".into(),
    })
    .await
    .expect("signin root");

    // Provision the tenant namespace/database and the fact table (the C1 schema, 3.x DDL).
    db.query("DEFINE NAMESPACE IF NOT EXISTS acme")
        .await
        .expect("define ns")
        .check()
        .expect("ns ok");
    db.use_ns("acme").use_db("memory").await.expect("use ns/db");
    db.query(
        "DEFINE DATABASE IF NOT EXISTS recall; \
         DEFINE TABLE IF NOT EXISTS fact SCHEMALESS; \
         DEFINE FIELD IF NOT EXISTS owner ON fact TYPE object; \
         DEFINE FIELD IF NOT EXISTS confidence ON fact TYPE float;",
    )
    .await
    .expect("schema ddl")
    .check()
    .expect("ddl ok");

    // put_fact-shaped write: parameterised CREATE binding a record object.
    let mut obj = surrealdb::types::Object::new();
    let mut owner = surrealdb::types::Object::new();
    owner.insert("tenant", Value::String("acme".into()));
    owner.insert("user", Value::String("u-sarah".into()));
    obj.insert("owner", Value::Object(owner));
    obj.insert(
        "confidence",
        Value::Number(surrealdb::types::Number::Float(0.9)),
    );
    db.query("CREATE fact:rt CONTENT $rec")
        .bind(("rec", Value::Object(obj)))
        .await
        .expect("create fact")
        .check()
        .expect("create ok");

    // get_fact-shaped read: scope-filtered SELECT, taken as JSON.
    let mut resp = db
        .query("SELECT * FROM fact:rt WHERE owner.user = $u")
        .bind(("u", "u-sarah"))
        .await
        .expect("select fact");
    let rows: Vec<Json> = resp.take(0).expect("take rows");
    assert_eq!(rows.len(), 1, "expected the round-tripped fact");
    let conf = rows[0].get("confidence").and_then(|v| v.as_f64());
    assert_eq!(conf, Some(0.9), "confidence round-trips over the network");

    eprintln!(
        "remote_put_get_round_trip: ran against {SURREALDB_IMAGE}:{SURREALDB_TAG} over the network"
    );
}

/// Connect to the WS endpoint, retrying briefly while the server finishes binding.
async fn connect_with_retry(
    endpoint: &str,
) -> Result<Surreal<surrealdb::engine::remote::ws::Client>, surrealdb::Error> {
    let mut last_err = None;
    for _ in 0..30 {
        match Surreal::new::<Ws>(endpoint).await {
            Ok(db) => return Ok(db),
            Err(e) => {
                last_err = Some(e);
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    Err(last_err.expect("at least one connection attempt"))
}

/// FU-019 — `Store::connect` signs in to a SECURED remote SurrealDB when credentials are configured,
/// and a credential-less connect against the same secured server cannot run privileged statements.
///
/// One authenticated server, two `Store::connect` calls:
///   * with `MEMORY_STORE_REMOTE_USER`/`_PASS` → the session is authenticated, DDL+DML round-trips;
///   * without them → connect succeeds (no signin) but a privileged statement is rejected.
#[tokio::test]
async fn store_connect_signs_in_to_a_secured_server() {
    // Authenticated server: root/root, in-memory.
    let image = GenericImage::new(SURREALDB_IMAGE, SURREALDB_TAG)
        .with_exposed_port(8000.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Started web server"))
        .with_cmd([
            "start",
            "--user",
            "root",
            "--pass",
            "root",
            "--bind",
            "0.0.0.0:8000",
            "memory",
        ]);
    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP store_connect_signs_in_to_a_secured_server: could not start SurrealDB ({e}); docker may be absent");
            return;
        }
    };
    let host_port = match container.get_host_port_ipv4(8000).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIP store_connect_signs_in_to_a_secured_server: no host port ({e})");
            return;
        }
    };
    let ws_url = format!("ws://127.0.0.1:{host_port}");

    // --- POSITIVE: creds configured → Store::connect signs in → DDL+DML round-trips ---------------
    let mut m = support::minimal_env();
    m.insert("MEMORY_STORE_REMOTE_URL".into(), ws_url.clone());
    m.insert("MEMORY_STORE_REMOTE_USER".into(), "root".into());
    m.insert("MEMORY_STORE_REMOTE_PASS".into(), "root".into());
    m.insert("MEMORY_EMBED_DIM".into(), "8".into());
    let cfg = support::config_from_map(&m);

    let store = memory::store::Store::connect(&cfg)
        .await
        .expect("Store::connect should sign in to the secured server with valid creds");
    let db = store.handle();
    db.query("DEFINE NAMESPACE IF NOT EXISTS acme")
        .await
        .expect("define ns over the authenticated session")
        .check()
        .expect("ns ddl ok (authenticated)");
    db.use_ns("acme").use_db("memory").await.expect("use ns/db");
    db.query("DEFINE TABLE IF NOT EXISTS fact SCHEMALESS; CREATE fact:rt SET confidence = 0.9")
        .await
        .expect("ddl + create over the authenticated session")
        .check()
        .expect("create ok (authenticated)");
    let mut resp = db
        .query("SELECT confidence FROM fact:rt")
        .await
        .expect("select over the authenticated session");
    let rows: Vec<Json> = resp.take(0).expect("take rows");
    assert_eq!(
        rows.len(),
        1,
        "the credentialed session round-trips a record"
    );
    assert_eq!(
        rows[0].get("confidence").and_then(|v| v.as_f64()),
        Some(0.9),
        "value round-trips over the authenticated remote connection"
    );

    // --- NEGATIVE: no creds → Store::connect (no signin) → privileged statement is rejected -------
    let mut m2 = support::minimal_env();
    m2.insert("MEMORY_STORE_REMOTE_URL".into(), ws_url.clone());
    m2.insert("MEMORY_EMBED_DIM".into(), "8".into());
    let cfg2 = support::config_from_map(&m2);

    let store2 = memory::store::Store::connect(&cfg2)
        .await
        .expect("connect itself succeeds without creds (no signin attempted)");
    let db2 = store2.handle();
    // A root-level statement on an unauthenticated session against a secured server must be rejected.
    let unauth = db2
        .query("DEFINE NAMESPACE IF NOT EXISTS globex")
        .await
        .and_then(|mut r| r.take::<Vec<Json>>(0).map(|_| ()));
    assert!(
        unauth.is_err(),
        "a secured server must reject privileged DDL on an unauthenticated session (proves signin is required)"
    );

    eprintln!("store_connect_signs_in_to_a_secured_server: signin verified against {SURREALDB_IMAGE}:{SURREALDB_TAG}");
}

/// SPIKE (RFC 03, D1 option 1) — the namespace-qualified batch, over a real network connection.
///
/// The embedded spikes in `src/store/mod.rs` show that folding `USE NS <t> DB memory;` into the same
/// `query()` batch as the statement removes the cross-tenant race. Embedded is the weaker test: the
/// engine is in-process, so "one request" is close to a function call. This runs the same shape
/// against a real `surrealdb/surrealdb` server over WebSocket, where a batch is a genuine network
/// round trip on a shared connection and interleaving is most likely.
///
/// Control and treatment run against the SAME connection in the SAME test, so the only difference is
/// where the namespace comes from.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spike_ns_qualified_batch_over_remote_connection() {
    use std::sync::Arc;

    const TENANTS: [&str; 4] = ["acme", "globex", "initech", "umbrella"];
    const PER_TENANT: usize = 25;

    let image = GenericImage::new(SURREALDB_IMAGE, SURREALDB_TAG)
        .with_exposed_port(8000.tcp())
        .with_wait_for(WaitFor::message_on_stdout("Started web server"))
        .with_cmd([
            "start",
            "--user",
            "root",
            "--pass",
            "root",
            "--bind",
            "0.0.0.0:8000",
            "memory",
        ]);

    let container = match image.start().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("SKIP spike_ns_qualified_batch_over_remote_connection: no container ({e})");
            return;
        }
    };
    let host_port = match container.get_host_port_ipv4(8000).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("SKIP spike_ns_qualified_batch_over_remote_connection: no port ({e})");
            return;
        }
    };
    let endpoint = format!("127.0.0.1:{host_port}");
    let db: Surreal<surrealdb::engine::remote::ws::Client> =
        match connect_with_retry(&endpoint).await {
            Ok(db) => db,
            Err(e) => {
                eprintln!("SKIP spike_ns_qualified_batch_over_remote_connection: connect ({e})");
                return;
            }
        };
    db.signin(Root {
        username: "root".into(),
        password: "root".into(),
    })
    .await
    .expect("signin root");

    // Provision each namespace and pre-define the probe table (sequential; no race here).
    for t in TENANTS {
        db.query(format!(
            "DEFINE NAMESPACE IF NOT EXISTS {t}; USE NS {t}; DEFINE DATABASE IF NOT EXISTS recall;"
        ))
        .await
        .expect("define ns/db")
        .check()
        .expect("ns/db ok");
        db.query(format!(
            "USE NS {t} DB memory; DEFINE TABLE IF NOT EXISTS probe SCHEMALESS"
        ))
        .await
        .expect("define probe")
        .check()
        .expect("probe ok");
    }

    let db = Arc::new(db);

    // ---- CONTROL: the current shape — select the namespace, then write. ----
    let mut handles = Vec::new();
    for tenant in TENANTS {
        for i in 0..PER_TENANT {
            let db = Arc::clone(&db);
            handles.push(tokio::spawn(async move {
                db.use_ns(tenant).use_db("memory").await.expect("use");
                let _ = db
                    .query("CREATE probe SET tenant = $t, seq = $i, mode = 'control'")
                    .bind(("t", tenant.to_string()))
                    .bind(("i", i as i64))
                    .await;
            }));
        }
    }
    for h in handles {
        h.await.expect("join");
    }

    // ---- TREATMENT: the namespace travels with the statement. ----
    let mut handles = Vec::new();
    for tenant in TENANTS {
        for i in 0..PER_TENANT {
            let db = Arc::clone(&db);
            handles.push(tokio::spawn(async move {
                let _ = db
                    .query(format!(
                        "USE NS {tenant} DB memory; \
                         CREATE probe SET tenant = $t, seq = $i, mode = 'treatment'"
                    ))
                    .bind(("t", tenant.to_string()))
                    .bind(("i", i as i64))
                    .await;
            }));
        }
    }
    for h in handles {
        h.await.expect("join");
    }

    // ---- Verify: count rows sitting in a namespace they were not written for. ----
    let mut control_misplaced = 0usize;
    let mut treatment_misplaced = 0usize;
    for tenant in TENANTS {
        let mut resp = db
            .query(format!(
                "USE NS {tenant} DB memory; SELECT tenant, mode FROM probe"
            ))
            .await
            .expect("verify");
        let rows: Vec<Json> = resp.take(1).expect("rows");
        for r in &rows {
            let owner = r.get("tenant").and_then(|v| v.as_str()).unwrap_or("?");
            let mode = r.get("mode").and_then(|v| v.as_str()).unwrap_or("?");
            if owner != tenant {
                if mode == "control" {
                    control_misplaced += 1;
                } else {
                    treatment_misplaced += 1;
                }
            }
        }
    }

    eprintln!(
        "SPIKE (remote): control misplaced = {control_misplaced}, treatment misplaced = {treatment_misplaced} \
         (out of {} writes each)",
        TENANTS.len() * PER_TENANT
    );

    assert_eq!(
        treatment_misplaced, 0,
        "namespace-qualified batches crossed namespaces over a remote connection \
         ({treatment_misplaced} misplaced); control misplaced {control_misplaced}"
    );
}
