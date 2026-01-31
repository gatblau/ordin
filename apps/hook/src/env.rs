//! B. The hook environment (2D-1c): `ORDIN_HOOK_ENDPOINT`, `ORDIN_HOOK_SECRET`, `ORDIN_SESSION_ID`,
//! `ORDIN_HOOK_PROTOCOL`.

use ordin_types::encoding::b64url_decode_exact;
use ordin_types::RunId;

use crate::error::{HookFailure, ENV_INVALID};

/// The longest socket path macOS accepts (Ordin Hook B).
pub const MACOS_ENDPOINT_MAX: usize = 104;
const PIPE_PREFIX: &str = r"\\.\pipe\";

pub struct HookEnv {
    pub endpoint: String,
    pub secret: [u8; 32],
    pub session: RunId,
    pub protocol: u16,
}

/// A lookup of environment variables, so tests inject their own.
pub type Lookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// True for an absolute socket path (within the macOS limit on macOS) or a Windows pipe name.
pub fn endpoint_ok(endpoint: &str) -> bool {
    if endpoint.starts_with(PIPE_PREFIX) {
        return endpoint.len() > PIPE_PREFIX.len();
    }
    if !endpoint.starts_with('/') || endpoint.contains('\0') {
        return false;
    }
    !cfg!(target_os = "macos") || endpoint.len() <= MACOS_ENDPOINT_MAX
}

pub fn read(get: Lookup<'_>) -> Result<HookEnv, HookFailure> {
    let fail = HookFailure::env_invalid;
    let endpoint = get("ORDIN_HOOK_ENDPOINT").ok_or_else(fail)?;
    if !endpoint_ok(&endpoint) {
        return Err(fail());
    }
    let raw_secret = get("ORDIN_HOOK_SECRET").ok_or_else(fail)?;
    let decoded = b64url_decode_exact(&raw_secret, 32, ENV_INVALID).map_err(|_| fail())?;
    let secret: [u8; 32] = decoded.try_into().map_err(|_| fail())?;
    let session = RunId::parse(&get("ORDIN_SESSION_ID").ok_or_else(fail)?).map_err(|_| fail())?;
    if get("ORDIN_HOOK_PROTOCOL").as_deref() != Some("1") {
        return Err(fail());
    }
    Ok(HookEnv {
        endpoint,
        secret,
        session,
        protocol: 1,
    })
}
