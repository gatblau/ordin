//! `parse_address` (RulesEngine.md *Internal Logic* I; wrightrun section E in Rust).

use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;

use regex::Regex;

use crate::error::RulesError;
use crate::types::Address;

const MAX_ADDRESS_CHARS: usize = 4_000;
const MAX_HOST_CHARS: usize = 253;
const MAX_LABEL_CHARS: usize = 63;

fn shape() -> &'static Regex {
    static SHAPE: OnceLock<Regex> = OnceLock::new();
    SHAPE.get_or_init(|| {
        // The pattern is a constant; it compiles.
        Regex::new(r"^([A-Za-z][A-Za-z0-9+.-]*)://([^/?#]*)([^?#]*)(?:\?([^#]*))?(?:#.*)?$")
            .unwrap_or_else(|_| unreachable!("constant pattern"))
    })
}

fn refused(reason: &'static str) -> RulesError {
    RulesError::NotAnAddress(reason)
}

/// Parses one address; each refusal names a reason that never repeats the text.
pub(crate) fn parse(text: &str) -> Result<Address, RulesError> {
    if text.is_empty() {
        return Err(refused("the address is empty"));
    }
    if text.chars().count() > MAX_ADDRESS_CHARS {
        return Err(refused("the address is too long"));
    }
    if text
        .chars()
        .any(|c| c == ' ' || c == '\\' || c.is_control() || c == '\u{7f}')
    {
        return Err(refused(
            "the address holds a space, a backslash or a control character",
        ));
    }
    let captures = shape()
        .captures(text)
        .ok_or_else(|| refused("the address has no scheme and host"))?;
    let scheme = captures
        .get(1)
        .map_or("", |m| m.as_str())
        .to_ascii_lowercase();
    let authority = captures.get(2).map_or("", |m| m.as_str());
    let raw_path = captures.get(3).map_or("", |m| m.as_str());
    let query = captures.get(4).map(|m| m.as_str().to_owned());

    if authority.contains('@') {
        return Err(refused("the address carries user information"));
    }
    if authority.contains('%') {
        return Err(refused("the address holds a percent sign in its host"));
    }

    let (host_text, port_text) = split_authority(authority)?;
    let port = match port_text {
        Some(p) => Some(parse_port(p)?),
        None => default_port(&scheme),
    };
    let (host, host_is_ip) = parse_host(host_text)?;
    let host_is_single_label = !host_is_ip && !host.contains('.');
    let path = normalise_path(raw_path)?;
    Ok(Address {
        scheme,
        host,
        port,
        path,
        query,
        host_is_ip,
        host_is_single_label,
    })
}

/// (host, port text). A bracketed IPv6 host keeps its brackets for `parse_host`.
fn split_authority(authority: &str) -> Result<(&str, Option<&str>), RulesError> {
    if authority.is_empty() {
        return Err(refused("the address has no host"));
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let close = rest
            .find(']')
            .ok_or_else(|| refused("the host is not valid"))?;
        let host = &authority[..close + 2];
        let after = &rest[close + 1..];
        return match after {
            "" => Ok((host, None)),
            _ => match after.strip_prefix(':') {
                Some(port) => Ok((host, Some(port))),
                None => Err(refused("the host is not valid")),
            },
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((host, Some(port))),
        None => Ok((authority, None)),
    }
}

fn parse_port(text: &str) -> Result<u16, RulesError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(refused("the port is not valid"));
    }
    match text.parse::<u32>() {
        Ok(port @ 1..=65_535) => u16::try_from(port).map_err(|_| refused("the port is not valid")),
        _ => Err(refused("the port is not valid")),
    }
}

fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" | "ws" => Some(80),
        "https" | "wss" => Some(443),
        _ => None,
    }
}

/// The host lower-cased, one trailing full stop removed, IDNA-encoded; and whether it is an IP.
fn parse_host(text: &str) -> Result<(String, bool), RulesError> {
    if let Some(inner) = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')) {
        let ip: Ipv6Addr = inner
            .parse()
            .map_err(|_| refused("the host is not valid"))?;
        return Ok((ip.to_string(), true));
    }
    let mut host = text.to_lowercase();
    if host.ends_with('.') {
        host.pop();
    }
    if host.is_empty() {
        return Err(refused("the address has no host"));
    }
    if !host.is_ascii() {
        host = match url::Host::parse(&host) {
            Ok(url::Host::Domain(domain)) => domain,
            _ => return Err(refused("the host is not valid")),
        };
    }
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Ok((ip.to_string(), true));
    }
    if host.chars().count() > MAX_HOST_CHARS {
        return Err(refused("the host is too long"));
    }
    let labels: Vec<&str> = host.split('.').collect();
    for label in &labels {
        let fits = (1..=MAX_LABEL_CHARS).contains(&label.len())
            && label
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !fits {
            return Err(refused("the host is not valid"));
        }
    }
    let last = labels.last().copied().unwrap_or_default();
    if last.bytes().all(|b| b.is_ascii_digit()) || last.starts_with("0x") {
        return Err(refused(
            "the host looks like a number that is not an address",
        ));
    }
    Ok((host, false))
}

