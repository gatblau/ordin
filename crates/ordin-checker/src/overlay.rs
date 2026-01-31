//! A source that reads as if a proposed withdrawal were already published (step 27): every agent that
//! uses the part is rebuilt with the withdrawal in force, so the Resolver substitutes the replacement.

use std::sync::Arc;

use ordin_agent_model::{
    PartFiles, PartKind, PartSource, PartSourceError, RepositoryRef, Resolution, SourceScope,
    SourcedPart, VocabularyAxis, VocabularySet, WithdrawnListEntry,
};

pub(crate) struct WithdrawalInForce {
    pub inner: Arc<dyn PartSource>,
    pub entry: WithdrawnListEntry,
}

#[async_trait::async_trait]
impl PartSource for WithdrawalInForce {
    async fn resolve(
        &self,
        scope: &SourceScope,
        kind: PartKind,
        id: &str,
        version: Option<&str>,
    ) -> Result<Resolution, PartSourceError> {
        self.inner.resolve(scope, kind, id, version).await
    }

    async fn resolve_any_kind(
        &self,
        scope: &SourceScope,
        id: &str,
    ) -> Result<Resolution, PartSourceError> {
        self.inner.resolve_any_kind(scope, id).await
    }

    async fn files(&self, part: &SourcedPart) -> Result<PartFiles, PartSourceError> {
        self.inner.files(part).await
    }

    async fn ingredients(&self, scope: &SourceScope) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.ingredients(scope).await
    }

    async fn withdrawn(
        &self,
        scope: &SourceScope,
    ) -> Result<Vec<WithdrawnListEntry>, PartSourceError> {
        let mut entries = self.inner.withdrawn(scope).await?;
        entries.push(self.entry.clone());
        Ok(entries)
    }

    async fn repository_parts(
        &self,
        repository: &RepositoryRef,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.repository_parts(repository).await
    }

    async fn vocabulary(&self, scope: &SourceScope) -> Result<VocabularySet, PartSourceError> {
        self.inner.vocabulary(scope).await
    }

    async fn used_by(&self, part: &SourcedPart) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.used_by(part).await
    }

    async fn citing_parts(
        &self,
        repository: &RepositoryRef,
        axis: VocabularyAxis,
        code: &str,
    ) -> Result<Vec<SourcedPart>, PartSourceError> {
        self.inner.citing_parts(repository, axis, code).await
    }
}
