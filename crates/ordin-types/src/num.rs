//! The SHA-256 and decimal-string forms (2C shared rule table), each checked on the way in.

use serde::{Deserialize, Deserializer, Serialize};

use crate::TypesError;

/// SHA-256 as 64 lowercase hexadecimal characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct Sha256Hex(pub String);

/// A base-10 decimal number written as text (`-12.50`), so no binary float enters the model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct DecimalString(pub String);

/// The number of hex characters of a SHA-256 digest.
const SHA256_HEX_CHARS: usize = 64;
/// At most 28 digits before the point and 10 after it.
const MAX_INTEGER_DIGITS: usize = 28;
const MAX_FRACTION_DIGITS: usize = 10;

impl Sha256Hex {
    /// Exactly 64 characters of `[0-9a-f]`, else `HASH_NOT_SHA256_HEX`.
    pub fn parse(raw: &str) -> Result<Self, TypesError> {
        let lower_hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        if raw.len() == SHA256_HEX_CHARS && raw.bytes().all(lower_hex) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(TypesError::HashNotSha256Hex)
        }
    }

    /// Lowercase hex of a 32-byte digest computed by the caller.
    pub fn from_digest(digest: &[u8; 32]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(SHA256_HEX_CHARS);
        for byte in digest {
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(out)
    }
}

/// True when `digits` is `min..=max` ASCII digits.
fn is_digits(digits: &str, max: usize) -> bool {
    !digits.is_empty() && digits.len() <= max && digits.bytes().all(|b| b.is_ascii_digit())
}

impl DecimalString {
    /// Matches `^-?[0-9]{1,28}(\.[0-9]{1,10})?$`, else `NUM_NOT_DECIMAL`.
    pub fn parse(raw: &str) -> Result<Self, TypesError> {
        let unsigned = raw.strip_prefix('-').unwrap_or(raw);
        let valid = match unsigned.split_once('.') {
            Some((whole, fraction)) => {
                is_digits(whole, MAX_INTEGER_DIGITS) && is_digits(fraction, MAX_FRACTION_DIGITS)
            }
            None => is_digits(unsigned, MAX_INTEGER_DIGITS),
        };
        if valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(TypesError::NumNotDecimal)
        }
    }
}

impl<'de> Deserialize<'de> for Sha256Hex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for DecimalString {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}
