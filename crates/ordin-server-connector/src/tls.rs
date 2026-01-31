//! The client of one server (ServerConnector.md Internal Logic 2): TLS 1.3 only, server verification by
//! the operating system's trust store or by a pinned leaf-certificate fingerprint, no redirects, one
//! connection pool per server.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::ResolvesClientCert;
use rustls::crypto::{CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::sign::CertifiedKey;
use rustls::{CertificateError, ClientConfig, DigitallySignedStruct, OtherError, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::error::ConnectorError;

/// What the last handshake's certificate check found.
const FLAG_CLEAR: u8 = 0;
const FLAG_UNTRUSTED: u8 = 1;
const FLAG_MISMATCH: u8 = 2;

/// One server's HTTP client with the record of its certificate check.
pub(crate) struct Bundle {
    pub(crate) client: reqwest::Client,
    flag: Arc<AtomicU8>,
}

impl Bundle {
    /// Forget the previous handshake's finding; called before each request.
    pub(crate) fn reset_flag(&self) {
        self.flag.store(FLAG_CLEAR, Ordering::SeqCst);
    }

    /// Maps a failed request to the code of its cause: a pinned fingerprint that did not match, a
    /// certificate the computer does not trust, or an unreachable server.
    pub(crate) fn classify(&self) -> ConnectorError {
        match self.flag.load(Ordering::SeqCst) {
            FLAG_MISMATCH => ConnectorError::FingerprintMismatch,
            FLAG_UNTRUSTED => ConnectorError::CertificateUntrusted,
            _ => ConnectorError::Unreachable,
        }
    }
}

#[derive(Debug)]
struct PinMismatch;

impl std::fmt::Display for PinMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the certificate is not the pinned one")
    }
}

impl std::error::Error for PinMismatch {}

/// Accepts the chain only when the leaf certificate's SHA-256 equals the pinned fingerprint; the
/// handshake signature is still checked with the provider's own algorithms.
#[derive(Debug)]
struct PinVerifier {
    fingerprint: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
    flag: Arc<AtomicU8>,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen: [u8; 32] = Sha256::digest(end_entity.as_ref()).into();
        if seen == self.fingerprint {
            Ok(ServerCertVerified::assertion())
        } else {
            self.flag.store(FLAG_MISMATCH, Ordering::SeqCst);
            Err(rustls::Error::InvalidCertificate(CertificateError::Other(
                OtherError(Arc::new(PinMismatch)),
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// Wraps the operating system's verifier and records a refused certificate.
#[derive(Debug)]
struct RecordingVerifier {
    inner: Arc<dyn ServerCertVerifier>,
    flag: Arc<AtomicU8>,
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let verdict = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        );
        if matches!(verdict, Err(rustls::Error::InvalidCertificate(_))) {
            self.flag.store(FLAG_UNTRUSTED, Ordering::SeqCst);
        }
        verdict
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// The client certificate the organisation profile names, as a rustls signing identity.
#[derive(Debug)]
struct FixedClientCert(Arc<CertifiedKey>);

impl ResolvesClientCert for FixedClientCert {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }

    fn has_certs(&self) -> bool {
        true
    }
}

/// What a client is built from.
pub(crate) struct ClientSpec {
    /// Lower-case hex SHA-256 of the pinned leaf certificate; `None` verifies with the system store.
    pub(crate) pin: Option<String>,
    pub(crate) client_cert: Option<Arc<CertifiedKey>>,
    pub(crate) connect_timeout: Duration,
    pub(crate) keep_alive: Duration,
    /// Test stand-ins on plain HTTP never read the environment's proxy.
    pub(crate) no_proxy: bool,
}

/// The 32 bytes of a 64-character hex fingerprint.
pub(crate) fn decode_fingerprint(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// Builds the client of one server (Internal Logic 2).
pub(crate) fn build(spec: ClientSpec) -> Result<Bundle, ConnectorError> {
    let provider: Arc<CryptoProvider> = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let flag = Arc::new(AtomicU8::new(FLAG_CLEAR));
    let verifier: Arc<dyn ServerCertVerifier> = match &spec.pin {
        Some(hex) => Arc::new(PinVerifier {
            fingerprint: decode_fingerprint(hex).ok_or(ConnectorError::Datastore)?,
            algorithms: provider.signature_verification_algorithms,
            flag: Arc::clone(&flag),
        }),
        None => {
            let platform = rustls_platform_verifier::Verifier::new(Arc::clone(&provider))
                .map_err(|_| ConnectorError::CertificateUntrusted)?;
            Arc::new(RecordingVerifier {
                inner: Arc::new(platform),
                flag: Arc::clone(&flag),
            })
        }
    };
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|_| ConnectorError::Unreachable)?
        .dangerous()
        .with_custom_certificate_verifier(verifier);
    let mut config = match spec.client_cert {
        Some(key) => builder.with_client_cert_resolver(Arc::new(FixedClientCert(key))),
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    let mut client = reqwest::Client::builder()
        .use_preconfigured_tls(config)
        .connect_timeout(spec.connect_timeout)
        .pool_idle_timeout(None)
        .http2_keep_alive_interval(spec.keep_alive)
        .redirect(reqwest::redirect::Policy::none());
    if spec.no_proxy {
        client = client.no_proxy();
    }
    let client = client.build().map_err(|_| ConnectorError::Unreachable)?;
    Ok(Bundle { client, flag })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_fingerprints() {
        let hex = "9f2c5b0e7a1d4c3b8e6f2a9d0c1b7e5f4a3d2c1b0e9f8a7d6c5b4a3f2e1d0c9b";
        let raw = decode_fingerprint(hex).unwrap();
        assert_eq!(raw[0], 0x9f);
        assert_eq!(raw[31], 0x9b);
        assert!(decode_fingerprint("abc").is_none());
        assert!(decode_fingerprint(&"zz".repeat(32)).is_none());
    }

    #[test]
    fn builds_with_and_without_a_pin() {
        let spec = |pin: Option<String>| ClientSpec {
            pin,
            client_cert: None,
            connect_timeout: Duration::from_secs(5),
            keep_alive: Duration::from_secs(20),
            no_proxy: true,
        };
        assert!(build(spec(None)).is_ok());
        assert!(build(spec(Some("00".repeat(32)))).is_ok());
        assert!(matches!(
            build(spec(Some("short".into()))),
            Err(ConnectorError::Datastore)
        ));
    }

    #[test]
    fn classify_follows_the_flag() {
        let bundle = build(ClientSpec {
            pin: None,
            client_cert: None,
            connect_timeout: Duration::from_secs(1),
            keep_alive: Duration::from_secs(20),
            no_proxy: true,
        })
        .unwrap();
        assert!(matches!(bundle.classify(), ConnectorError::Unreachable));
        bundle.flag.store(FLAG_UNTRUSTED, Ordering::SeqCst);
        assert!(matches!(
            bundle.classify(),
            ConnectorError::CertificateUntrusted
        ));
        bundle.flag.store(FLAG_MISMATCH, Ordering::SeqCst);
        assert!(matches!(
            bundle.classify(),
            ConnectorError::FingerprintMismatch
        ));
        bundle.reset_flag();
        assert!(matches!(bundle.classify(), ConnectorError::Unreachable));
    }
}
