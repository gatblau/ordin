//! The GitHub and GitLab stand-ins: a `wiremock` server on loopback and a small program in place of
//! `gh` and `glab`. The program turns `api <path> --method M --include -f k=v …` into one `curl`
//! request, whose output has the shape of `--include`, so the real path through `run_tool` and
//! the status reading runs. Any other arguments are echoed, for the MCP tools.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use wiremock::MockServer;

use super::support::{fixture_with, Fixture, SignedIn, BOTH};
use crate::service::lock;

/// The tool is one committed script, `standin-tool.sh` (the fields travel in the request body, one
/// `key=value` line each); a folder gets a link to it named `name` and the server's address in
/// `standin-base`. The link is used, not a copy written here: a file this test process has just written
/// can still be open for writing in a child that another test's thread forked at that moment, and
/// running it then fails `ETXTBSY` ("gh could not be started" under load).
pub(crate) fn install_tool(folder: &Path, name: &str, base: &str) -> PathBuf {
    let path = folder.join(name);
    std::fs::write(folder.join("standin-base"), base).unwrap();
    if !path.exists() {
        let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/standin-tool.sh");
        std::os::unix::fs::symlink(tool, &path).unwrap();
    }
    path
}

/// A stand-in program `name` in `folder` that runs `body` with `sh`: a link to the committed
/// `linked-script.sh` and the body in `<name>.body`. Nothing that is run is written here, for the reason
/// `install_tool` gives.
pub(crate) fn linked_script(folder: &Path, name: &str, body: &str) -> PathBuf {
    let path = folder.join(name);
    std::fs::write(folder.join(format!("{name}.body")), format!("{body}\n")).unwrap();
    let tool = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/linked-script.sh");
    std::os::unix::fs::symlink(tool, &path).unwrap();
    path
}

pub(crate) struct Hosts {
    pub fixture: Fixture,
    pub server: MockServer,
}

impl Hosts {
    pub(crate) fn service(&self) -> &Arc<crate::ProjectService> {
        &self.fixture.service
    }
}

/// `tools` named `gh` and `glab` that talk to `server`.
pub(crate) async fn hosts_with(signed_in: SignedIn) -> Hosts {
    let fixture = fixture_with(signed_in).await;
    let server = MockServer::start().await;
    point_tools(&fixture, &server.uri());
    Hosts { fixture, server }
}

pub(crate) fn point_tools(fixture: &Fixture, base: &str) {
    let gh = install_tool(&fixture.dir.0, "gh", base);
    let glab = install_tool(&fixture.dir.0, "glab", base);
    let mut tunables = lock(&fixture.service.tunables);
    tunables.gh_program = gh;
    tunables.glab_program = glab;
}

pub(crate) async fn hosts() -> Hosts {
    hosts_with(BOTH).await
}
