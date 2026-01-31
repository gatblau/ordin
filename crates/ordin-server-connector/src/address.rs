//! Parsing and checking of a server's address and port (ServerConnector.md Shared Context, field
//! rules of `ServerConnection`; Internal Logic 3.1).

use std::net::IpAddr;

use crate::error::ConnectorError;

/// The port used when the person types none (Constants).
pub(crate) const DEFAULT_PORT: u16 = 8443;
/// The lowest and highest port a server may listen on (field rule `port`).
pub(crate) const PORT_MIN: u16 = 1025;
pub(crate) const PORT_MAX: u16 = 65535;
const HOST_MAX_CHARS: usize = 253;
const LABEL_MAX_CHARS: usize = 63;

/// True for a host name of at most 253 characters (labels of 1 to 63 letters, digits and hyphens, none
/// starting or ending with a hyphen) or an IP literal written without brackets.
pub(crate) fn is_address(host: &str) -> bool {
    if host.parse::<IpAddr>().is_ok() {
        return true;
    }
    if host.is_empty() || host.chars().count() > HOST_MAX_CHARS {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.chars().count() <= LABEL_MAX_CHARS
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

/// Splits what the person typed into host and port: `host`, `host:port`, `[v6]`, `[v6]:port` or a bare
/// IPv6 literal. The port defaults to 8443. A host that is neither a name nor an IP literal →
/// `AddressInvalid`; a port outside 1025–65535 → `PortOutOfRange`.
pub(crate) fn split_address(raw: &str) -> Result<(String, u16), ConnectorError> {
    let raw = raw.trim();
    let (host, port): (&str, Option<&str>) = if let Some(rest) = raw.strip_prefix('[') {
        let (inner, tail) = rest.split_once(']').ok_or(ConnectorError::AddressInvalid)?;
        match tail {
            "" => (inner, None),
            _ => (
                inner,
                Some(
                    tail.strip_prefix(':')
                        .ok_or(ConnectorError::AddressInvalid)?,
                ),
            ),
        }
    } else if raw.parse::<IpAddr>().is_ok() {
        (raw, None)
    } else {
        match raw.rsplit_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (raw, None),
        }
    };
    if host.contains(':') && host.parse::<std::net::Ipv6Addr>().is_err() {
        return Err(ConnectorError::AddressInvalid);
    }
    if !is_address(host) {
        return Err(ConnectorError::AddressInvalid);
    }
    let port = match port {
        None => DEFAULT_PORT,
        Some(p) => check_port(
            p.parse::<u32>()
                .map_err(|_| ConnectorError::PortOutOfRange)?,
        )?,
    };
    Ok((host.to_owned(), port))
}

/// Field rule `port`: 1025–65535.
pub(crate) fn check_port(port: u32) -> Result<u16, ConnectorError> {
    match u16::try_from(port) {
        Ok(p) if (PORT_MIN..=PORT_MAX).contains(&p) => Ok(p),
        _ => Err(ConnectorError::PortOutOfRange),
    }
}

/// `host:port` for a URL; an IPv6 literal is written in brackets.
pub(crate) fn authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_every_form() {
        assert_eq!(
            split_address("10.20.4.17").unwrap(),
            ("10.20.4.17".into(), 8443)
        );
        assert_eq!(
            split_address("a.example.org:9000").unwrap(),
            ("a.example.org".into(), 9000)
        );
        assert_eq!(
            split_address("[fe80::1]:9000").unwrap(),
            ("fe80::1".into(), 9000)
        );
        assert_eq!(
            split_address("[fe80::1]").unwrap(),
            ("fe80::1".into(), 8443)
        );
        assert_eq!(split_address("fe80::1").unwrap(), ("fe80::1".into(), 8443));
    }

    #[test]
    fn refuses_bad_hosts_and_ports() {
        for bad in [
            "",
            "bad_host",
            "-a.b",
            "a..b",
            "[fe80::1",
            "[fe80::1]x",
            "a:b:c",
            "[bad_host]",
        ] {
            assert!(
                matches!(split_address(bad), Err(ConnectorError::AddressInvalid)),
                "{bad}"
            );
        }
        for bad in ["h:80", "h:70000", "h:abc", "h:", "h:1024"] {
            assert!(
                matches!(split_address(bad), Err(ConnectorError::PortOutOfRange)),
                "{bad}"
            );
        }
        let long = format!("{}.org", "a".repeat(254));
        assert!(!is_address(&long));
        assert!(!is_address(&"a".repeat(64)));
    }

    #[test]
    fn authority_brackets_v6() {
        assert_eq!(authority("fe80::1", 8443), "[fe80::1]:8443");
        assert_eq!(authority("h.example", 8443), "h.example:8443");
    }
}
