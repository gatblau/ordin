//! The fake OBS of Step 81 serving a WebSocket port, for journey 13 (phase-5-playbook.md PB-MIN-2).
//!
//! `fake-obs <port> <output file>`: an OBS 30 with no password on 127.0.0.1:<port>. StopRecord answers
//! with `<output file>` as the recording; the journey puts a real recording there. Runs until killed.
// spec: ordin-desktop/phase-5-playbook#PB-MIN-2

use std::path::PathBuf;
use std::process::ExitCode;

use ordin_obs::fake::{FakeObs, Obs};

const USAGE: &str = "usage: fake-obs <port> <output file>";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, output] = args.as_slice() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let Ok(port) = port.parse::<u16>() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let fake = FakeObs::start_on(Obs::new("30.2.3", PathBuf::from(output)), port).await;
    println!("fake obs listening on 127.0.0.1:{}", fake.port);
    std::future::pending::<()>().await;
    ExitCode::SUCCESS
}
