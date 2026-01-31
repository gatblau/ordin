//! The Claude Code child's environment (ClaudeLocalAdapter.md `start` step 8). It is built from an
//! empty map: an allow-list of ordin's own variables, then what the adapter sets. Nothing here is
//! logged and nothing touches ordin's own environment.

use std::ffi::OsString;
use std::sync::Arc;

use ordin_hook_relay::HookSessionEnv;

use crate::config::{KeyRoute, SignInMode};

/// Reads one variable of ordin's own environment. Production reads the process; tests give their own.
pub(crate) type ParentEnv = Arc<dyn Fn(&str) -> Option<OsString> + Send + Sync>;

/// Copied when present, on every platform.
const COPIED: [&str; 8] = [
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "TMPDIR",
];
/// Copied when present, in either case.
const PROXIES: [&str; 3] = ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"];
/// Copied when present, on Windows only.
#[cfg(windows)]
const WINDOWS_ONLY: [&str; 5] = [
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "TEMP",
];

/// `hook` is `None` for the CLI checks and `claude auth login`, which run without hook variables.
// spec: ordin-desktop/components/ClaudeLocalAdapter#SA-CLOSE-01
// spec: ordin-desktop/components/ClaudeLocalAdapter#ADR-044
// spec: ordin-desktop/components/ClaudeLocalAdapter#C-74
pub(crate) fn child_environment(
    parent: &ParentEnv,
    sign_in: &SignInMode,
    hook: Option<&HookSessionEnv>,
) -> Vec<(String, OsString)> {
    let mut env: Vec<(String, OsString)> = Vec::new();
    let mut copy = |name: &str| {
        if let Some(value) = parent(name) {
            env.push((name.to_owned(), value));
        }
    };
    COPIED.iter().for_each(|name| copy(name));
    for name in PROXIES {
        copy(name);
        copy(&name.to_ascii_lowercase());
    }
    #[cfg(windows)]
    WINDOWS_ONLY.iter().for_each(|name| copy(name));

    if let Some(hook) = hook {
        for (name, value) in hook.env_vars() {
            env.push((name.to_owned(), value.into()));
        }
    }
    if let SignInMode::OrganisationKey { models, route } = sign_in {
        if let (KeyRoute::Environment, Some(key)) = (route, &models.key) {
            env.push(("ANTHROPIC_API_KEY".to_owned(), key.expose().into()));
        }
        if let Some(gateway) = &models.gateway_url {
            env.push(("ANTHROPIC_BASE_URL".to_owned(), gateway.into()));
        }
        for variable in &models.environment {
            env.push((variable.name.clone(), variable.value().into()));
        }
    }
    env
}
