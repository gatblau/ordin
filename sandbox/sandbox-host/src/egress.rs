//! Egress address policy for the broker's outbound calls (ADR-042).
//!
//! The capability allowlist names hosts an administrator chose, but a name can resolve — or
//! be made to resolve — to an address that is never a legitimate API: loopback, link-local
//! (including cloud metadata at 169.254.169.254), unspecified, multicast, documentation or
//! reserved space. Those are always refused. Private space (RFC 1918, carrier-grade NAT,
//! IPv6 unique-local) is where internal company services live, so it is allowed only for a
//! capability whose admin-signed policy sets `allowPrivateNetwork`.
//!
//! The check is made where the connection is made: the HTTP client resolves names through
//! [`PolicyResolver`], which refuses a name if *any* of its addresses is disallowed and hands
//! the client only the addresses it checked. So a DNS answer cannot change between check and
//! connect. Literal IP hosts never reach a resolver and are checked by [`check_literal`].

use std::error::Error;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

/// Which address classes a call may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressPolicy {
    /// Public addresses only.
    PublicOnly,
    /// Public and private addresses (`allowPrivateNetwork`).
    AllowPrivate,
}

impl AddressPolicy {
    /// The policy for a capability's `allowPrivateNetwork` flag.
    pub fn for_capability(allow_private_network: bool) -> AddressPolicy {
        if allow_private_network {
            AddressPolicy::AllowPrivate
        } else {
            AddressPolicy::PublicOnly
        }
    }
}

/// How an address is classified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressClass {
    Public,
    Private,
    /// Never a legitimate API destination.
    Forbidden,
}

/// The refusal carried through the HTTP client's error chain, so callers can tell an egress
/// refusal from an ordinary connection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressDenied;

impl fmt::Display for EgressDenied {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("egress to this address is not allowed")
    }
}

impl Error for EgressDenied {}

/// Classify one address.
pub fn classify(ip: IpAddr) -> AddressClass {
    match ip {
        IpAddr::V4(v4) => classify_v4(v4),
        IpAddr::V6(v6) => classify_v6(v6),
    }
}

/// Whether `ip` may be reached under `policy`.
pub fn allowed(ip: IpAddr, policy: AddressPolicy) -> bool {
    match classify(ip) {
        AddressClass::Public => true,
        AddressClass::Private => policy == AddressPolicy::AllowPrivate,
        AddressClass::Forbidden => false,
    }
}

fn classify_v4(ip: Ipv4Addr) -> AddressClass {
    let [a, b, c, _] = ip.octets();
    let forbidden = a == 0 // "this network", including 0.0.0.0
        || a == 127 // loopback
        || (a == 169 && b == 254) // link-local, cloud metadata
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // TEST-NET-1
        || (a == 198 && b == 51 && c == 100) // TEST-NET-2
        || (a == 203 && b == 0 && c == 113) // TEST-NET-3
        || (a == 198 && (b == 18 || b == 19)) // benchmarking
        || a >= 224; // multicast, reserved, broadcast
    if forbidden {
        return AddressClass::Forbidden;
    }
    let private = a == 10
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 100 && (64..=127).contains(&b)); // carrier-grade NAT
    if private {
        AddressClass::Private
    } else {
        AddressClass::Public
    }
}

fn classify_v6(ip: Ipv6Addr) -> AddressClass {
    // An IPv4 address carried in IPv6 is judged as that IPv4 address.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return classify_v4(v4);
    }
    let s = ip.segments();
    // NAT64 well-known prefix 64:ff9b::/96 embeds an IPv4 address in the last 32 bits.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        let o = ip.octets();
        return classify_v4(Ipv4Addr::new(o[12], o[13], o[14], o[15]));
    }
    let forbidden = ip.is_unspecified()
        || ip.is_loopback()
        || s[0] & 0xffc0 == 0xfe80 // link-local
        || s[0] & 0xffc0 == 0xfec0 // site-local (deprecated)
        || s[0] & 0xff00 == 0xff00 // multicast
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || s[..6].iter().all(|x| *x == 0); // IPv4-compatible (deprecated) ::a.b.c.d
    if forbidden {
        return AddressClass::Forbidden;
    }
    if s[0] & 0xfe00 == 0xfc00 {
        // unique-local fc00::/7
        return AddressClass::Private;
    }
    AddressClass::Public
}

