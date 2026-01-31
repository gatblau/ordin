//! Linked git repositories and the workspace summary (WorkspaceService.md Internal Logic 6.7, 10.2
//! and 10.3). No git library: the files of `.git` are read as text.

use std::path::{Path, PathBuf};

use ordin_types::domain::places::{SourceLink, SourceLinkKind, SourceLinkState};
use ordin_types::domain::runs::ShareHost;
use ordin_types::WorkspaceId;

use crate::error::WorkspaceError;
use crate::log;
use crate::service::WorkspaceService;
use crate::types::{LinkedRepository, WorkspaceSummary};

/// The git directory of a working tree: `.git` itself, or the directory a `.git` file names, followed
/// once.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.trim().strip_prefix("gitdir:")?.trim();
    let path = Path::new(target);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    resolved.is_dir().then_some(resolved)
}

/// Where a linked worktree keeps its shared refs: the directory its `commondir` file names.
fn refs_dir(git: &Path) -> PathBuf {
    match std::fs::read_to_string(git.join("commondir")) {
        Ok(text) => {
            let common = Path::new(text.trim());
            if common.is_absolute() {
                common.to_path_buf()
            } else {
                git.join(common)
            }
        }
        Err(_) => git.to_path_buf(),
    }
}

/// The branch `HEAD` names and the commit of `refs/remotes/origin/<branch>`, read as text
/// (Internal Logic 6.7). A detached `HEAD` or an unreadable ref is `None`.
pub(crate) fn git_origin(root: &Path) -> Option<(String, String)> {
    let git = git_dir(root)?;
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?.to_owned();
    let common = refs_dir(&git);
    let reference = format!("refs/remotes/origin/{branch}");
    if let Ok(text) = std::fs::read_to_string(common.join(&reference)) {
        let commit = text.trim();
        if !commit.is_empty() {
            return Some((branch, commit.to_owned()));
        }
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (commit, name) = line.split_once(' ')?;
        (name.trim() == reference && !commit.starts_with('#') && !commit.starts_with('^'))
            .then(|| (branch.clone(), commit.to_owned()))
    })
}

/// The `url` of `[remote "origin"]` in a git config.
fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
            continue;
        }
        if in_origin {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "url" {
                    return Some(value.trim().to_owned());
                }
            }
        }
    }
    None
}

/// `(host name, owner/name)` of `https://<host>/<owner>/<name>(.git)?` or
/// `git@<host>:<owner>/<name>(.git)?`.
fn parse_remote(url: &str) -> Option<(String, String)> {
    let (host, path) = if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        (host.to_owned(), path.to_owned())
    } else {
        let parsed = url::Url::parse(url).ok()?;
        if parsed.scheme() != "https" {
            return None;
        }
        (
            parsed.host_str()?.to_owned(),
            parsed.path().trim_start_matches('/').to_owned(),
        )
    };
    let path = path.strip_suffix(".git").unwrap_or(&path);
    let (owner, name) = path.split_once('/')?;
    if host.is_empty() || owner.is_empty() || name.is_empty() || name.contains('/') {
        return None;
    }
    Some((host, format!("{owner}/{name}")))
}

fn share_host(host_name: &str) -> Option<ShareHost> {
    let lower = host_name.to_lowercase();
    if lower == "github.com" {
        Some(ShareHost::Github)
    } else if lower == "gitlab.com" || lower.starts_with("gitlab.") {
        Some(ShareHost::Gitlab)
    } else {
        None
    }
}

impl WorkspaceService {
    /// Internal Logic 10.2 for one link: `None` (and `repository_remote_unknown`) when the folder has
    /// no `origin` remote on GitHub or GitLab.
    pub(crate) async fn linked_repository(&self, link: &SourceLink) -> Option<LinkedRepository> {
        let place_id = link.place_id?;
        let place = self.files.place(place_id).await.ok()?;
        let root = PathBuf::from(&place.path);
        let found = tokio::task::spawn_blocking(move || {
            let git = git_dir(&root)?;
            let config = std::fs::read_to_string(refs_dir(&git).join("config"))
                .or_else(|_| std::fs::read_to_string(git.join("config")))
                .ok()?;
            let url = origin_url(&config)?;
            let (host_name, repository) = parse_remote(&url)?;
            let host = share_host(&host_name)?;
            Some((host, host_name, repository))
        })
        .await
        .ok()
        .flatten();
        match found {
            Some((host, host_name, repository)) => Some(LinkedRepository {
                source_link_id: link.id,
                host,
                repository,
                host_name,
                place_id,
            }),
            None => {
                log::repository_remote_unknown(link.id);
                None
            }
        }
    }

    pub async fn linked_repositories(
        &self,
        id: WorkspaceId,
    ) -> Result<Vec<LinkedRepository>, WorkspaceError> {
        self.workspace(id).await?;
        let mut found = Vec::new();
        for link in self.links_of(id).await? {
            if link.kind == SourceLinkKind::GitRepositoryFolder
                && link.state == SourceLinkState::Active
            {
                if let Some(repository) = self.linked_repository(&link).await {
                    found.push(repository);
                }
            }
        }
        Ok(found)
    }

    /// Internal Logic 10.3: the own folder first, then the active links by `added_at`.
    pub async fn summary(&self, id: WorkspaceId) -> Result<WorkspaceSummary, WorkspaceError> {
        let workspace = self.workspace(id).await?;
        let mut places = Vec::new();
        if let Ok(own) = self.files.place(workspace.own_folder_place_id).await {
            places.push(own.friendly_name);
        }
        for link in self.links_of(id).await? {
            if link.state == SourceLinkState::Active {
                places.push(link.display_name);
            }
        }
        Ok(WorkspaceSummary {
            workspace_id: id,
            name: workspace.name,
            state: workspace.state,
            places,
            repositories: self.linked_repositories(id).await?,
        })
    }
}
