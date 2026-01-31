//! In-process authentication (*Internal Logic* §2, M-4): a bearer valid for one call.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use memory::auth::{AuthError, CallerAuthenticator};
use memory::service::CallContext;
use memory::types::scope::ScopeContext;
use ordin_types::encoding::b64url_encode;
use tokio::sync::RwLock;

use crate::error::MemoryError;

type Contexts = Arc<RwLock<HashMap<String, ScopeContext>>>;

/// SA-RECON-01: the contexts of the calls in progress.
// spec: ordin-desktop/components/MemoryService#SA-RECON-01
#[derive(Default)]
pub(crate) struct InProcessAuthenticator {
    contexts: Contexts,
}

impl InProcessAuthenticator {
    /// Registers `context` under a new bearer of 32 random bytes in base64url. The bearer is gone
    /// when the guard drops. No context is ever built from tool arguments.
    pub(crate) async fn mint(&self, mut context: ScopeContext) -> Result<BearerGuard, MemoryError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|_| MemoryError::internal())?;
        let bearer = b64url_encode(&bytes);
        context.token_jti = bearer.chars().take(8).collect();
        self.contexts.write().await.insert(bearer.clone(), context);
        Ok(BearerGuard {
            bearer,
            contexts: self.contexts.clone(),
        })
    }

    #[cfg(test)]
    pub(crate) async fn live_bearers(&self) -> usize {
        self.contexts.read().await.len()
    }
}

#[async_trait]
impl CallerAuthenticator for InProcessAuthenticator {
    async fn validate(
        &self,
        bearer: &str,
        correlation_id: &str,
    ) -> Result<ScopeContext, AuthError> {
        let contexts = self.contexts.read().await;
        let mut context = contexts
            .get(bearer)
            .cloned()
            .ok_or_else(|| AuthError::InvalidToken("unknown in-process bearer".to_owned()))?;
        context.correlation_id = correlation_id.to_owned();
        Ok(context)
    }
}

/// Removes the bearer from the map on drop, also when the call panics or is cancelled.
pub(crate) struct BearerGuard {
    bearer: String,
    contexts: Contexts,
}

impl BearerGuard {
    pub(crate) fn cx<'a>(
        &'a self,
        correlation_id: &'a str,
        idempotency_key: Option<&'a str>,
    ) -> CallContext<'a> {
        CallContext {
            bearer: &self.bearer,
            correlation_id,
            idempotency_key,
        }
    }
}

impl Drop for BearerGuard {
    fn drop(&mut self) {
        let bearer = std::mem::take(&mut self.bearer);
        match self.contexts.try_write() {
            Ok(mut contexts) => {
                contexts.remove(&bearer);
            }
            Err(_) => {
                let contexts = self.contexts.clone();
                if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                    runtime.spawn(async move {
                        contexts.write().await.remove(&bearer);
                    });
                }
            }
        }
    }
}
