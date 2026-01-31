//! Adding the bundled agents at start (OrdinDesktop.md, amendment January 2026, DSK-REC-24): one
//! task, not awaited, asks the Package Catalogue for Ordin's agent and then, in a build with the
//! `authoring` switch, the agent maker (DSK-REC-24a), retrying a retryable failure. Step 4 spawns it
//! once `catalogue.start(shutdown)` has returned `Ok`. The same task then adds the starter agents to
//! every active workspace (DSK-DOC-1), and `workspaces.create` adds them to a new one.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use ordin_localisation::Localisation;
use ordin_package_catalogue::{BundledAgent, PackageCatalogue, PackageError};
use ordin_types::{AgentInstanceId, WorkspaceId};
use ordin_workspaces::{WorkspaceListFilter, WorkspaceService};
use tokio_util::sync::CancellationToken;

/// The wait before another attempt after a retryable failure (*chosen here* by DSK-REC-24).
pub const RETRY_AFTER: Duration = Duration::from_secs(30);
/// Attempts per agent per start, the first included (*chosen here* by DSK-REC-24).
pub const MAX_ATTEMPTS: u32 = 5;

/// How one bundled agent's calls ended at this start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundledOutcome {
    Ready(AgentInstanceId),
    /// The last error's code; no further attempt until the next start.
    Unavailable(&'static str),
    /// The shutdown token was cancelled between attempts.
    Cancelled,
}

/// The bundled agents of this build, in order: Ordin's agent, then the agent maker only when the
/// program is built with the `authoring` switch, which brings it.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-24a
pub fn bundled_agents() -> Vec<BundledAgent> {
    let mut agents = vec![BundledAgent::Ordin];
    if cfg!(feature = "authoring") {
        agents.push(BundledAgent::AgentMaker);
    }
    agents
}

/// `packages.bundled_instance` for each of `bundled_agents()`, in the active locale.
// spec: ordin-desktop/components/OrdinDesktop#DSK-REC-24
pub async fn add_bundled_agents(
    packages: Arc<PackageCatalogue>,
    localisation: Arc<Localisation>,
    shutdown: CancellationToken,
) -> Vec<(BundledAgent, BundledOutcome)> {
    let call = |agent: BundledAgent| {
        let packages = packages.clone();
        let locale = localisation.active().locale;
        async move {
            packages
                .bundled_instance(agent, &locale)
                .await
                .map(|installed| installed.instance.id)
        }
    };
    add_each(&bundled_agents(), call, RETRY_AFTER, &shutdown).await
}

/// `agents` in order, each with DSK-REC-24's retry rule over `call`.
pub async fn add_each<F, Fut>(
    agents: &[BundledAgent],
    call: F,
    retry_after: Duration,
    shutdown: &CancellationToken,
) -> Vec<(BundledAgent, BundledOutcome)>
where
    F: Fn(BundledAgent) -> Fut,
    Fut: Future<Output = Result<AgentInstanceId, PackageError>>,
{
    let mut out = Vec::with_capacity(agents.len());
    for &agent in agents {
        let outcome = add_one(agent, &call, retry_after, shutdown).await;
        let cancelled = outcome == BundledOutcome::Cancelled;
        out.push((agent, outcome));
        if cancelled {
            break;
        }
    }
    out
}

