//! Change streams of the agent profile (Internal Logic N): one `GET /v1/knowledge/changes` kept
//! open per configured proxy, with the proxies' tables of contents refreshed on every move.

use std::sync::Arc;

use futures::StreamExt;
use ordin_types::domain::agent_profile::KnowledgeProxy;
use ordin_types::wire::knowledge_query::{KnowledgeContents, RepositoryMoved};
use sse_stream::SseByteStream;
use tokio_util::sync::CancellationToken;

use crate::client::{Inner, KnowledgeQueryClient, Mode, COMPONENT};
use crate::transport::{read_plain, ServiceTokenFailure};
use crate::types::*;

impl KnowledgeQueryClient {
    /// Opens one change stream per configured proxy and keeps each open; reports moves and refreshed
    /// contents to `listener` until `stop` is cancelled or the client is closed.
    pub fn start_change_streams(&self, listener: Arc<dyn ChangeListener>, stop: CancellationToken) {
        let Mode::Agent { proxies, .. } = &self.inner.mode else {
            return;
        };
        for proxy in proxies.clone() {
            let inner = Arc::clone(&self.inner);
            let listener = Arc::clone(&listener);
            let stop = stop.clone();
            let handle = tokio::spawn(stream_loop(inner, proxy, listener, stop));
            self.inner.track(handle);
        }
    }

    /// The tables of contents the proxies gave last; used only to choose proxies.
    pub fn proxy_contents(&self) -> Vec<KnowledgeContents> {
        match self.inner.proxy_contents.lock() {
            Ok(map) => {
                let mut names: Vec<&String> = map.keys().collect();
                names.sort();
                names
                    .into_iter()
                    .filter_map(|n| map.get(n).cloned())
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    }
}

impl Inner {
    /// A service token for one proxy; `None` when it cannot be had.
    async fn proxy_token(&self, proxy: &KnowledgeProxy) -> Option<String> {
        let Mode::Agent {
            service: Some(service),
            ..
        } = &self.mode
        else {
            return None;
        };
        match service.token_for(&self.transport, proxy).await {
            Ok(token) => Some(token.expose().to_owned()),
            Err(ServiceTokenFailure::Refused) => {
                tracing::error!(
                    component = COMPONENT,
                    agent = proxy.name.as_str(),
                    "kq_service_token_refused"
                );
                None
            }
            Err(ServiceTokenFailure::Unreachable) => None,
        }
    }

    async fn invalidate_proxy_token(&self, proxy: &KnowledgeProxy) {
        if let Mode::Agent {
            service: Some(service),
            ..
        } = &self.mode
        {
            service.invalidate(&proxy.name).await;
        }
    }

    /// `GET /v1/knowledge/contents` of one proxy; the answer replaces the cached one.
    async fn refresh_proxy_contents(
        &self,
        proxy: &KnowledgeProxy,
        listener: &Arc<dyn ChangeListener>,
    ) {
        let Some(token) = self.proxy_token(proxy).await else {
            return;
        };
        let base = proxy.url.trim_end_matches('/');
        let request = self
            .transport
            .client(base)
            .get(
                self.transport
                    .address(format!("{base}/v1/knowledge/contents")),
            )
            .bearer_auth(token)
            .timeout(self.timings.about_timeout);
        let Ok(plain) = read_plain(request).await else {
            return;
        };
        if plain.status == 401 {
            self.invalidate_proxy_token(proxy).await;
            return;
        }
        if plain.status != 200 {
            return;
        }
        let Ok(contents) = serde_json::from_slice::<KnowledgeContents>(&plain.body) else {
            return;
        };
        if let Ok(mut map) = self.proxy_contents.lock() {
            map.insert(proxy.name.clone(), contents.clone());
        }
        listener.contents_refreshed(&proxy.name, contents);
    }

    /// Opens the stream; `None` on any failure to get a 2xx answer.
    async fn open_changes(&self, proxy: &KnowledgeProxy) -> Option<reqwest::Response> {
        let token = self.proxy_token(proxy).await?;
        let base = proxy.url.trim_end_matches('/');
        let response = self
            .transport
            .client(base)
            .get(
                self.transport
                    .address(format!("{base}/v1/knowledge/changes")),
            )
            .bearer_auth(token)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .send()
            .await
            .ok()?;
        if response.status().as_u16() == 401 {
            self.invalidate_proxy_token(proxy).await;
            return None;
        }
        response.status().is_success().then_some(response)
    }
}

/// Reads events until the stream closes or `stop` is cancelled.
async fn read_changes(
    inner: &Arc<Inner>,
    proxy: &KnowledgeProxy,
    listener: &Arc<dyn ChangeListener>,
    stop: &CancellationToken,
    response: reqwest::Response,
) {
    let mut stream = Box::pin(SseByteStream::new(response.bytes_stream()));
    loop {
        let next = tokio::select! {
            _ = stop.cancelled() => return,
            _ = inner.shutdown.cancelled() => return,
            next = stream.next() => next,
        };
        let Some(Ok(event)) = next else { return };
        if event.event.as_deref() != Some("repository_moved") {
            continue;
        }
        let Some(moved) = event
            .data
            .as_deref()
            .and_then(|d| serde_json::from_str::<RepositoryMoved>(d).ok())
        else {
            continue;
        };
        listener.repository_moved(&proxy.name, moved);
        inner.refresh_proxy_contents(proxy, listener).await;
    }
}

async fn stream_loop(
    inner: Arc<Inner>,
    proxy: KnowledgeProxy,
    listener: Arc<dyn ChangeListener>,
    stop: CancellationToken,
) {
    let mut failures: u32 = 0;
    loop {
        if stop.is_cancelled() || inner.shutdown.is_cancelled() {
            return;
        }
        match inner.open_changes(&proxy).await {
            Some(response) => {
                // On (re)connect the contents are refreshed, so moves missed while closed show
                // through `last_commit`.
                inner.refresh_proxy_contents(&proxy, &listener).await;
                read_changes(&inner, &proxy, &listener, &stop, response).await;
                if stop.is_cancelled() || inner.shutdown.is_cancelled() {
                    return;
                }
                failures = 1;
            }
            None => failures += 1,
        }
        tracing::info!(
            component = COMPONENT,
            proxy = proxy.name.as_str(),
            attempt = failures,
            "kq_change_stream_closed"
        );
        if failures >= 2 {
            listener.proxy_unreachable(&proxy.name);
        }
        let backoff = &inner.timings.backoff;
        let at = (failures.saturating_sub(1) as usize).min(backoff.len().saturating_sub(1));
        let delay = backoff.get(at).copied().unwrap_or_default();
        tokio::select! {
            _ = stop.cancelled() => return,
            _ = inner.shutdown.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}