/// Check a URL host that is a literal IP address. A DNS name returns `Ok` here; the
/// resolver checks it at connect time. `allow_loopback_literal` is the ADR-032 development
/// exception: the literal `127.0.0.1`, and only that, stays reachable.
pub fn check_literal(
    host: &str,
    policy: AddressPolicy,
    allow_loopback_literal: bool,
) -> Result<(), EgressDenied> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(ip) = bare.parse::<IpAddr>() else {
        return Ok(());
    };
    if allow_loopback_literal && ip == IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Ok(());
    }
    if allowed(ip, policy) {
        Ok(())
    } else {
        Err(EgressDenied)
    }
}

/// A DNS resolver for the HTTP client that applies an [`AddressPolicy`]: it resolves with
/// the system resolver and refuses the name if any returned address is disallowed. The
/// client connects only to the addresses returned here.
pub struct PolicyResolver {
    policy: AddressPolicy,
}

impl PolicyResolver {
    pub fn new(policy: AddressPolicy) -> Arc<PolicyResolver> {
        Arc::new(PolicyResolver { policy })
    }
}

impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let policy = self.policy;
        let host = name.as_str().to_string();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addrs.is_empty() || addrs.iter().any(|a| !allowed(a.ip(), policy)) {
                return Err(Box::new(EgressDenied) as Box<dyn Error + Send + Sync>);
            }
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// True when `err`'s source chain contains an [`EgressDenied`].
pub fn is_egress_denied(err: &(dyn Error + 'static)) -> bool {
    let mut cur: Option<&(dyn Error + 'static)> = Some(err);
    while let Some(e) = cur {
        if e.downcast_ref::<EgressDenied>().is_some() {
            return true;
        }
        cur = e.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn forbidden_addresses_are_refused_even_with_private_allowed() {
        for s in [
            "127.0.0.1",
            "127.1.2.3",
            "0.0.0.0",
            "169.254.169.254",
            "224.0.0.1",
            "255.255.255.255",
            "192.0.2.10",
            "198.18.0.1",
            "::1",
            "::",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "64:ff9b::7f00:1",
        ] {
            assert_eq!(classify(ip(s)), AddressClass::Forbidden, "{s}");
            assert!(!allowed(ip(s), AddressPolicy::AllowPrivate), "{s}");
        }
    }

    #[test]
    fn private_addresses_need_the_opt_in() {
        for s in [
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.1.1",
            "100.64.0.1",
            "fd00::1",
            "::ffff:10.1.2.3",
        ] {
            assert_eq!(classify(ip(s)), AddressClass::Private, "{s}");
            assert!(!allowed(ip(s), AddressPolicy::PublicOnly), "{s}");
            assert!(allowed(ip(s), AddressPolicy::AllowPrivate), "{s}");
        }
    }

    #[test]
    fn public_addresses_are_allowed() {
        for s in ["1.1.1.1", "8.8.8.8", "172.32.0.1", "2606:4700:4700::1111"] {
            assert_eq!(classify(ip(s)), AddressClass::Public, "{s}");
            assert!(allowed(ip(s), AddressPolicy::PublicOnly), "{s}");
        }
    }

    #[test]
    fn literal_hosts_are_checked_and_names_are_deferred() {
        assert_eq!(
            check_literal("169.254.169.254", AddressPolicy::AllowPrivate, false),
            Err(EgressDenied)
        );
        assert_eq!(
            check_literal("[::1]", AddressPolicy::PublicOnly, false),
            Err(EgressDenied)
        );
        assert_eq!(
            check_literal("10.0.0.5", AddressPolicy::PublicOnly, false),
            Err(EgressDenied)
        );
        assert_eq!(
            check_literal("10.0.0.5", AddressPolicy::AllowPrivate, false),
            Ok(())
        );
        assert_eq!(
            check_literal("api.example.com", AddressPolicy::PublicOnly, false),
            Ok(())
        );
    }

    #[test]
    fn the_development_loopback_exception_is_the_literal_only() {
        assert_eq!(
            check_literal("127.0.0.1", AddressPolicy::PublicOnly, true),
            Ok(())
        );
        assert_eq!(
            check_literal("127.0.0.2", AddressPolicy::PublicOnly, true),
            Err(EgressDenied)
        );
        assert_eq!(
            check_literal("::1", AddressPolicy::PublicOnly, true),
            Err(EgressDenied)
        );
    }

    #[tokio::test]
    async fn the_resolver_refuses_a_name_that_resolves_to_loopback() {
        use reqwest::dns::Resolve;
        let r = PolicyResolver::new(AddressPolicy::AllowPrivate);
        let name: reqwest::dns::Name = "localhost".parse().unwrap();
        let err = match r.resolve(name).await {
            Ok(_) => panic!("localhost must be refused"),
            Err(e) => e,
        };
        assert!(is_egress_denied(err.as_ref()));
    }
}
