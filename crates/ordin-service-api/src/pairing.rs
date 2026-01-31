//! The body types of the pairing route (2C wire *Names* item 6; *chosen here*, closes 2C G-2). The
//! route itself is the Remote API's; the Remote API (server side) and the Server Connector (desktop
//! side) share these types.

use ordin_types::num::Sha256Hex;
use ordin_types::{BackendId, DesktopPairingId};
use serde::{Deserialize, Serialize};

use crate::api::InputRejection;
use crate::commands::rules;
use crate::wire::ServiceApiVersion;

/// The pairing-code alphabet: 32 symbols, no `I`, `O`, `0`, `1` (*chosen here*: the person may type
/// it, and the four look-alike symbols are left out).
pub const CODE_ALPHABET: &str = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
/// Symbols in a pairing code (40 bits).
const CODE_LEN: usize = 8;
/// The line form's prefix and version.
const LINE_PREFIX: &str = "ordin-pair:1:";

/// Body of `POST /v1/pairing`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct PairRequest {
    pub api_version: ServiceApiVersion,
    /// Normalised pairing code: 8 characters of `[A-HJ-NP-Z2-9]`, no hyphen.
    pub code: String,
    /// The desktop computer's name, 1–63 characters ("Asha's laptop").
    pub computer_name: String,
}

/// `kind: "ok"` result of `POST /v1/pairing`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct PairReply {
    pub desktop_pairing_id: DesktopPairingId,
    /// 32 random bytes, base64url without padding (43 characters). Sent once, never again.
    pub desktop_key: String,
    pub backend: BackendId,
    /// The server's display name, 1–120 characters ("Northwind server").
    pub server_name: String,
    pub api_version: ServiceApiVersion,
}

/// The one line the setup command prints when the server's certificate is not one the desktop
/// already trusts (ADR-096 item 5, OQ-61):
/// `ordin-pair:1:<host>:<port>:<sha256 of the leaf certificate, 64 lowercase hex>:<code>`, with an
/// IPv6 host in brackets, e.g. `ordin-pair:1:10.20.4.17:8443:9f2c…e41a:K7QM2XPA`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingLine {
    pub host: String,
    pub port: u16,
    pub fingerprint: Sha256Hex,
    pub code: String,
}

fn invalid() -> InputRejection {
    InputRejection {
        code: rules::INPUT_INVALID,
        at: String::new(),
    }
}

impl PairingLine {
    /// Parses the line form; any deviation → `SVC_INPUT_INVALID`.
    pub fn parse(raw: &str) -> Result<PairingLine, InputRejection> {
        let rest = raw.strip_prefix(LINE_PREFIX).ok_or_else(invalid)?;
        // The last three fields never contain `:`; the host may (IPv6 in brackets).
        let mut tail = rest.rsplitn(4, ':');
        let code = tail.next().ok_or_else(invalid)?;
        let fingerprint = tail.next().ok_or_else(invalid)?;
        let port = tail.next().ok_or_else(invalid)?;
        let host_part = tail.next().ok_or_else(invalid)?;
        let host = match host_part.strip_prefix('[') {
            Some(inner) => {
                let inner = inner.strip_suffix(']').ok_or_else(invalid)?;
                match inner.parse::<std::net::IpAddr>() {
                    Ok(std::net::IpAddr::V6(_)) => inner.to_owned(),
                    _ => return Err(invalid()),
                }
            }
            None if host_part.contains(':') => return Err(invalid()),
            None if rules::is_host(host_part) => host_part.to_owned(),
            None => return Err(invalid()),
        };
        let port: u16 = port.parse().map_err(|_| invalid())?;
        if port < 1025 {
            return Err(invalid());
        }
        let fingerprint = Sha256Hex::parse(fingerprint).map_err(|_| invalid())?;
        if code.len() != CODE_LEN || !code.chars().all(|c| CODE_ALPHABET.contains(c)) {
            return Err(invalid());
        }
        Ok(PairingLine {
            host,
            port,
            fingerprint,
            code: code.to_owned(),
        })
    }

    /// The line form; an IPv6 host is written in brackets.
    pub fn format(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!(
            "{LINE_PREFIX}{host}:{}:{}:{}",
            self.port, self.fingerprint.0, self.code
        )
    }
}

/// Uppercases, removes spaces and one hyphen, and refuses any character outside the alphabet or a
/// length other than 8. Returns the 8-character code.
pub fn normalise_code(raw: &str) -> Result<String, InputRejection> {
    let mut hyphens = 0;
    let mut code = String::with_capacity(CODE_LEN);
    for c in raw.chars() {
        match c {
            ' ' => {}
            '-' => {
                hyphens += 1;
                if hyphens > 1 {
                    return Err(invalid());
                }
            }
            c => {
                let upper = c.to_ascii_uppercase();
                if !CODE_ALPHABET.contains(upper) {
                    return Err(invalid());
                }
                code.push(upper);
            }
        }
    }
    if code.len() != CODE_LEN {
        return Err(invalid());
    }
    Ok(code)
}

/// Displays a code as `ABCD-EFGH`.
pub fn display_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    if chars.len() != CODE_LEN {
        return code.to_owned();
    }
    let (left, right) = chars.split_at(CODE_LEN / 2);
    format!(
        "{}-{}",
        left.iter().collect::<String>(),
        right.iter().collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "9f2c0000000000000000000000000000000000000000000000000000000ee41a";

    #[test]
    fn codes() {
        assert_eq!(normalise_code("k7qm-2xpa").unwrap(), "K7QM2XPA");
        assert_eq!(normalise_code(" K7QM 2XPA ").unwrap(), "K7QM2XPA");
        assert!(normalise_code("K7QM-2X-PA").is_err());
        assert!(normalise_code("K7QM2XP").is_err());
        assert!(normalise_code("K7QM2XP0").is_err());
        assert!(normalise_code("K7QM2XPI").is_err());
        assert_eq!(display_code("K7QM2XPA"), "K7QM-2XPA");
        assert_eq!(display_code("short"), "short");
        assert_eq!(normalise_code("x").unwrap_err().code, "SVC_INPUT_INVALID");
    }

    #[test]
    fn lines_round_trip() {
        for raw in [
            format!("ordin-pair:1:10.20.4.17:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:[fe80::1]:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:ordin.example.org:65535:{FP}:K7QM2XPA"),
        ] {
            let line = PairingLine::parse(&raw).expect(&raw);
            assert_eq!(line.format(), raw);
        }
    }

    #[test]
    fn lines_refused() {
        for raw in [
            format!("ordin-pair:2:10.20.4.17:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:10.20.4.17:80:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:10.20.4.17:8443:{FP}:K7QM2XP"),
            "ordin-pair:1:10.20.4.17:8443:abc:K7QM2XPA".to_owned(),
            format!("ordin-pair:1:fe80::1:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:[10.0.0.1]:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:[fe80::1:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:bad_host:8443:{FP}:K7QM2XPA"),
            format!("ordin-pair:1:h:x:{FP}:K7QM2XPA"),
            "ordin-pair:1:".to_owned(),
        ] {
            assert!(PairingLine::parse(&raw).is_err(), "{raw}");
        }
    }
}
