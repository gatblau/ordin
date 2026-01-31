//! The Resolver and Compiler (ResolverAndCompiler.md).
//!
//! Turns one agent, a WorkspaceProfile with its practice packs and overlays, into a deterministic
//! bundle and its lock file. It picks open practice packs from the languages of the workspace's code,
//! otherwise from the packs the agent's maker fixed, and leaves a required choice open when the maker
//! left it for the agent to ask. It detects conflicting parts, rebuilds an agent with the replacement of
//! a withdrawn part, rebuilds a bundle from a lock file, composes the agent's resolved tool policy, and
//! compiles a package's always-on instructions into `AGENTS.md` text without writing anything.
//!
//! Parts are read only through the Agent Model's `PartSource`, bound late by the program with
//! `install_part_source`. The crate has no datastore: a bundle is a directory under `bundle_root`.
#![forbid(unsafe_code)]

mod closure;
mod conflict;
mod error;
mod held;
mod log;
mod policy;
mod request;
mod resolve;
mod select;
mod space;
mod store;

use std::collections::{BTreeSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use ordin_agent_model::{LockFile, PartSource, RepositoryRef, ResolutionPlan, SourceScope};

/// The Agent Model types, re-exported so the Release Builder reaches them through this crate (2B-3
/// gives the Release Builder no direct edge to `ordin-agent-model`).
pub use ordin_agent_model as model;

/// `SourceOverride` (a source used for one request in place of the bound one) and `StagedPart` (a
/// draft's new or changed part that shadows a source part of the same kind and identifier) are owned by
/// the Agent Model and re-exported here unchanged. A staged part's commit is written as forty `0`s in
/// plan and lock.
// spec: ordin-desktop/components/ResolverAndCompiler#SA-CLOSE-01
pub use ordin_agent_model::{SourceOverride, StagedPart};

pub use conflict::{Conflict, ConflictCandidate};
pub use error::{ResolveError, ALL_CODES};
pub use request::{
    CompiledInstructions, PackAnswer, PackSpace, PruneReport, Replacement, ResolveRequest,
    ResolverConfig,
};
pub use store::Bundle;

use held::Held;
use resolve::Resolved;

/// How many recent resolutions `compile` can find by plan hash.
const RECENT_RESOLUTIONS: usize = 8;

struct Recent {
    plan_hash: String,
    resolved: Arc<Resolved>,
    source: Arc<dyn PartSource>,
}

/// Resolve `request` against `source` (ignoring `request.source`) and return the always-on instructions
/// as `AGENTS.md` text. No `ResolverAndCompiler` and no bound source is needed.
pub async fn compile_instructions(
    source: &dyn PartSource,
    request: &ResolveRequest,
) -> Result<CompiledInstructions, ResolveError> {
    let outcome = async {
        let resolved = resolve::resolve_core(source, request).await?;
        resolve::instructions_of(source, &resolved).await
    }
    .await;
    if let Err(error) = &outcome {
        log::resolve_failed(&request.root_id, error);
    }
    outcome
}

pub struct ResolverAndCompiler {
    source: OnceLock<Arc<dyn PartSource>>,
    config: ResolverConfig,
    recent: Mutex<VecDeque<Recent>>,
}

impl ResolverAndCompiler {
    /// Built without a source (R18); every call that reads parts fails with `RES_SOURCE_NOT_BOUND` until
    /// `install_part_source` has run.
    pub fn new(config: ResolverConfig) -> Result<Self, ResolveError> {
        std::fs::create_dir_all(&config.bundle_root).map_err(|e| error::store_failed(&e))?;
        Ok(ResolverAndCompiler {
            source: OnceLock::new(),
            config,
            recent: Mutex::new(VecDeque::new()),
        })
    }

    /// Bind the source once, after the Catalogue Service is built. A second call fails with
    /// `RES_SOURCE_ALREADY_BOUND` and leaves the first source in place.
    // spec: ordin-desktop/components/ResolverAndCompiler#SA-RECON-01
    pub fn install_part_source(&self, source: Arc<dyn PartSource>) -> Result<(), ResolveError> {
        self.source
            .set(source)
            .map_err(|_| ResolveError::SourceAlreadyBound)
    }

    /// The source for one operation: the request's own, else the bound one.
    fn source_for(
        &self,
        override_source: Option<&SourceOverride>,
        operation: &str,
    ) -> Result<Arc<dyn PartSource>, ResolveError> {
        if let Some(source) = override_source {
            return Ok(Arc::clone(&source.0));
        }
        self.bound(operation)
    }

    /// The bound source only (`resolve_lock`, `rebuild_from_lock`).
    fn bound(&self, operation: &str) -> Result<Arc<dyn PartSource>, ResolveError> {
        match self.source.get() {
            Some(source) => Ok(Arc::clone(source)),
            None => {
                log::source_not_bound(operation);
                Err(ResolveError::SourceNotBound)
            }
        }
    }

    fn remember(&self, resolved: &Arc<Resolved>, source: &Arc<dyn PartSource>) {
        let mut recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|r| r.plan_hash != resolved.plan.plan_hash);
        recent.push_back(Recent {
            plan_hash: resolved.plan.plan_hash.clone(),
            resolved: Arc::clone(resolved),
            source: Arc::clone(source),
        });
        while recent.len() > RECENT_RESOLUTIONS {
            recent.pop_front();
        }
    }

    fn recall(&self, plan_hash: &str) -> Option<(Arc<Resolved>, Arc<dyn PartSource>)> {
        let recent = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        recent
            .iter()
            .find(|r| r.plan_hash == plan_hash)
            .map(|r| (Arc::clone(&r.resolved), Arc::clone(&r.source)))
    }

    async fn resolve_logged(
        &self,
        source: &Arc<dyn PartSource>,
        request: &ResolveRequest,
    ) -> Result<Arc<Resolved>, ResolveError> {
        match resolve::resolve_core(source.as_ref(), request).await {
            Ok(resolved) => {
                let resolved = Arc::new(resolved);
                self.remember(&resolved, source);
                Ok(resolved)
            }
            Err(error) => {
                log::resolve_failed(&request.root_id, &error);
                Err(error)
            }
        }
    }

    /// Resolve only: the plan, with `tool_policy` filled.
    pub async fn resolve(&self, request: &ResolveRequest) -> Result<ResolutionPlan, ResolveError> {
        let source = self.source_for(request.source.as_ref(), "resolve")?;
        let resolved = self.resolve_logged(&source, request).await?;
        Ok(resolved.plan.clone())
    }

    /// Compile a plan into a bundle on disk (reuses an existing bundle with the same plan hash).
    // `compile` takes a plan and no scope, but reading a part's files needs the part as the source
    // holds it. The parts of the resolution that produced the plan are kept for the last
    // `RECENT_RESOLUTIONS` resolutions and found by plan hash; a plan this resolver did not produce is
    // looked up in the bundled catalogue and in `staged`.
    pub async fn compile(
        &self,
        plan: &ResolutionPlan,
        staged: &[StagedPart],
    ) -> Result<Bundle, ResolveError> {
        if let Some((resolved, source)) = self.recall(&plan.plan_hash) {
            return self
                .compile_held(source.as_ref(), plan, &resolved.held)
                .await;
        }
        let source = self.bound("compile")?;
        let scope = SourceScope {
            home: RepositoryRef::Bundled,
            profile: Vec::new(),
            include_bundled: true,
        };
        let mut reader = held::Reader::new(source.as_ref(), &scope, staged, &[]);
        let mut parts: Vec<Held> = Vec::with_capacity(plan.resolved.len());
        for entry in &plan.resolved {
            let found = reader
                .lookup(Some(entry.kind), &entry.id, Some(&entry.version))
                .await?
                .ok_or_else(|| ResolveError::LockPartUnavailable {
                    part: entry.id.clone(),
                })?;
            parts.push(found);
        }
        self.compile_held(source.as_ref(), plan, &parts).await
    }

    async fn compile_held(
        &self,
        source: &dyn PartSource,
        plan: &ResolutionPlan,
        held: &[Held],
    ) -> Result<Bundle, ResolveError> {
        store::compile_held(
            &self.config.bundle_root,
            &self.config.compiler_version,
            source,
            plan,
            held,
        )
        .await
    }

    /// `resolve` then `compile`.
    pub async fn build(&self, request: &ResolveRequest) -> Result<Bundle, ResolveError> {
        let source = self.source_for(request.source.as_ref(), "build")?;
        let resolved = self.resolve_logged(&source, request).await?;
        self.compile_held(source.as_ref(), &resolved.plan, &resolved.held)
            .await
    }

    /// Read-only: the plan a lock file names, each part checked against the source; writes nothing.
    pub async fn resolve_lock(
        &self,
        lock: &LockFile,
        scope: &SourceScope,
    ) -> Result<ResolutionPlan, ResolveError> {
        let source = self.bound("resolve_lock")?;
        let resolved = resolve::resolve_lock_core(source.as_ref(), lock, scope).await?;
        Ok(resolved.plan)
    }

    /// `resolve_lock` then `compile`: rebuilds the bundle directory on disk (Installers' repair).
    pub async fn rebuild_from_lock(
        &self,
        lock: &LockFile,
        scope: &SourceScope,
    ) -> Result<Bundle, ResolveError> {
        let source = self.bound("rebuild_from_lock")?;
        let resolved = resolve::resolve_lock_core(source.as_ref(), lock, scope).await?;
        self.compile_held(source.as_ref(), &resolved.plan, &resolved.held)
            .await
    }

    /// The packs and overlays the root allows, for the Checker.
    pub async fn pack_space(&self, request: &ResolveRequest) -> Result<PackSpace, ResolveError> {
        let source = self.source_for(request.source.as_ref(), "pack_space")?;
        space::pack_space_core(source.as_ref(), request).await
    }

    /// Conflicts over every pair of parts that can be installed together (ADR-088).
    pub async fn conflicts(
        &self,
        request: &ResolveRequest,
        space: &PackSpace,
    ) -> Result<Vec<Conflict>, ResolveError> {
        let source = self.source_for(request.source.as_ref(), "conflicts")?;
        space::conflicts_core(source.as_ref(), request, space).await
    }

    /// An existing bundle by plan hash (`sha256:<hex>`).
    pub fn open_bundle(&self, plan_hash: &str) -> Result<Bundle, ResolveError> {
        store::open_bundle(&self.config.bundle_root, plan_hash)
    }

    /// Remove every bundle directory whose plan hash is not in `keep`.
    pub fn prune(&self, keep: &BTreeSet<String>) -> Result<PruneReport, ResolveError> {
        store::prune_root(&self.config.bundle_root, keep)
    }
}
