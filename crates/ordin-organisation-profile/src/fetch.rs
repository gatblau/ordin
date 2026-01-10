//! The two GETs of a refresh (Internal Logic 2.3 and 2.4), with the program's HTTP client.

use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, ETAG, IF_NONE_MATCH};
use reqwest::StatusCode;

use crate::error::ProfileError;
use crate::COMPONENT;

/// Total timeout of one request (Internal Logic 2.3).
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Body caps: the document and the signature.
pub(crate) const DOCUMENT_CAP: usize = 1_048_576;
pub(crate) const SIGNATURE_CAP: usize = 8_192;

/// What one GET produced.
pub(crate) enum Fetched {
    NotModified,
    Body {
        bytes: Vec<u8>,
        etag: Option<String>,
    },
}

/// One capped GET.
///
// SPEC-GAP-5: the spec fixes a 10 s connect timeout and "at most 3 redirects and only to https
// addresses", but the program passes the `reqwest::Client` (ProfileMode::Read.http), and reqwest sets
// the connect timeout and the redirect policy only when a client is built. This crate sets the 30 s
// total timeout on each request and refuses a response whose final address is not `https`
// (`PROF_UNREACHABLE`); the program must build its client with `connect_timeout(10 s)` and
// `redirect::Policy::limited(3)` for the rest of the rule to hold.
pub(crate) async fn get_capped(
    http: &reqwest::Client,
    url: &str,
    accept_json: bool,
    if_none_match: Option<&str>,
    cap: usize,
) -> Result<Fetched, ProfileError> {
    let started = Instant::now();
    let mut request = http.get(url).timeout(TOTAL_TIMEOUT);
    if accept_json {
        request = request.header(ACCEPT, "application/json");
    }
    if let Some(etag) = if_none_match {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let mut response = request
        .send()
        .await
        .map_err(|_| failed(ProfileError::Unreachable))?;
    if crate::installer::check_address(response.url().as_str()).is_err() {
        return Err(failed(ProfileError::Unreachable));
    }
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        done(status, 0, started);
        return Ok(Fetched::NotModified);
    }
    if status != StatusCode::OK {
        return Err(failed(ProfileError::HttpStatus {
            status: status.as_u16(),
        }));
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if response
        .content_length()
        .is_some_and(|len| len > cap as u64)
    {
        return Err(failed(ProfileError::TooLarge));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| failed(ProfileError::Unreachable))?
    {
        if bytes.len() + chunk.len() > cap {
            return Err(failed(ProfileError::TooLarge));
        }
        bytes.extend_from_slice(&chunk);
    }
    done(status, bytes.len(), started);
    Ok(Fetched::Body { bytes, etag })
}

fn done(status: StatusCode, bytes: usize, started: Instant) {
    tracing::info!(
        component = COMPONENT,
        http_status = status.as_u16(),
        bytes = bytes as u64,
        duration_ms = started.elapsed().as_millis() as u64,
        "profile.fetch.done"
    );
}

/// Logs `profile.fetch.failed` and returns the error.
fn failed(error: ProfileError) -> ProfileError {
    use ordin_types::CodedError as _;
    match &error {
        ProfileError::HttpStatus { status } => tracing::warn!(
            component = COMPONENT,
            code = error.code(),
            http_status = *status,
            "profile.fetch.failed"
        ),
        _ => tracing::warn!(
            component = COMPONENT,
            code = error.code(),
            "profile.fetch.failed"
        ),
    }
    error
}
