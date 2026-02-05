//! The integration test of `ArchitectureTest.md`: runs `cargo metadata` over the workspace and holds
//! its crate graph to Phase 2B-3, then scans the sources for the person-only commands.

/// `PERSON_ONLY_COMMANDS` as exported by `ordin-service-api` (ServiceApi.md, amendments January 2026),
/// copied in full by ArchitectureTest.md (*Shared Context*, amendment January 2026: 15 names), and the
/// three ServiceApi.md SVC-TW-2 adds (ADR-121), which the check of that list covers. This crate has no
/// workspace dependency, so it holds its own copy.
const PERSON_ONLY_COMMANDS: &[&str] = &[
    "team_knowledge.adopt",
    "team_knowledge.adopt_again",
    "team_knowledge.keep_as_is",
    "memory.correct",
    "memory.forget",
    "memory.remember_everywhere",
    "memory.keep",
    "memory.correct_link",
    "memory.forget_link",
    "memory.forget_thing",
    "memory.same_as",
    "knowledge_agents.add",
    "knowledge_agents.remove",
    "setup.update_server",
    "setup.install_update",
    // spec: ordin-desktop/components/ServiceApi#SVC-TW-2
    "team_knowledge.open_workspace",
    "team_knowledge.allow_grant",
    "team_knowledge.decline_grant",
    // spec: ordin-desktop/components/ServiceApi#SVC-LOG-1
    "diagnostics.clear_logs",
    // spec: ordin-desktop/components/ServiceApi#SVC-WEB-1
    "app.open_web_page",
    // spec: ordin-desktop/components/ServiceApi#SVC-MIN-2
    "minutes.record",
    "minutes.give_model_consent",
    "minutes.accept_notice",
    "minutes.save_obs_password",
    "minutes.remove",
    "minutes.delete_recording",
    "minutes.remove_model",
];

fn workspace_manifest() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml")
}

#[test]
fn crate_graph_matches_phase_2b3() {
    ordin_arch_test::check_table(ordin_arch_test::ALLOWED).expect("allowed table is consistent");
    let metadata = ordin_arch_test::read_metadata(&workspace_manifest()).expect("cargo metadata");
    let report = ordin_arch_test::check(&metadata, ordin_arch_test::ALLOWED);
    for name in &report.not_yet_built {
        eprintln!("not yet built: {name}");
    }
    assert!(
        report.is_ok(),
        "{}",
        report
            .violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn person_only_commands_not_reachable_by_agents() {
    let metadata = ordin_arch_test::read_metadata(&workspace_manifest()).expect("cargo metadata");
    let report = ordin_arch_test::scan::check_person_only(&metadata, PERSON_ONLY_COMMANDS);
    assert!(
        report.is_ok(),
        "{}",
        report
            .violations
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Every `warn!`/`error!` in ordin's own sources has a message that is an event name (Logging.md R-2),
/// so no warning or error is written as `invalid_event_name`.
// spec: ordin-desktop/cross-cutting/Logging#LOG-EV-1
#[test]
fn every_warning_and_error_has_an_event_name() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["crates", "apps/desktop/src"] {
        collect_rs(&root.join(dir), &mut files);
    }
    let mut bad = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let own = rel.starts_with("apps/desktop/src/")
            || (rel.starts_with("crates/ordin-") && rel.split('/').nth(2) == Some("src"));
        if !own || rel.contains("/tests/") || rel.ends_with("tests.rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        let source = match source.find("#[cfg(test)]\nmod ") {
            Some(at) => &source[..at],
            None => &source[..],
        };
        for (line, message) in log_messages(source) {
            if !is_event_name_or_placeholder(&message) {
                bad.push(format!("{rel}:{line}: {message:?}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "warn!/error! messages that are not event names:\n{}",
        bad.join("\n")
    );
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n != "target" && n != "node_modules")
            {
                collect_rs(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `(line, message)` for each `warn!(`/`error!(` call that is ordin's own: the last top-level string
/// literal of the call. A call with a `target:` outside `ordin` is a library's line and is skipped.
fn log_messages(source: &str) -> Vec<(usize, String)> {
    let bytes = source.as_bytes();
    let mut found = Vec::new();
    for name in ["warn!", "error!"] {
        let mut from = 0;
        while let Some(at) = source[from..].find(name) {
            let start = from + at;
            from = start + name.len();
            let before = bytes.get(start.wrapping_sub(1)).copied().unwrap_or(b' ');
            if before.is_ascii_alphanumeric() || before == b'_' {
                continue;
            }
            let mut i = from;
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if bytes.get(i) != Some(&b'(') {
                continue;
            }
            let open = i;
            let (mut depth, mut last, mut j) = (0usize, None, open);
            while j < bytes.len() {
                match bytes[j] {
                    b'"' => {
                        let mut k = j + 1;
                        while k < bytes.len() && bytes[k] != b'"' {
                            k += if bytes[k] == b'\\' { 2 } else { 1 };
                        }
                        if depth == 1 {
                            last = Some(source[j + 1..k.min(bytes.len())].to_owned());
                        }
                        j = k + 1;
                        continue;
                    }
                    b'(' | b'[' | b'{' => depth += 1,
                    b')' | b']' | b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            let call = &source[open..j.min(bytes.len())];
            let foreign_target = call
                .split_once("target:")
                .map(|(_, rest)| rest.trim_start())
                .is_some_and(|rest| !rest.starts_with("\"ordin"));
            if foreign_target {
                continue;
            }
            if let Some(message) = last {
                found.push((source[..start].matches('\n').count() + 1, message));
            }
        }
    }
    found
}

fn is_event_name_or_placeholder(message: &str) -> bool {
    let event = |s: &str| {
        let mut chars = s.chars();
        s.len() <= 64
            && chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
    };
    let placeholder = message
        .strip_prefix('{')
        .and_then(|m| m.strip_suffix('}'))
        .is_some_and(|inner| {
            !inner.is_empty() && inner.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
    event(message) || placeholder
}

#[test]
fn the_event_name_scan_reads_the_message_and_skips_libraries() {
    let source = r#"
        tracing::warn!(code = "X", "ssh.check_failed");
        warn!(a = 1, "checking the SSH key failed");
        tracing::error!(component = "desktop", "{line}");
        tracing::warn!(target: "hyper::client", "a library's warning");
        let x = forewarn!("ignored");
    "#;
    let checked: Vec<(String, bool)> = log_messages(source)
        .into_iter()
        .map(|(_, m)| (m.clone(), is_event_name_or_placeholder(&m)))
        .collect();
    assert_eq!(
        checked,
        vec![
            ("ssh.check_failed".to_owned(), true),
            ("checking the SSH key failed".to_owned(), false),
            ("{line}".to_owned(), true),
        ]
    );
}
