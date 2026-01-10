//! `ordin-service-api-gen`: writes or checks the JSON Schema and TypeScript files the screens module
//! is written against (ServiceApi.md *Schema and TypeScript generation*).
//!
//! - `ordin-service-api-gen write <ui_dir>` writes `<ui_dir>/src/lib/service-api/schema.gen.json`,
//!   `types.gen.ts` and `commands.gen.ts`.
//! - `ordin-service-api-gen check <ui_dir>` regenerates in memory and exits 1 with the line
//!   `service-api types are stale: <file>` when any file differs.

use std::path::Path;
use std::process::ExitCode;

use ordin_service_api::typescript::{generate, OUTPUT_DIR};

const USAGE: &str = "usage: ordin-service-api-gen (write|check) <ui_dir>";

fn write(ui_dir: &Path) -> ExitCode {
    let dir = ui_dir.join(OUTPUT_DIR);
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {}: {error}", dir.display());
        return ExitCode::FAILURE;
    }
    for (name, content) in generate() {
        let path = dir.join(name);
        if let Err(error) = std::fs::write(&path, content) {
            eprintln!("cannot write {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

fn check(ui_dir: &Path) -> ExitCode {
    let dir = ui_dir.join(OUTPUT_DIR);
    let mut stale = false;
    for (name, content) in generate() {
        let current = std::fs::read_to_string(dir.join(name)).ok();
        if current.as_deref() != Some(content.as_str()) {
            println!("service-api types are stale: {name}");
            stale = true;
        }
    }
    if stale {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, dir] if command == "write" => write(Path::new(dir)),
        [command, dir] if command == "check" => check(Path::new(dir)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
