//! *Verify* (Internal Logic 3): the detached JWS (RFC 7515 Appendix F, `alg` `EdDSA`), then the
//! format version, the strict parse, the bounds and the version order. Nothing in the document is
//! parsed before the signature verifies (AuthenticationAndAuthorisation.md, *Organisation profile
//! signature*).

use ed25519_dalek::{Signature, VerifyingKey};
use ordin_types::encoding::{b64url_decode_exact, b64url_encode};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::document::OrganisationProfileDocument;
use crate::error::ProfileError;
use crate::validate::{validate, version_parts};

/// One installer key: its raw 32 bytes as a verifying key and its `kid`.
#[derive(Debug, Clone)]
pub(crate) struct InstallerKey {
    pub(crate) kid: String,
    pub(crate) key: VerifyingKey,
}

/// Decodes one installer key (base64url without padding, exactly 32 bytes, a valid Ed25519 point).
pub(crate) fn decode_key(raw: &str) -> Result<InstallerKey, ProfileError> {
    let bytes = b64url_decode_exact(raw, 32, "PROF_INSTALLER_INVALID")
        .map_err(|_| ProfileError::InstallerInvalid)?;
    let mut array = [0u8; 32];
    array.copy_from_slice(&bytes);
    let key = VerifyingKey::from_bytes(&array).map_err(|_| ProfileError::InstallerInvalid)?;
    Ok(InstallerKey {
        kid: kid_of(&array),
        key,
    })
}

/// A key's `kid`: the first 16 hex characters of SHA-256 of its raw 32 bytes.
pub(crate) fn kid_of(raw: &[u8; 32]) -> String {
    let digest: [u8; 32] = Sha256::digest(raw).into();
    ordin_types::num::Sha256Hex::from_digest(&digest).0[..16].to_owned()
}

/// The header keys this build accepts; any other key (including `b64`) is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // read so that a non-string `typ` is refused; its value is not used
    typ: Option<String>,
}

/// What *verify* returns: the parsed, bounded document.
pub(crate) struct Verified {
    pub(crate) document: OrganisationProfileDocument,
}

/// The format version a document declares, read alone before the strict parse (step 3.5).
#[derive(Deserialize)]
struct FormatProbe {
    format_version: serde_json::Value,
}

/// Steps 3.1–3.7. `active_version` is `None` to skip the order check (the cached profile at start).
/// On a failure after the parse, the parsed `profile_version` is returned beside the error so that the
/// refused row can record it.
pub(crate) fn verify(
    document: &[u8],
    jws: &str,
    keys: &[InstallerKey],
    active_version: Option<&str>,
) -> Result<Verified, (ProfileError, Option<String>)> {
    check_signature(document, jws, keys).map_err(|e| (e, None))?;
    let parsed = parse(document).map_err(|e| (e, None))?;
    let version = Some(parsed.profile_version.clone());
    validate(&parsed).map_err(|e| (e, version.clone()))?;
    if let Some(active) = active_version {
        // Step 3.7: strictly greater, component-wise; equal is refused too.
        let newer = matches!(
            (version_parts(&parsed.profile_version), version_parts(active)),
            (Some(new), Some(old)) if new > old
        );
        // An active row whose version does not parse cannot be compared: any valid version is newer.
        let newer = newer || version_parts(active).is_none();
        if !newer {
            return Err((ProfileError::VersionNotNewer, version));
        }
    }
    Ok(Verified { document: parsed })
}

/// Steps 3.1–3.4.
fn check_signature(document: &[u8], jws: &str, keys: &[InstallerKey]) -> Result<(), ProfileError> {
    let invalid = || ProfileError::SignatureInvalid;
    // 3.1 `<header>..<signature>`.
    let parts: Vec<&str> = jws.trim_end_matches(['\r', '\n']).split('.').collect();
    let [header_segment, "", signature_segment] = parts.as_slice() else {
        return Err(invalid());
    };
    let header_bytes = decode_any(header_segment).ok_or_else(invalid)?;
    let signature_bytes = b64url_decode_exact(signature_segment, 64, "PROF_SIGNATURE_INVALID")
        .map_err(|_| invalid())?;
    // 3.2 header.
    let header: Header = serde_json::from_slice(&header_bytes).map_err(|_| invalid())?;
    if header.alg != "EdDSA" {
        return Err(invalid());
    }
    if let Some(kid) = &header.kid {
        let hex = kid.len() == 16
            && kid
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            return Err(invalid());
        }
    }
    // 3.3 key.
    let candidates: Vec<&InstallerKey> = match &header.kid {
        Some(kid) => {
            let found: Vec<&InstallerKey> = keys.iter().filter(|k| &k.kid == kid).collect();
            if found.is_empty() {
                return Err(ProfileError::KeyUnknown);
            }
            found
        }
        None => keys.iter().collect(),
    };
    // 3.4 signature over `<header segment>.<base64url(document)>`.
    let mut array = [0u8; 64];
    array.copy_from_slice(&signature_bytes);
    let signature = Signature::from_bytes(&array);
    let signing_input = format!("{header_segment}.{}", b64url_encode(document));
    if candidates.iter().any(|k| {
        k.key
            .verify_strict(signing_input.as_bytes(), &signature)
            .is_ok()
    }) {
        Ok(())
    } else {
        Err(invalid())
    }
}

/// base64url without padding, at least one byte: `n` characters carry `n * 3 / 4` bytes.
fn decode_any(segment: &str) -> Option<Vec<u8>> {
    if segment.is_empty() {
        return None;
    }
    let len = segment.len() * 3 / 4;
    b64url_decode_exact(segment, len, "PROF_SIGNATURE_INVALID").ok()
}

/// Step 3.5: UTF-8 JSON; `format_version` first, then the strict parse with the serde key path.
fn parse(document: &[u8]) -> Result<OrganisationProfileDocument, ProfileError> {
    let text = std::str::from_utf8(document).map_err(|_| ProfileError::content(""))?;
    let probe: FormatProbe =
        serde_json::from_str(text).map_err(|_| ProfileError::content("format_version"))?;
    match probe.format_version.as_u64() {
        Some(v) if v == u64::from(crate::FORMAT_VERSION) => {}
        Some(_) => return Err(ProfileError::FormatUnsupported),
        None => return Err(ProfileError::content("format_version")),
    }
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let parsed: OrganisationProfileDocument =
        serde_path_to_error::deserialize(&mut deserializer)
            .map_err(|e| ProfileError::content(key_path_of(&e)))?;
    deserializer.end().map_err(|_| ProfileError::content(""))?;
    Ok(parsed)
}

/// The dotted key path of a parse failure (an unknown key's path ends with the key itself); the
/// document root is the empty path.
fn key_path_of(error: &serde_path_to_error::Error<serde_json::Error>) -> String {
    let path = error.path().to_string();
    if path == "." {
        String::new()
    } else {
        path
    }
}
