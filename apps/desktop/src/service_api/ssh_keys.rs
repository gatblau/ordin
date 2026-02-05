//! `app.ssh_keys`, `app.create_ssh_key`, `app.choose_ssh_key` and `app.check_ssh_key` (OrdinDesktop.md
//! DSK-SSH-1 to DSK-SSH-3; ServiceApi.md SVC-SSH-1; amended January 2026; user decision; Approved by
//! the user January 2026): GitHub with an SSH key the person chose. Only the key's path is stored, in
//! `preferences.json`; the private key is never opened, read or logged here. libgit2 reads it, by path,
//! when git talks to GitHub (ProjectService.md PROJ-SSH-1).

use std::path::{Path, PathBuf};
use std::time::Duration;

use ordin_projects::ssh::{self, CheckOutcome, KeygenError, SshKeyFile};
use ordin_service_api::commands::{
    ChooseSshKey, SshCheckOutcome, SshKey, SshKeyCheck, SshKeysView,
};
use ordin_service_api::ServiceError;

use super::DesktopServiceApi;
use crate::error::{
    PREFERENCES_WRITE_FAILED, SSH_KEYGEN_MISSING, SSH_KEY_NOT_MADE, SSH_KEY_UNKNOWN, SSH_NO_KEY,
};

/// "Check it works" answers within the command's 10-second budget.
const CHECK_LIMIT: Duration = Duration::from_secs(8);

/// The person's SSH folder: `$HOME/.ssh`, or `%USERPROFILE%\.ssh` on Windows.
pub(crate) fn ssh_folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))?;
    Some(PathBuf::from(home).join(".ssh"))
}

fn wire(key: SshKeyFile) -> SshKey {
    SshKey {
        path: key.path.to_string_lossy().into_owned(),
        name: key.name,
        public_key: key.public_key,
    }
}

/// The view over `folder`, with `chosen` as stored.
// spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
pub(crate) fn keys_view(folder: Option<&Path>, chosen: Option<&Path>) -> SshKeysView {
    let keys = folder.map(ssh::list_keys).unwrap_or_default();
    SshKeysView {
        keys: keys.into_iter().map(wire).collect(),
        chosen: chosen.map(|p| p.to_string_lossy().into_owned()),
    }
}

/// The listed key whose path is `path`, if any; a path outside the folder is never accepted.
pub(crate) fn listed(folder: Option<&Path>, path: &str) -> Option<PathBuf> {
    folder
        .map(ssh::list_keys)
        .unwrap_or_default()
        .into_iter()
        .find(|k| k.path.to_string_lossy() == path)
        .map(|k| k.path)
}

fn outcome(found: CheckOutcome) -> SshCheckOutcome {
    match found {
        CheckOutcome::Works => SshCheckOutcome::Works,
        CheckOutcome::Refused => SshCheckOutcome::Refused,
        CheckOutcome::Unreachable => SshCheckOutcome::Unreachable,
    }
}

impl DesktopServiceApi {
    /// Stores the choice and hands it to git (DSK-SSH-1).
    fn store_ssh_choice(&self, key: Option<PathBuf>) -> Result<(), ServiceError> {
        let preferences = self.handle(&self.modules.preferences)?;
        preferences
            .set_github_ssh_key(key.clone())
            .map_err(|_| self.own_refusal(PREFERENCES_WRITE_FAILED, true))?;
        ssh::set_chosen_key(key);
        Ok(())
    }

    fn chosen_ssh_key(&self) -> Result<Option<PathBuf>, ServiceError> {
        Ok(self
            .handle(&self.modules.preferences)?
            .read()
            .github_ssh_key)
    }

    /// `app.ssh_keys` (DSK-SSH-1): every `*.pub` in the SSH folder with its private file beside it, and
    /// the key chosen.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
    pub(crate) fn app_ssh_keys(&self) -> Result<SshKeysView, ServiceError> {
        let chosen = self.chosen_ssh_key()?;
        Ok(keys_view(ssh_folder().as_deref(), chosen.as_deref()))
    }

    /// `app.create_ssh_key` (DSK-SSH-2): a new ed25519 key made by `ssh-keygen`, then chosen.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-2
    pub(crate) async fn app_create_ssh_key(&self) -> Result<SshKeysView, ServiceError> {
        let folder = ssh_folder().ok_or_else(|| self.own_refusal(SSH_KEY_NOT_MADE, true))?;
        let at = folder.clone();
        let made =
            tokio::task::spawn_blocking(move || ssh::create_key(&at, Path::new("ssh-keygen")))
                .await;
        let made = joined(made, "create", Err(KeygenError::Failed));
        let made = match made {
            Ok(made) => made,
            Err(KeygenError::Missing) => {
                tracing::warn!(
                    component = "desktop",
                    code = SSH_KEYGEN_MISSING,
                    "ssh_key_not_made"
                );
                return Err(self.own_refusal(SSH_KEYGEN_MISSING, false));
            }
            Err(KeygenError::Failed) => {
                tracing::warn!(
                    component = "desktop",
                    code = SSH_KEY_NOT_MADE,
                    "ssh_key_not_made"
                );
                return Err(self.own_refusal(SSH_KEY_NOT_MADE, true));
            }
        };
        self.store_ssh_choice(Some(made.path.clone()))?;
        Ok(keys_view(Some(&folder), Some(&made.path)))
    }