async fn add_one<F, Fut>(
    agent: BundledAgent,
    call: &F,
    retry_after: Duration,
    shutdown: &CancellationToken,
) -> BundledOutcome
where
    F: Fn(BundledAgent) -> Fut,
    Fut: Future<Output = Result<AgentInstanceId, PackageError>>,
{
    let name = agent.log_name();
    for attempt in 1..=MAX_ATTEMPTS {
        match call(agent).await {
            Ok(agent_instance_id) => {
                tracing::debug!(component = "desktop", agent = name, agent_instance_id = %agent_instance_id, "bundled_agent_ready");
                return BundledOutcome::Ready(agent_instance_id);
            }
            Err(error) if !error.retryable => {
                tracing::error!(
                    component = "desktop",
                    agent = name,
                    code = error.code,
                    "bundled_agent_unavailable"
                );
                return BundledOutcome::Unavailable(error.code);
            }
            Err(error) if attempt == MAX_ATTEMPTS => {
                tracing::error!(
                    component = "desktop",
                    agent = name,
                    code = error.code,
                    attempt,
                    "bundled_agent_unavailable"
                );
                return BundledOutcome::Unavailable(error.code);
            }
            Err(error) => {
                tracing::warn!(
                    component = "desktop",
                    agent = name,
                    code = error.code,
                    attempt,
                    "bundled_agent_unavailable"
                );
                tokio::select! {
                    () = shutdown.cancelled() => return BundledOutcome::Cancelled,
                    () = tokio::time::sleep(retry_after) => {}
                }
            }
        }
    }
    BundledOutcome::Cancelled
}

/// How one workspace's starter agents ended at this call (DSK-DOC-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarterOutcome {
    /// The instances added; empty when every starter agent was offered before.
    Done(Vec<AgentInstanceId>),
    /// The last error's code; tried again at the next start.
    Unavailable(&'static str),
    /// The shutdown token was cancelled between attempts.
    Cancelled,
}

/// DSK-DOC-1 (1): after the bundled agents, `packages.add_starter_agents` for each active workspace,
/// one at a time, in the active locale. The workspace's languages are passed as `agents.add` passes
/// them: the screens send none and the program adds none.
// spec: ordin-desktop/components/OrdinDesktop#DSK-DOC-1
pub async fn add_starter_agents_everywhere(
    packages: Arc<PackageCatalogue>,
    workspaces: Arc<WorkspaceService>,
    localisation: Arc<Localisation>,
    shutdown: CancellationToken,
) -> Vec<(WorkspaceId, StarterOutcome)> {
    let active = match workspaces.list(WorkspaceListFilter::Active).await {
        Ok(active) => active,
        Err(error) => {
            tracing::error!(
                component = "desktop",
                code = error.code(),
                "starter_agents_unavailable"
            );
            return Vec::new();
        }
    };
    let mut out = Vec::with_capacity(active.len());
    for workspace in active {
        let outcome = add_starter_agents(
            packages.clone(),
            localisation.clone(),
            workspace.id,
            &shutdown,
        )
        .await;
        let cancelled = outcome == StarterOutcome::Cancelled;
        out.push((workspace.id, outcome));
        if cancelled {
            break;
        }
    }
    out
}

/// DSK-DOC-1 (2): the starter agents for a workspace just created, in a task of its own, so the
/// command's reply does not wait for it.
// spec: ordin-desktop/components/OrdinDesktop#DSK-DOC-1
pub fn spawn_starter_agents(
    packages: Arc<PackageCatalogue>,
    localisation: Arc<Localisation>,
    workspace_id: WorkspaceId,
) {
    tokio::spawn(async move {
        // The task ends with the runtime; no token cancels it sooner.
        let shutdown = CancellationToken::new();
        add_starter_agents(packages, localisation, workspace_id, &shutdown).await
    });
}

/// `packages.add_starter_agents` for one workspace with DSK-REC-24's retry rule.
pub async fn add_starter_agents(
    packages: Arc<PackageCatalogue>,
    localisation: Arc<Localisation>,
    workspace_id: WorkspaceId,
    shutdown: &CancellationToken,
) -> StarterOutcome {
    let call = || {
        let packages = packages.clone();
        let locale = localisation.active().locale;
        async move {
            packages
                .add_starter_agents(workspace_id, Vec::new(), &locale)
                .await
        }
    };
    starter_with_retry(workspace_id, call, RETRY_AFTER, shutdown).await
}

