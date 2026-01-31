//! `ordin-service-api-check-requests <folder> [<allow list>]`: checks every request the screens'
//! tests sent against the rules the backend applies before dispatch (plan 04, safety net A).
//!
//! The screens' tests write one `<test file>.jsonl` per test file into `<folder>`, one
//! `{"test": …, "command": …, "input": …}` object per line. Each line goes through
//! `ordin_service_api::check_request`, the steps `accept` runs on the command and its input.
//!
//! A refusal whose code is the one the test's fake backend answered with (`replied`) agrees with the
//! test: the screen draws the reply the backend gives, so it is counted, not reported.
//!
//! The allow list holds the requests a test sends wrong on purpose, one `<test file> <command> <reason>`
//! per line (`#` starts a comment). A refusal it names is counted, not reported.
//!
//! Prints one line per refusal, `<test file> <command> <code> at <pointer> (value <JSON>)`, then a
//! summary line. Exits 1 when a refusal is not on the allow list, or when the folder holds no request
//! at all (the tests did not record, so nothing was checked).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;

use serde::Deserialize;
use serde_json::Value;

const USAGE: &str = "usage: ordin-service-api-check-requests <folder> [<allow list>]";

#[derive(Deserialize)]
struct Recorded {
    test: String,
    command: String,
    input: Value,
    /// The error code the test's fake backend answered with, if it answered with one.
    #[serde(default)]
    replied: Option<String>,
}

fn allow_list(path: Option<&Path>) -> Result<BTreeSet<(String, String)>, String> {
    let Some(path) = path else {
        return Ok(BTreeSet::new());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut allowed = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, char::is_whitespace);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(test), Some(command), Some(reason)) if !reason.trim().is_empty() => {
                allowed.insert((test.to_owned(), command.to_owned()));
            }
            _ => {
                return Err(format!(
                    "{}:{}: write <test file> <command> <reason>",
                    path.display(),
                    n + 1
                ))
            }
        }
    }
    Ok(allowed)
}

fn run(folder: &Path, allowed: &BTreeSet<(String, String)>) -> Result<bool, String> {
    let mut files: Vec<_> = std::fs::read_dir(folder)
        .map_err(|e| format!("cannot read {}: {e}", folder.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    let mut checked = 0usize;
    let mut refused: BTreeMap<String, usize> = BTreeMap::new();
    let mut allowed_count = 0usize;
    let mut agreed = 0usize;
    let mut seen = BTreeSet::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        for (n, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let recorded: Recorded = serde_json::from_str(line).map_err(|e| {
                format!("{}:{}: not a recorded request: {e}", file.display(), n + 1)
            })?;
            checked += 1;
            let Err(rejection) =
                ordin_service_api::check_request(&recorded.command, recorded.input.clone())
            else {
                continue;
            };
            // The test answered with the refusal the backend gives: the screen draws the real reply.
            if recorded.replied.as_deref() == Some(rejection.code) {
                agreed += 1;
                continue;
            }
            if allowed.contains(&(recorded.test.clone(), recorded.command.clone())) {
                allowed_count += 1;
                continue;
            }
            // One line per test file, command, code and pointer: a test that sends the same bad
            // request ten times is one defect.
            let report = format!(
                "{} {} {} at {} (value {})",
                recorded.test,
                recorded.command,
                rejection.code,
                rejection.at,
                refused_value(&recorded.input, &rejection.at)
            );
            if seen.insert(report.clone()) {
                println!("{report}");
            }
            *refused.entry(recorded.command).or_default() += 1;
        }
    }
    let total: usize = refused.values().sum();
    println!(
        "checked {checked} requests from {} test files: {total} refused ({} distinct), {agreed} refused as the test's reply says, {allowed_count} refused on purpose",
        files.len(),
        seen.len()
    );
    if checked == 0 {
        println!("no request was recorded: run the screens' tests with ORDIN_UI_REQUESTS_DIR set");
        return Ok(false);
    }
    Ok(total == 0)
}

/// The refused value, as JSON cut to 80 characters, so the line says what was wrong. The inputs are
/// the screens' test data, never a person's.
fn refused_value(input: &Value, at: &str) -> String {
    let pointer = at.strip_prefix("/input").unwrap_or(at);
    let text = match input.pointer(pointer) {
        Some(value) => value.to_string(),
        None => "missing".to_owned(),
    };
    if text.chars().count() > 80 {
        format!("{}…", text.chars().take(80).collect::<String>())
    } else {
        text
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (folder, list) = match args.as_slice() {
        [folder] => (Path::new(folder), None),
        [folder, list] => (Path::new(folder), Some(Path::new(list))),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = allow_list(list).and_then(|allowed| run(folder, &allowed));
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}
