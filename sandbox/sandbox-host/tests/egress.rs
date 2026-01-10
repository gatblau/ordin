//! ADR-042: the broker's outbound clients refuse forbidden destinations at connect time —
//! literal IPs before any request, and DNS names through the policy resolver, which hands
//! the client only the addresses it checked. No network or container is needed: every
//! case is refused before a connection is attempted.

use std::time::Duration;

use sandboxd::downstream::{DownstreamClient, DownstreamError};
use sandboxd::mcp_upstream::{McpError, McpUpstreamClient};
use sandboxd::types::{AuthMode, CapabilityKind, ResolvedCapability};

fn rest(host: &str, allow_private_network: bool) -> ResolvedCapability {
    ResolvedCapability {
        id: "c".into(),
        provider: String::new(),
        audience: None,
        scopes: vec![],
        host: host.into(),
        path_allow: vec![],
        methods: vec!["GET".into()],
        require_step_up: false,
        auth_mode: AuthMode::Unauthenticated,
        allow_write: false,
        secret_ref: None,
        key_placement: None,
        kind: CapabilityKind::Rest,
        server_url: None,
        tool_allow: vec![],
        allow_private_network,
    }
}

fn mcp(server_url: &str, allow_private_network: bool) -> ResolvedCapability {
    ResolvedCapability {
        host: String::new(),
        methods: vec![],
        kind: CapabilityKind::Mcp,
        server_url: Some(server_url.into()),
        tool_allow: vec!["t".into()],
        ..rest("", allow_private_network)
    }
}

async fn rest_call(
    client: &DownstreamClient,
    cap: &ResolvedCapability,
) -> Result<(), DownstreamError> {
    client
        .do_call(cap, "GET", "/x", &vec![], b"", |_| {})
        .await
        .map(|_| ())
}

#[tokio::test]
async fn rest_refuses_metadata_loopback_and_unflagged_private_addresses() {
    let c = DownstreamClient::new(1024, Duration::from_secs(5), false).unwrap();
    for host in [
        "169.254.169.254",
        "127.0.0.1",
        "[::1]",
        "0.0.0.0",
        "10.1.2.3",
        "localhost",
    ] {
        assert_eq!(
            rest_call(&c, &rest(host, false)).await,
            Err(DownstreamError::EgressDenied),
            "{host}"
        );
    }
    // Forbidden ranges stay forbidden even for a capability that may reach private space.
    for host in ["169.254.169.254", "localhost", "[fe80::1]"] {
        assert_eq!(
            rest_call(&c, &rest(host, true)).await,
            Err(DownstreamError::EgressDenied),
            "{host}"
        );
    }
}

#[tokio::test]
async fn rest_private_address_is_dialled_only_with_the_flag() {
    let c = DownstreamClient::new(1024, Duration::from_millis(300), false).unwrap();
    // 10.255.255.1 is not routed here: with the flag the call is attempted (and fails as a
    // transport error or timeout), proving the address policy let it through.
    let r = rest_call(&c, &rest("10.255.255.1", true)).await;
    assert!(
        matches!(
            r,
            Err(DownstreamError::Unavailable) | Err(DownstreamError::Timeout)
        ),
        "{r:?}"
    );
}

#[tokio::test]
async fn rest_dev_loopback_exception_is_the_literal_only() {
    let c = DownstreamClient::new(1024, Duration::from_millis(300), true).unwrap();
    // 127.0.0.1 passes the address policy under ADR-032 (nothing listens on port 9, so the
    // call then fails as a transport error); `localhost` is still refused.
    let r = rest_call(&c, &rest("127.0.0.1:9", false)).await;
    assert!(r != Err(DownstreamError::EgressDenied), "{r:?}");
    assert_eq!(
        rest_call(&c, &rest("localhost:9", false)).await,
        Err(DownstreamError::EgressDenied)
    );
}

#[tokio::test]
async fn mcp_refuses_forbidden_and_unflagged_private_origins() {
    let c = McpUpstreamClient::new(1024, Duration::from_secs(5), false).unwrap();
    let args = serde_json::json!({});
    for url in [
        "https://169.254.169.254/mcp",
        "https://10.0.0.8/mcp",
        "https://localhost/mcp",
    ] {
        let r = c.call_tool(&mcp(url, false), "t", &args, |_| {}).await;
        assert!(matches!(r, Err(McpError::EgressDenied)), "{url}: {r:?}");
    }
}