    /// `app.choose_ssh_key` (DSK-SSH-1): one of the listed keys, or `null` to go back to the token.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
    pub(crate) fn app_choose_ssh_key(
        &self,
        input: ChooseSshKey,
    ) -> Result<SshKeysView, ServiceError> {
        let folder = ssh_folder();
        let key = match input.path {
            None => None,
            Some(path) => Some(
                listed(folder.as_deref(), &path)
                    .ok_or_else(|| self.own_refusal(SSH_KEY_UNKNOWN, false))?,
            ),
        };
        self.store_ssh_choice(key.clone())?;
        Ok(keys_view(folder.as_deref(), key.as_deref()))
    }

    /// `app.check_ssh_key` (DSK-SSH-3): a connection to a public GitHub repository with the chosen key,
    /// GitHub's host key checked against the built-in fingerprints.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-3
    pub(crate) async fn app_check_ssh_key(&self) -> Result<SshKeyCheck, ServiceError> {
        let key = self
            .chosen_ssh_key()?
            .ok_or_else(|| self.own_refusal(SSH_NO_KEY, false))?;
        let started = std::time::Instant::now();
        let found = tokio::task::spawn_blocking(move || ssh::check_key(&key, CHECK_LIMIT)).await;
        let found = joined(found, "check", CheckOutcome::Unreachable);
        check_finished(found, started.elapsed());
        Ok(SshKeyCheck {
            outcome: outcome(found),
        })
    }
}

/// A blocking SSH task that did not finish (it panicked or was cancelled) is logged, then read as
/// `fallback`, so the person still gets an answer.
fn joined<T>(result: Result<T, tokio::task::JoinError>, task: &'static str, fallback: T) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(
                component = "desktop",
                task,
                panicked = error.is_panic(),
                "ssh.check_task_failed"
            );
            fallback
        }
    }
}

/// The outcome of a key check, as its variant name, and how long it took.
fn check_finished(found: CheckOutcome, elapsed: std::time::Duration) {
    tracing::info!(
        component = "desktop",
        outcome = format!("{found:?}").as_str(),
        elapsed_ms = elapsed.as_millis() as u64,
        "ssh.check_finished"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Lines(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Lines {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lines").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn a_failed_ssh_task_and_a_finished_check_are_logged_as_text() {
        let failed = tokio::task::spawn_blocking(|| -> CheckOutcome { panic!("boom") }).await;
        let lines = Lines::default();
        let writer = lines.clone();
        let subscriber = ordin_types::log::subscriber(move || writer.clone(), tracing::Level::INFO);
        let found = tracing::subscriber::with_default(subscriber, || {
            let found = joined(failed, "check", CheckOutcome::Unreachable);
            check_finished(found, std::time::Duration::from_millis(42));
            found
        });
        assert_eq!(found, CheckOutcome::Unreachable);
        let text = String::from_utf8(lines.0.lock().expect("lines").clone()).expect("utf-8");
        assert!(
            text.contains(r#""event":"ssh.check_task_failed""#),
            "{text}"
        );
        assert!(text.contains(r#""panicked":true"#), "{text}");
        assert!(text.contains(r#""outcome":"Unreachable""#), "{text}");
        assert!(text.contains(r#""elapsed_ms":42"#), "{text}");
        assert!(!text.contains("<debug>"), "{text}");
    }

    struct Home(PathBuf);
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn home() -> Home {
        let path = std::env::temp_dir().join(format!("ordin-dsk-ssh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(path.join(".ssh")).unwrap();
        Home(path)
    }

    // spec: ordin-desktop/components/OrdinDesktop#DSK-SSH-1
    #[test]
    fn only_a_listed_key_can_be_chosen_and_the_view_names_the_choice() {
        let h = home();
        let folder = h.0.join(".ssh");
        std::fs::write(folder.join("id_ed25519"), "PRIVATE").unwrap();
        std::fs::write(folder.join("id_ed25519.pub"), "ssh-ed25519 AAAA sam\n").unwrap();
        let key = folder.join("id_ed25519");
        let view = keys_view(Some(&folder), Some(&key));
        assert_eq!(view.keys.len(), 1);
        assert_eq!(view.keys[0].public_key, "ssh-ed25519 AAAA sam");
        assert_eq!(view.chosen.as_deref(), Some(key.to_string_lossy().as_ref()));
        assert_eq!(
            listed(Some(&folder), &key.to_string_lossy()),
            Some(key.clone())
        );
        assert_eq!(listed(Some(&folder), "/etc/passwd"), None);
        assert_eq!(listed(None, &key.to_string_lossy()), None);
        let serialised = serde_json::to_string(&view).unwrap();
        assert!(
            !serialised.contains("PRIVATE"),
            "the private key is never read"
        );
    }

    #[test]
    fn the_check_outcomes_map_one_to_one() {
        assert_eq!(outcome(CheckOutcome::Works), SshCheckOutcome::Works);
        assert_eq!(outcome(CheckOutcome::Refused), SshCheckOutcome::Refused);
        assert_eq!(
            outcome(CheckOutcome::Unreachable),
            SshCheckOutcome::Unreachable
        );
    }
}