/// DSK-REC-24's retry rule over `call` for one workspace: retryable → again after `retry_after`, at
/// most `MAX_ATTEMPTS`; permanent → logged `starter_agents_unavailable` at `error`.
pub async fn starter_with_retry<F, Fut>(
    workspace_id: WorkspaceId,
    call: F,
    retry_after: Duration,
    shutdown: &CancellationToken,
) -> StarterOutcome
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<Vec<AgentInstanceId>, PackageError>>,
{
    for attempt in 1..=MAX_ATTEMPTS {
        match call().await {
            Ok(added) => return StarterOutcome::Done(added),
            Err(error) if !error.retryable || attempt == MAX_ATTEMPTS => {
                tracing::error!(
                    component = "desktop",
                    workspace_id = %workspace_id,
                    code = error.code,
                    attempt,
                    "starter_agents_unavailable"
                );
                return StarterOutcome::Unavailable(error.code);
            }
            Err(error) => {
                tracing::warn!(
                    component = "desktop",
                    workspace_id = %workspace_id,
                    code = error.code,
                    attempt,
                    "starter_agents_unavailable"
                );
                tokio::select! {
                    () = shutdown.cancelled() => return StarterOutcome::Cancelled,
                    () = tokio::time::sleep(retry_after) => {}
                }
            }
        }
    }
    StarterOutcome::Cancelled
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Both agents, as a build with the `authoring` switch adds them.
    const BOTH: [BundledAgent; 2] = [BundledAgent::Ordin, BundledAgent::AgentMaker];

    // DSK-REC-24a: the agent maker is added only in a build with the authoring switch.
    // spec: ordin-desktop/components/OrdinDesktop#DSK-REC-24a
    #[test]
    fn the_agent_maker_comes_only_with_the_authoring_switch() {
        if cfg!(feature = "authoring") {
            assert_eq!(bundled_agents(), BOTH.to_vec());
        } else {
            assert_eq!(bundled_agents(), vec![BundledAgent::Ordin]);
        }
    }

    fn error(code: &'static str, retryable: bool) -> PackageError {
        // A PackageError with the code and flag the test needs, through its public fields.
        PackageError {
            code,
            retryable,
            action: None,
            detail: String::new(),
        }
    }

    // DSK-REC-24 step 1 and the order: Ordin first, then the agent maker.
    #[tokio::test]
    async fn both_agents_ready_in_order() {
        let calls = Mutex::new(Vec::new());
        let out = add_each(
            &BOTH,
            |agent| {
                calls.lock().unwrap().push(agent);
                async move { Ok(agent.instance_id()) }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            *calls.lock().unwrap(),
            vec![BundledAgent::Ordin, BundledAgent::AgentMaker]
        );
        assert_eq!(
            out,
            vec![
                (
                    BundledAgent::Ordin,
                    BundledOutcome::Ready(BundledAgent::Ordin.instance_id())
                ),
                (
                    BundledAgent::AgentMaker,
                    BundledOutcome::Ready(BundledAgent::AgentMaker.instance_id())
                ),
            ]
        );
    }

    // DSK-REC-24 steps 2 and 3: a retryable failure is tried again, at most 5 attempts; a permanent
    // one is not, and the task moves on to the next agent.
    #[tokio::test]
    async fn retryable_failures_are_tried_five_times_and_permanent_ones_once() {
        let calls = Mutex::new(Vec::new());
        let out = add_each(
            &BOTH,
            |agent| {
                calls.lock().unwrap().push(agent);
                async move {
                    match agent {
                        BundledAgent::Ordin => Err(error("PKG_BUNDLED_AGENT_MISSING", false)),
                        BundledAgent::AgentMaker => Err(error("PKG_LIBRARY_UNAVAILABLE", true)),
                    }
                }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        let calls = calls.lock().unwrap().clone();
        assert_eq!(
            calls.iter().filter(|a| **a == BundledAgent::Ordin).count(),
            1
        );
        assert_eq!(
            calls
                .iter()
                .filter(|a| **a == BundledAgent::AgentMaker)
                .count(),
            MAX_ATTEMPTS as usize
        );
        assert_eq!(
            out,
            vec![
                (
                    BundledAgent::Ordin,
                    BundledOutcome::Unavailable("PKG_BUNDLED_AGENT_MISSING")
                ),
                (
                    BundledAgent::AgentMaker,
                    BundledOutcome::Unavailable("PKG_LIBRARY_UNAVAILABLE")
                ),
            ]
        );
    }

    // DSK-REC-24 step 2: a retry that succeeds.
    #[tokio::test]
    async fn a_retry_that_succeeds() {
        let attempts = Mutex::new(0u32);
        let out = add_each(
            &BOTH,
            |agent| {
                let n = {
                    let mut a = attempts.lock().unwrap();
                    *a += 1;
                    *a
                };
                async move {
                    if n == 1 {
                        Err(error("PKG_CHECK_UNAVAILABLE", true))
                    } else {
                        Ok(agent.instance_id())
                    }
                }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(
            out[0].1,
            BundledOutcome::Ready(BundledAgent::Ordin.instance_id())
        );
        assert_eq!(*attempts.lock().unwrap(), 3);
    }

    // DSK-REC-24 step 4: a cancelled shutdown token ends the task between attempts.
    #[tokio::test]
    async fn a_cancelled_token_ends_the_task_between_attempts() {
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let out = add_each(
            &BOTH,
            |_| async { Err(error("PKG_LIBRARY_UNAVAILABLE", true)) },
            Duration::from_secs(3600),
            &shutdown,
        )
        .await;
        assert_eq!(out, vec![(BundledAgent::Ordin, BundledOutcome::Cancelled)]);
    }

    // DSK-DOC-1: a workspace's starter agents with DSK-REC-24's retry rule — a retryable failure
    // is tried again, at most 5 attempts; a permanent one once.
    #[tokio::test]
    async fn starter_agents_are_retried_like_the_bundled_agents() {
        let workspace = WorkspaceId::new();
        let attempts = Mutex::new(0u32);
        let out = starter_with_retry(
            workspace,
            || {
                *attempts.lock().unwrap() += 1;
                async { Err(error("PKG_LIBRARY_UNAVAILABLE", true)) }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(out, StarterOutcome::Unavailable("PKG_LIBRARY_UNAVAILABLE"));
        assert_eq!(*attempts.lock().unwrap(), MAX_ATTEMPTS);

        let attempts = Mutex::new(0u32);
        let out = starter_with_retry(
            workspace,
            || {
                *attempts.lock().unwrap() += 1;
                async { Err(error("PKG_STORE_FAILED", false)) }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(out, StarterOutcome::Unavailable("PKG_STORE_FAILED"));
        assert_eq!(*attempts.lock().unwrap(), 1);
    }

    // DSK-DOC-1: a retry that succeeds, and a cancelled token between attempts.
    #[tokio::test]
    async fn a_starter_retry_that_succeeds_and_one_cancelled() {
        let workspace = WorkspaceId::new();
        let added = AgentInstanceId::new();
        let attempts = Mutex::new(0u32);
        let out = starter_with_retry(
            workspace,
            || {
                let n = {
                    let mut a = attempts.lock().unwrap();
                    *a += 1;
                    *a
                };
                async move {
                    if n == 1 {
                        Err(error("PKG_CHECK_UNAVAILABLE", true))
                    } else {
                        Ok(vec![added])
                    }
                }
            },
            Duration::from_millis(1),
            &CancellationToken::new(),
        )
        .await;
        assert_eq!(out, StarterOutcome::Done(vec![added]));

        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let out = starter_with_retry(
            workspace,
            || async { Err(error("PKG_LIBRARY_UNAVAILABLE", true)) },
            Duration::from_secs(3600),
            &shutdown,
        )
        .await;
        assert_eq!(out, StarterOutcome::Cancelled);
    }
}
