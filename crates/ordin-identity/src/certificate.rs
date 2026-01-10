//! The person's client certificate (SA-CLOSE-02; Internal Logic H.5, H.12): PEM read with `rustls`'
//! PEM support, the key loaded with the process's default `rustls` crypto provider, and the leaf's
//! subject and `notAfter` read from its DER.
//!
//! No X.509 library is named in HLD 05 and SA-CLOSE-02 allows none beyond `rustls`, which does not
//! expose the subject or the validity period. The two fields are therefore read by the small DER walk
//! below, which reads only `tbsCertificate.validity.notAfter` and `tbsCertificate.subject`.

use std::sync::Arc;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use ordin_types::time::Timestamp;
use rustls::crypto::CryptoProvider;
use rustls::pki_types::pem::PemObject as _;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::sign::CertifiedKey;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize as _;

use crate::error::{code, fail, CredError};

/// The most certificates an imported chain may hold (H.12).
const MAX_CHAIN: usize = 10;
/// The longest subject kept in the view.
const MAX_SUBJECT_CHARS: usize = 254;

/// The JSON kept in the secret store (H.5).
#[derive(Serialize, Deserialize)]
pub(crate) struct StoredCertificate {
    pub format: u32,
    pub certificate_pem: String,
    pub key_pem: String,
}

impl Drop for StoredCertificate {
    fn drop(&mut self) {
        self.key_pem.zeroize();
    }
}

/// What H.12 learns from a valid import.
pub(crate) struct Parsed {
    pub key: Arc<CertifiedKey>,
    pub subject: String,
    pub not_after: Timestamp,
}

fn provider() -> Arc<CryptoProvider> {
    CryptoProvider::get_default()
        .cloned()
        .unwrap_or_else(|| Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
}

/// Parses and checks a chain and its key. `invalid` is the code for an unreadable, missing,
/// unloadable or mismatched pair: `CRED_CERTIFICATE_INVALID` on import, `CRED_SECRET_STORE_CORRUPT`
/// when reading back a stored value.
pub(crate) fn parse(
    certificate_pem: &[u8],
    key_pem: &[u8],
    invalid: &'static str,
) -> Result<Parsed, CredError> {
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(certificate_pem)
        .collect::<Result<_, _>>()
        .or_else(|_| fail(invalid))?;
    if chain.is_empty() || chain.len() > MAX_CHAIN {
        return fail(invalid);
    }
    let key = PrivateKeyDer::from_pem_slice(key_pem).or_else(|_| fail(invalid))?;
    let (subject, not_after) =
        leaf_fields(chain[0].as_ref()).ok_or_else(|| CredError::of(invalid))?;
    let certified = CertifiedKey::from_der(chain, key, &provider()).or_else(|_| fail(invalid))?;
    Ok(Parsed {
        key: Arc::new(certified),
        subject,
        not_after,
    })
}

/// H.12: a leaf whose `notAfter` is past is `CRED_CERTIFICATE_EXPIRED`.
pub(crate) fn check_not_expired(parsed: &Parsed) -> Result<(), CredError> {
    if parsed.not_after < Timestamp::now() {
        return fail(code::CERTIFICATE_EXPIRED);
    }
    Ok(())
}

// ---- A minimal DER reader: tag, length, contents.

struct Tlv<'a> {
    tag: u8,
    contents: &'a [u8],
}

/// Reads one element; returns it and the rest.
fn read_tlv(input: &[u8]) -> Option<(Tlv<'_>, &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let count = usize::from(first & 0x7f);
        if count == 0 || count > 4 || rest.len() < count {
            return None;
        }
        let len = rest[..count]
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
        (len, &rest[count..])
    };
    if rest.len() < len {
        return None;
    }
    Some((
        Tlv {
            tag,
            contents: &rest[..len],
        },
        &rest[len..],
    ))
}

/// The elements of a constructed value, in order.
fn children(contents: &[u8]) -> Option<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    let mut rest = contents;
    while !rest.is_empty() {
        let (tlv, next) = read_tlv(rest)?;
        out.push(tlv);
        rest = next;
    }
    Some(out)
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const OID: u8 = 0x06;
const UTC_TIME: u8 = 0x17;
const GENERALIZED_TIME: u8 = 0x18;
const VERSION_TAG: u8 = 0xa0;

/// `(subject, notAfter)` of a DER certificate.
fn leaf_fields(der: &[u8]) -> Option<(String, Timestamp)> {
    let (certificate, _) = read_tlv(der)?;
    if certificate.tag != SEQUENCE {
        return None;
    }
    let parts = children(certificate.contents)?;
    let tbs = parts.first().filter(|t| t.tag == SEQUENCE)?;
    let fields = children(tbs.contents)?;
    // version [0] is optional; then serialNumber, signature, issuer, validity, subject.
    let skip = usize::from(fields.first()?.tag == VERSION_TAG);
    let validity = fields.get(skip + 3).filter(|t| t.tag == SEQUENCE)?;
    let subject = fields.get(skip + 4).filter(|t| t.tag == SEQUENCE)?;
    let times = children(validity.contents)?;
    let not_after = read_time(times.get(1)?)?;
    Some((subject_text(subject.contents)?, not_after))
}

