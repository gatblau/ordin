//! Internal Logic C1: downloading one asset with the shared client, bounded retries and a size cap.

use std::hash::{BuildHasher as _, Hasher as _};
use std::time::Duration;

use ordin_types::text::{check_https_url, AreaCodes};

use crate::check::{AssetSlot, Refusal};

/// The waits and bounds of fetching and re-checking (production values in [`Timing::production`]).
#[derive(Debug, Clone)]
pub(crate) struct Timing {
    /// 30 s per asset (C1).
    pub fetch_timeout: Duration,
    /// The waits after the first and second failed attempt: 2 s and 8 s (C1).
    pub backoff: [Duration; 2],
    /// Up to 1 s of jitter added to each wait (C1).
    pub jitter_max: Duration,
    /// A `Retry-After` up to 60 s is honoured (C1).
    pub retry_after_max: Duration,
    /// A pack left `received` by a failed fetch is checked again after 15 minutes (C1).
    pub recheck_after: Duration,
}

impl Timing {
    pub(crate) fn production() -> Timing {
        Timing {
            fetch_timeout: Duration::from_secs(30),
            backoff: [Duration::from_secs(2), Duration::from_secs(8)],
            jitter_max: Duration::from_secs(1),
            retry_after_max: Duration::from_secs(60),
            recheck_after: Duration::from_secs(15 * 60),
        }
    }
}

/// At most 3 attempts per asset (C1).
pub(crate) const ATTEMPTS: u32 = 3;

/// Unit tests reach the `wiremock` stand-in over plain HTTP on loopback; every build that ships
/// accepts `https` addresses only (Security: "only `https`").
const ALLOW_LOOPBACK_HTTP: bool = cfg!(test);

/// The codes a URL check reports; only `url_invalid` can be returned by `check_https_url`.
const URL_CODES: AreaCodes = AreaCodes {
    name_empty: "BRAND_ASSET_INVALID",
    name_too_long: "BRAND_ASSET_INVALID",
    path_invalid: "BRAND_ASSET_INVALID",
    url_invalid: "BRAND_ASSET_INVALID",
};

/// Why a fetch did not produce a body.
#[derive(Debug)]
pub(crate) enum FetchError {
    /// Retries exhausted on network errors, 5xx or 429 (`BRAND_ASSET_FETCH_FAILED`).
    Failed {
        http_status: Option<u16>,
        attempt: u32,
    },
    /// HTTP 4xx other than 429, a body over its cap, or a URL that is not `https`.
    Refused(Refusal),
}

/// One outcome of one attempt.
enum Attempt {
    Body(Vec<u8>),
    Retry {
        http_status: Option<u16>,
        retry_after: Option<Duration>,
    },
    Refused(Refusal),
}

/// A fraction in `[0, 1)` from the process's random hasher seed (no extra dependency).
fn unit_random() -> f64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(0);
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

fn retry_after(response: &reqwest::Response, max: Duration) -> Option<Duration> {
    let seconds: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    let wait = Duration::from_secs(seconds);
    (wait <= max).then_some(wait)
}

async fn attempt(
    http: &reqwest::Client,
    url: &reqwest::Url,
    slot: AssetSlot,
    timing: &Timing,
) -> Attempt {
    let response = match http
        .get(url.clone())
        .timeout(timing.fetch_timeout)
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            return Attempt::Retry {
                http_status: None,
                retry_after: None,
            }
        }
    };
    let status = response.status();
    if status.as_u16() == 429 || status.is_server_error() {
        return Attempt::Retry {
            http_status: Some(status.as_u16()),
            retry_after: retry_after(&response, timing.retry_after_max),
        };
    }
    if !status.is_success() {
        return Attempt::Refused(slot_invalid(slot));
    }
    let mut response = response;
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > slot.max_bytes() {
                    return Attempt::Refused(slot_invalid(slot));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Attempt::Body(body),
            Err(_) => {
                return Attempt::Retry {
                    http_status: Some(status.as_u16()),
                    retry_after: None,
                }
            }
        }
    }
}

/// The `AssetInvalid` refusal of a slot (the same sentence C2 uses).
fn slot_invalid(slot: AssetSlot) -> Refusal {
    slot.invalid()
}

/// C1: `GET url`, at most [`ATTEMPTS`] attempts with the backoff of `timing`.
pub(crate) async fn fetch(
    http: &reqwest::Client,
    url: &str,
    slot: AssetSlot,
    timing: &Timing,
) -> Result<Vec<u8>, FetchError> {
    let Ok(url) = check_https_url(url, ALLOW_LOOPBACK_HTTP, URL_CODES) else {
        return Err(FetchError::Refused(slot_invalid(slot)));
    };
    let mut last_status = None;
    for number in 1..=ATTEMPTS {
        match attempt(http, &url, slot, timing).await {
            Attempt::Body(body) => return Ok(body),
            Attempt::Refused(refusal) => return Err(FetchError::Refused(refusal)),
            Attempt::Retry {
                http_status,
                retry_after,
            } => {
                last_status = http_status;
                if number < ATTEMPTS {
                    let index = usize::try_from(number - 1).unwrap_or(0).min(1);
                    let jitter = timing.jitter_max.mul_f64(unit_random());
                    let wait = retry_after.unwrap_or(timing.backoff[index] + jitter);
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }
    Err(FetchError::Failed {
        http_status: last_status,
        attempt: ATTEMPTS,
    })
}
