//! A language server stand-in for this crate's tests: a plain program speaking LSP over stdio.
//!
//! A line holding `ERROR` or `WARN` gets a diagnostic; a line starting `fn <name>` opens a symbol
//! that runs to the next one. `didSave` makes it ask the client two questions, `didClose` makes it
//! write a malformed frame, and a settings change is echoed as a diagnostic of `file:///settings`.
//!
//! A test target with `harness = false`, so `cargo test` and `cargo llvm-cov` both build it beside
//! the test binaries; run by cargo itself, without `STAND_IN`, it does nothing.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

fn send(out: &mut impl Write, message: &Value) {
    let body = message.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    out.flush().unwrap();
}

fn read(input: &mut impl BufRead) -> Option<Value> {
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length: ") {
            length = value.parse().ok()?;
        }
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn range(from: usize, to: usize) -> Value {
    json!({"start": {"line": from, "character": 0}, "end": {"line": to, "character": 0}})
}

fn diagnostics(text: &str) -> Vec<Value> {
    text.lines()
        .enumerate()
        .filter_map(|(at, line)| {
            let (severity, code) = if line.contains("ERROR") {
                (1, "E1")
            } else if line.contains("WARN") {
                (2, "W1")
            } else {
                return None;
            };
            Some(json!({
                "range": range(at, at),
                "severity": severity,
                "code": code,
                "message": line.trim(),
            }))
        })
        .collect()
}

fn symbols(text: &str) -> Vec<Value> {
    let starts: Vec<(usize, String)> = text
        .lines()
        .enumerate()
        .filter_map(|(at, line)| {
            let name = line.strip_prefix("fn ")?.split('(').next()?;
            Some((at, name.to_owned()))
        })
        .collect();
    let total = text.lines().count();
    starts
        .iter()
        .enumerate()
        .map(|(index, (at, name))| {
            let end = starts.get(index + 1).map_or(total, |(next, _)| *next - 1);
            json!({"name": name, "kind": 12, "range": range(*at, end)})
        })
        .collect()
}

fn main() {
    if std::env::var_os("STAND_IN").is_none() {
        return;
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut texts = std::collections::HashMap::<String, String>::new();
    let mut root = String::new();
    while let Some(message) = read(&mut input) {
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let id = message.get("id").cloned();
        let uri = message["params"]["textDocument"]["uri"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let result = match method.as_str() {
            "initialize" => {
                root = message["params"]["rootUri"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                json!({"capabilities": {}})
            }
            "textDocument/didOpen" => {
                let text = message["params"]["textDocument"]["text"]
                    .as_str()
                    .unwrap_or_default();
                texts.insert(uri.clone(), text.to_owned());
                send(
                    &mut out,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {"uri": uri, "diagnostics": diagnostics(text)},
                    }),
                );
                continue;
            }
            "textDocument/didSave" => {
                send(
                    &mut out,
                    &json!({
                        "jsonrpc": "2.0", "id": 900, "method": "workspace/configuration",
                        "params": {"items": [{"section": "x"}]},
                    }),
                );
                send(
                    &mut out,
                    &json!({
                        "jsonrpc": "2.0", "id": 901, "method": "window/showMessageRequest",
                        "params": {},
                    }),
                );
                continue;
            }
            "textDocument/didClose" => {
                out.write_all(b"this is not a frame\r\n\r\n").unwrap();
                out.flush().unwrap();
                continue;
            }
            "workspace/didChangeConfiguration" => {
                send(
                    &mut out,
                    &json!({
                        "jsonrpc": "2.0",
                        "method": "textDocument/publishDiagnostics",
                        "params": {"uri": "file:///settings", "diagnostics": [{
                            "range": range(0, 0),
                            "message": message["params"]["settings"].to_string(),
                        }]},
                    }),
                );
                continue;
            }
            "textDocument/documentSymbol" => {
                json!(symbols(texts.get(&uri).map_or("", String::as_str)))
            }
            "textDocument/definition" | "textDocument/references" => json!([
                {"uri": uri, "range": range(1, 1)},
                {"uri": "file:///elsewhere/out.rs", "range": range(2, 2)},
            ]),
            "textDocument/hover" => json!({"contents": "The rates client."}),
            "textDocument/completion" => json!([{"label": "fetch"}]),
            "workspace/symbol" => json!([{
                "name": "fetch",
                "kind": 12,
                "location": {"uri": format!("{root}/src/rates.rs"), "range": range(3, 3)},
            }]),
            "exit" => return,
            "" | "initialized" => continue,
            _ => match id {
                Some(_) => Value::Null,
                None => continue,
            },
        };
        if let Some(id) = id {
            send(
                &mut out,
                &json!({"jsonrpc": "2.0", "id": id, "result": result}),
            );
        }
    }
}