fn digits(bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(bytes).ok()?;
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `UTCTime` `YYMMDDHHMMSSZ` (years 50–99 are 19YY) or `GeneralizedTime` `YYYYMMDDHHMMSSZ`.
fn read_time(tlv: &Tlv<'_>) -> Option<Timestamp> {
    let c = tlv.contents;
    let (year, rest) = match tlv.tag {
        UTC_TIME if c.len() == 13 => {
            let yy = digits(&c[..2])?;
            (if yy >= 50 { 1900 + yy } else { 2000 + yy }, &c[2..])
        }
        GENERALIZED_TIME if c.len() == 15 => (digits(&c[..4])?, &c[4..]),
        _ => return None,
    };
    if rest.last() != Some(&b'Z') {
        return None;
    }
    let date = NaiveDate::from_ymd_opt(
        i32::try_from(year).ok()?,
        digits(&rest[0..2])?,
        digits(&rest[2..4])?,
    )?;
    let time = NaiveTime::from_hms_opt(
        digits(&rest[4..6])?,
        digits(&rest[6..8])?,
        digits(&rest[8..10])?,
    )?;
    let at = NaiveDateTime::new(date, time).and_utc();
    Timestamp::from_unix_millis(at.timestamp_millis()).ok()
}

/// The common name, else the whole subject in RFC 4514 form, cut to 254 characters.
fn subject_text(name: &[u8]) -> Option<String> {
    let mut attributes: Vec<(String, String)> = Vec::new();
    for rdn in children(name)? {
        if rdn.tag != SET {
            return None;
        }
        for pair in children(rdn.contents)? {
            let items = children(pair.contents)?;
            let (oid, value) = (items.first()?, items.get(1)?);
            if oid.tag != OID {
                return None;
            }
            let value = String::from_utf8_lossy(value.contents).into_owned();
            attributes.push((oid_name(oid.contents), value));
        }
    }
    let text = match attributes.iter().find(|(name, _)| name == "CN") {
        Some((_, cn)) => cn.clone(),
        // RFC 4514 writes the last RDN first.
        None => attributes
            .iter()
            .rev()
            .map(|(name, value)| format!("{name}={}", escape_rfc4514(value)))
            .collect::<Vec<_>>()
            .join(","),
    };
    Some(text.chars().take(MAX_SUBJECT_CHARS).collect())
}

/// RFC 4514 short names for the attributes it lists; any other as its dotted OID.
fn oid_name(oid: &[u8]) -> String {
    match oid {
        [0x55, 0x04, 0x03] => "CN".to_owned(),
        [0x55, 0x04, 0x06] => "C".to_owned(),
        [0x55, 0x04, 0x07] => "L".to_owned(),
        [0x55, 0x04, 0x08] => "ST".to_owned(),
        [0x55, 0x04, 0x09] => "STREET".to_owned(),
        [0x55, 0x04, 0x0a] => "O".to_owned(),
        [0x55, 0x04, 0x0b] => "OU".to_owned(),
        [0x09, 0x92, 0x26, 0x89, 0x93, 0xf2, 0x2c, 0x64, 0x01, 0x19] => "DC".to_owned(),
        [0x09, 0x92, 0x26, 0x89, 0x93, 0xf2, 0x2c, 0x64, 0x01, 0x01] => "UID".to_owned(),
        other => dotted(other),
    }
}

fn dotted(oid: &[u8]) -> String {
    let mut parts: Vec<u64> = Vec::new();
    let mut value: u64 = 0;
    for (i, b) in oid.iter().enumerate() {
        value = (value << 7) | u64::from(b & 0x7f);
        if b & 0x80 == 0 {
            if parts.is_empty() && i < oid.len() {
                let first = (value / 40).min(2);
                parts.push(first);
                parts.push(value - first * 40);
            } else {
                parts.push(value);
            }
            value = 0;
        }
    }
    parts
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn escape_rfc4514(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let last = value.chars().count().saturating_sub(1);
    for (i, ch) in value.chars().enumerate() {
        let special = matches!(ch, ',' | '+' | '"' | '\\' | '<' | '>' | ';')
            || (i == 0 && (ch == '#' || ch == ' '))
            || (i == last && ch == ' ');
        if special {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_oid_reads_the_first_two_arcs_from_one_byte() {
        assert_eq!(
            dotted(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d]),
            "1.2.840.113549"
        );
    }

    #[test]
    fn rfc4514_escapes_the_special_characters() {
        assert_eq!(escape_rfc4514("#a,b "), "\\#a\\,b\\ ");
    }

    #[test]
    fn a_subject_without_a_common_name_is_written_in_rfc4514_form() {
        // SEQUENCE { SET { SEQUENCE { OID 2.5.4.10, UTF8String "ACME" } }, SET { SEQUENCE { OID 2.5.4.6, PrintableString "GB" } } }
        let name = [
            0x31, 0x0d, 0x30, 0x0b, 0x06, 0x03, 0x55, 0x04, 0x0a, 0x0c, 0x04, b'A', b'C', b'M',
            b'E', 0x31, 0x0b, 0x30, 0x09, 0x06, 0x03, 0x55, 0x04, 0x06, 0x13, 0x02, b'G', b'B',
        ];
        assert_eq!(subject_text(&name).as_deref(), Some("C=GB,O=ACME"));
    }

    #[test]
    fn utc_time_before_2050_is_this_century() {
        let tlv = Tlv {
            tag: UTC_TIME,
            contents: b"270630000000Z",
        };
        assert_eq!(
            read_time(&tlv).map(|t| t.to_wire()).as_deref(),
            Some("2027-06-30T00:00:00.000Z")
        );
        let tlv = Tlv {
            tag: GENERALIZED_TIME,
            contents: b"20990630000000Z",
        };
        assert_eq!(
            read_time(&tlv).map(|t| t.to_wire()).as_deref(),
            Some("2099-06-30T00:00:00.000Z")
        );
    }
}
