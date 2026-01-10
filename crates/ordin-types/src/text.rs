//! Text normalisation and the length, name, path and URL checks of the 2C shared rule table
//! (InputValidation.md *Internal Logic* 3).

use icu::normalizer::ComposingNormalizerBorrowed;

use crate::TypesError;

/// The area-prefixed codes of one entity's name, path and URL rules (2C shared rule table).
#[derive(Debug, Clone, Copy)]
pub struct AreaCodes {
    pub name_empty: &'static str,    // e.g. "PLACE_NAME_INVALID"
    pub name_too_long: &'static str, // e.g. "PLACE_NAME_INVALID"
    pub path_invalid: &'static str,  // e.g. "WS_PATH_INVALID"
    pub url_invalid: &'static str,   // e.g. "CONF_URL_INVALID"
}

/// The longest local path the shared rule accepts, in bytes.
const MAX_PATH_BYTES: usize = 4096;
/// The longest URL the shared rule accepts, in bytes.
const MAX_URL_BYTES: usize = 2048;

/// NFC-normalises `raw` and refuses control characters other than `\n` and `\t`.
///
/// `\r` is refused too: a caller holding CRLF text converts it to `\n` before calling.
pub fn normalise_text(raw: &str) -> Result<String, TypesError> {
    if raw
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(TypesError::TextControlChar);
    }
    let nfc = ComposingNormalizerBorrowed::new_nfc();
    if nfc.is_normalized(raw) {
        // The already-normalised path allocates only the returned string.
        Ok(raw.to_owned())
    } else {
        Ok(nfc.normalize(raw).into_owned())
    }
}

/// Counts Unicode scalar values; refuses fewer than `min` or more than `max` with `code`.
pub fn check_len(
    value: &str,
    min: usize,
    max: usize,
    code: &'static str,
) -> Result<(), TypesError> {
    let count = value.chars().count();
    if count < min || count > max {
        return Err(TypesError::Rule { code });
    }
    Ok(())
}

/// Trims, NFC-normalises, then applies 1..=max scalar values (max 120 unless the entity says
/// otherwise). Returns the normalised value to store.
pub fn check_display_name(raw: &str, max: usize, codes: AreaCodes) -> Result<String, TypesError> {
    let value = normalise_text(raw.trim())?;
    let count = value.chars().count();
    if count == 0 {
        return Err(TypesError::Rule {
            code: codes.name_empty,
        });
    }
    if count > max {
        return Err(TypesError::Rule {
            code: codes.name_too_long,
        });
    }
    Ok(value)
}

/// Absolute path, 1–4096 bytes of UTF-8, no NUL byte.
pub fn check_abs_path(raw: &str, codes: AreaCodes) -> Result<(), TypesError> {
    let refused = TypesError::Rule {
        code: codes.path_invalid,
    };
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains('\0') {
        return Err(refused);
    }
    if !std::path::Path::new(raw).is_absolute() {
        return Err(refused);
    }
    Ok(())
}

/// Absolute `https` URL, at most 2048 bytes; `allow_loopback_http` admits `http://` to
/// `localhost` and `127.0.0.1` only (2D-1a `ORDIN_SERVER_URL`, `ORDIN_OTLP_ENDPOINT`).
pub fn check_https_url(
    raw: &str,
    allow_loopback_http: bool,
    codes: AreaCodes,
) -> Result<url::Url, TypesError> {
    let refused = TypesError::Rule {
        code: codes.url_invalid,
    };
    if raw.len() > MAX_URL_BYTES {
        return Err(refused);
    }
    let parsed = url::Url::parse(raw).map_err(|_| refused.clone())?;
    let Some(host) = parsed.host_str() else {
        return Err(refused);
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(refused);
    }
    let scheme_allowed = match parsed.scheme() {
        "https" => true,
        "http" => allow_loopback_http && (host == "localhost" || host == "127.0.0.1"),
        _ => false,
    };
    if !scheme_allowed {
        return Err(refused);
    }
    Ok(parsed)
}
