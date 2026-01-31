//! A hand-written TLS 1.3 loopback stand-in for the pairing route: it presents one of two committed
//! leaf certificates (`tests/fixtures/leaf_a.der`, `leaf_b.der`, valid from 2026-01-10) and answers every
//! request with the canned JSON given, as a minimal HTTP/1.1 server.

use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub struct TlsStandIn {
    pub port: u16,
    pub task: tokio::task::JoinHandle<()>,
}

impl Drop for TlsStandIn {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap_or_else(|_| panic!("fixture {name} is missing"))
}

/// Lower-case hex SHA-256 of a committed leaf certificate.
pub fn fingerprint(which: &str) -> String {
    let der = fixture(&format!("leaf_{which}.der"));
    Sha256::digest(der)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl TlsStandIn {
    /// Serves `body` with `status` to every request, presenting the certificate `which` ("a" or "b").
    pub async fn start(which: &str, status: u16, body: String) -> TlsStandIn {
        let cert = CertificateDer::from(fixture(&format!("leaf_{which}.der")));
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture(&format!(
            "leaf_{which}.key.der"
        ))));
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .expect("versions")
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .expect("certificate");
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let task = tokio::spawn(async move {
            loop {
                let Ok((tcp, _)) = listener.accept().await else {
                    return;
                };
                let acceptor = acceptor.clone();
                let body = body.clone();
                tokio::spawn(async move {
                    let Ok(mut tls) = acceptor.accept(tcp).await else {
                        return;
                    };
                    let mut buf = vec![0u8; 8192];
                    let mut seen = Vec::new();
                    let (mut head_end, mut length) = (None, 0usize);
                    loop {
                        let Ok(n) = tls.read(&mut buf).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        seen.extend_from_slice(&buf[..n]);
                        if head_end.is_none() {
                            if let Some(i) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                                head_end = Some(i + 4);
                                let head = String::from_utf8_lossy(&seen[..i]).to_lowercase();
                                length = head
                                    .lines()
                                    .find_map(|l| l.strip_prefix("content-length:"))
                                    .and_then(|v| v.trim().parse().ok())
                                    .unwrap_or(0);
                            }
                        }
                        if let Some(h) = head_end {
                            if seen.len() >= h + length {
                                break;
                            }
                        }
                    }
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nOrdin-Api-Version: 1.0\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = tls.write_all(response.as_bytes()).await;
                    let _ = tls.shutdown().await;
                });
            }
        });
        TlsStandIn { port, task }
    }
}
