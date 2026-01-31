//! Playbook §7.4: `ordin-telemetry` is absent from `ordin-desktop`'s dependency graph, with every
//! build switch on (the graph the Architecture Test reads, ArchitectureTest.md amendment January 2026).

use std::collections::{BTreeSet, HashMap};
use std::process::Command;

use serde_json::Value;

fn metadata() -> Value {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--all-features"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "cargo metadata: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("metadata is JSON")
}

#[test]
fn ordin_telemetry_is_absent_from_the_desktop_dependency_graph() {
    let metadata = metadata();
    let packages = metadata["packages"].as_array().expect("packages");
    let name_of: HashMap<&str, &str> = packages
        .iter()
        .map(|p| {
            (
                p["id"].as_str().expect("id"),
                p["name"].as_str().expect("name"),
            )
        })
        .collect();
    let nodes = metadata["resolve"]["nodes"].as_array().expect("nodes");
    let deps_of: HashMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|n| {
            let id = n["id"].as_str().expect("id");
            let deps = n["deps"]
                .as_array()
                .expect("deps")
                .iter()
                .filter(|d| {
                    // Normal and build edges: what ships in the program, dev-dependencies excluded.
                    d["dep_kinds"].as_array().expect("kinds").iter().any(|k| {
                        let kind = &k["kind"];
                        kind.is_null() || kind.as_str() == Some("build")
                    })
                })
                .map(|d| d["pkg"].as_str().expect("pkg"))
                .collect();
            (id, deps)
        })
        .collect();
    let root = packages
        .iter()
        .find(|p| p["name"] == "ordin-desktop")
        .and_then(|p| p["id"].as_str())
        .expect("ordin-desktop is a member");
    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if seen.insert(id) {
            stack.extend(deps_of.get(id).into_iter().flatten().copied());
        }
    }
    let reached: BTreeSet<&str> = seen.iter().map(|id| name_of[id]).collect();
    assert!(
        reached.contains("ordin-session-supervisor"),
        "the walk reaches the modules"
    );
    assert!(
        !reached.contains("ordin-telemetry"),
        "ordin-telemetry is reached from ordin-desktop"
    );
}
