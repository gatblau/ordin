//! The program's one outbound client (`CoreInputs.http`, SA-HTTP-02), built with the connect timeout
//! and the redirect rule OrganisationProfile.md step 2.3 puts on the client the program passes in
//! `ProfileMode::Read.http`: at most 3 redirects, and only to `https` addresses.

use std::time::Duration;

/// The connect timeout of the profile download (OrganisationProfile.md step 2.3).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Redirects followed at most (OrganisationProfile.md step 2.3).
pub const MAX_REDIRECTS: usize = 3;

/// Whether a redirect to `next`, after `followed` earlier redirects, is followed: refused when it is
/// not `https`, or when it would be the fourth.
pub fn redirect_allowed(next: &reqwest::Url, followed: usize) -> bool {
    next.scheme() == "https" && followed < MAX_REDIRECTS
}

/// The redirect policy that refuses any hop not to `https` and allows 3; a refused redirect ends
/// the request with an error, so no `http` address is ever requested. It replaces
/// `redirect::Policy::limited(3)` (plan 01 Phase 11, FU-016 L-2).
pub fn redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        if redirect_allowed(attempt.url(), attempt.previous().len().saturating_sub(1)) {
            attempt.follow()
        } else {
            attempt.error("redirect refused: not https, or more than 3")
        }
    })
}

/// The program's outbound client: connect timeout 10 s and [`redirect_policy`].
pub fn outbound_client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(redirect_policy())
        .build()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    #[test]
    fn only_https_hops_and_at_most_three() {
        let https = reqwest::Url::parse("https://profiles.example/acme.json").expect("url");
        let http = reqwest::Url::parse("http://profiles.example/acme.json").expect("url");
        assert!(redirect_allowed(&https, 0));
        assert!(redirect_allowed(&https, 2));
        assert!(!redirect_allowed(&https, 3), "a fourth redirect");
        assert!(!redirect_allowed(&http, 0), "an http hop");
    }

    /// A plain-HTTP server answering every request with `302 Location: <location>`; counts requests.
    async fn redirecting_server(location: String) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = format!("http://{}", listener.local_addr().expect("address"));
        let hits = Arc::new(AtomicUsize::new(0));
        let counted = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                counted.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let reply = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(reply.as_bytes()).await;
            }
        });
        (address, hits)
    }

    // Plan 01 Phase 11 exit: a chain through an `http` hop ending on `https` is refused, and the
    // `http` hop is never requested.
    #[tokio::test]
    async fn a_chain_through_an_http_hop_is_refused() {
        let (second, second_hits) =
            redirecting_server("https://profiles.example/acme.json".to_owned()).await;
        let (first, first_hits) = redirecting_server(format!("{second}/next")).await;
        let client = outbound_client().expect("client");
        let result = client.get(format!("{first}/profile")).send().await;
        let error = result.expect_err("refused");
        assert!(error.is_redirect(), "{error:?}");
        assert_eq!(first_hits.load(Ordering::SeqCst), 1);
        assert_eq!(
            second_hits.load(Ordering::SeqCst),
            0,
            "the http hop is never requested"
        );
    }
}