/// Percent-decoded once as UTF-8, dot segments removed (RFC 3986 §5.2.4); empty path is `/`.
fn normalise_path(raw: &str) -> Result<String, RulesError> {
    let decoded = percent_decode(raw)?;
    if decoded.chars().any(|c| c.is_control() || c == '\\') {
        return Err(refused("the path is not valid"));
    }
    let resolved = remove_dot_segments(&decoded);
    Ok(if resolved.is_empty() {
        "/".to_owned()
    } else {
        resolved
    })
}

fn percent_decode(raw: &str) -> Result<String, RulesError> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| refused("the path is not valid"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| refused("the path is not valid"))
}

/// RFC 3986 §5.2.4.
fn remove_dot_segments(path: &str) -> String {
    let mut input = path.to_owned();
    let mut output = String::new();
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest.to_owned();
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest.to_owned();
        } else if input.starts_with("/./") {
            input = input[2..].to_owned();
        } else if input == "/." {
            input = "/".to_owned();
        } else if input.starts_with("/../") || input == "/.." {
            input = if input == "/.." {
                "/".to_owned()
            } else {
                input[3..].to_owned()
            };
            match output.rfind('/') {
                Some(at) => output.truncate(at),
                None => output.clear(),
            }
        } else if input == "." || input == ".." {
            input.clear();
        } else {
            let start = usize::from(input.starts_with('/'));
            let end = input[start..]
                .find('/')
                .map_or(input.len(), |at| at + start);
            output.push_str(&input[..end]);
            input = input[end..].to_owned();
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_segments_follow_rfc_3986() {
        assert_eq!(remove_dot_segments("/a/b/c/./../../g"), "/a/g");
        assert_eq!(remove_dot_segments("mid/content=5/../6"), "mid/6");
        assert_eq!(remove_dot_segments("/.."), "/");
    }

    #[test]
    fn refusals_name_a_reason_only() {
        let cases = [
            "",
            "no-scheme",
            "https://user@example.org/",
            "https://exa%6dple.org/",
            "https://example.org:0/",
            "https://example.org:70000/",
            "https://example.org:x/",
            "https://[::1/",
            "https://[zz]/",
            "https://[::1]x/",
            "https://exa_mple.org/",
            "https://example.123/",
            "https://example.0x1f/",
            "https://a b/",
            "https:///path",
            "https://example.org/%zz",
            "https://example.org/%0a",
            "https://example.org/%ff",
        ];
        for case in cases {
            let error = parse(case).expect_err(case);
            assert_eq!(error.code(), "RULE_NOT_AN_ADDRESS");
            if !case.is_empty() {
                assert!(!error.to_string().contains(case), "{case}");
            }
        }
        let long = format!("https://example.org/{}", "a".repeat(4_000));
        assert!(parse(&long).is_err());
        let long_host = format!("https://{}.org/", vec!["a".repeat(60); 5].join("."));
        assert!(parse(&long_host).is_err());
    }

    #[test]
    fn hosts_ports_and_flags() {
        let a = parse("http://[::1]:8080/x").unwrap();
        assert!(a.host_is_ip);
        assert_eq!(a.port, Some(8080));
        let b = parse("wss://intranet").unwrap();
        assert!(b.host_is_single_label);
        assert_eq!(b.port, Some(443));
        assert_eq!(b.path, "/");
        let c = parse("ftp://10.0.0.1./a#frag").unwrap();
        assert!(c.host_is_ip);
        assert_eq!(c.port, None);
        let d = parse("https://bücher.example/").unwrap();
        assert_eq!(d.host, "xn--bcher-kva.example");
        let e = parse("https://Example.ORG./a?b=c").unwrap();
        assert_eq!(e.host, "example.org");
        assert_eq!(e.query.as_deref(), Some("b=c"));
    }
}
