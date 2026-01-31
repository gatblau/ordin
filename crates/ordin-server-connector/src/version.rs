//! The `Ordin-Api-Version` check (ServerConnector.md Internal Logic 9).

use ordin_service_api::ServiceApiVersion;

/// The response header every Remote API answer carries.
pub(crate) const HEADER: &str = "Ordin-Api-Version";

/// Parses `<major>.<minor>`; anything else → `None`.
pub(crate) fn parse(raw: &str) -> Option<ServiceApiVersion> {
    let (major, minor) = raw.trim().split_once('.')?;
    if major.is_empty() || minor.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if !minor.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(ServiceApiVersion {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

/// Step 9: which side is older when `desktop_accepts` is false.
pub(crate) fn server_older(desktop: ServiceApiVersion, server: ServiceApiVersion) -> bool {
    server.major < desktop.major
        || (server.major == desktop.major && u32::from(server.minor) + 1 < u32::from(desktop.minor))
}

/// What the header of one response says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Absent or malformed → `BadAnswer`.
    Unreadable,
    /// The desktop can use this server.
    Usable(ServiceApiVersion),
    /// The desktop cannot; `server_older` names the side to update.
    Unusable {
        version: ServiceApiVersion,
        server_older: bool,
    },
}

pub(crate) fn judge(
    desktop: ServiceApiVersion,
    header: Option<&reqwest::header::HeaderValue>,
) -> Verdict {
    let Some(version) = header.and_then(|v| v.to_str().ok()).and_then(parse) else {
        return Verdict::Unreadable;
    };
    if ServiceApiVersion::desktop_accepts(desktop, version) {
        Verdict::Usable(version)
    } else {
        Verdict::Unusable {
            version,
            server_older: server_older(desktop, version),
        }
    }
}

/// `"<major>.<minor>"`, the form stored in `service_api_version`.
pub(crate) fn text(version: ServiceApiVersion) -> String {
    format!("{}.{}", version.major, version.minor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u16, minor: u16) -> ServiceApiVersion {
        ServiceApiVersion { major, minor }
    }

    #[test]
    fn parses_and_judges() {
        assert_eq!(parse("1.0"), Some(v(1, 0)));
        assert_eq!(parse(" 2.13 "), Some(v(2, 13)));
        for bad in [
            "", "1", "1.", ".1", "a.1", "1.b", "1.2.3", "-1.0", "99999.0",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
        assert!(server_older(v(1, 2), v(1, 0)));
        assert!(!server_older(v(1, 0), v(1, 1)));
        assert!(server_older(v(2, 0), v(1, 9)));
        assert!(!server_older(v(1, 2), v(2, 0)));
        let h = |s: &'static str| reqwest::header::HeaderValue::from_static(s);
        assert_eq!(judge(v(1, 0), None), Verdict::Unreadable);
        assert_eq!(judge(v(1, 0), Some(&h("x"))), Verdict::Unreadable);
        assert_eq!(judge(v(1, 0), Some(&h("1.0"))), Verdict::Usable(v(1, 0)));
        assert_eq!(
            judge(v(1, 2), Some(&h("1.0"))),
            Verdict::Unusable {
                version: v(1, 0),
                server_older: true
            }
        );
        assert_eq!(
            judge(v(1, 0), Some(&h("1.1"))),
            Verdict::Unusable {
                version: v(1, 1),
                server_older: false
            }
        );
        assert_eq!(text(v(1, 0)), "1.0");
    }
}
