//! The one timestamp form of the workspace: RFC 3339, UTC, milliseconds, `Z` (SA-TIME-01).

use chrono::{DateTime, SecondsFormat, SubsecRound, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::TypesError;

/// RFC 3339, UTC, millisecond precision, `Z` suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub DateTime<Utc>);

/// `9999-12-31T23:59:59.999Z`, the last instant `from_unix_millis` accepts.
const MAX_UNIX_MILLIS: i64 = 253_402_300_799_999;

impl Timestamp {
    /// The current time, truncated to whole milliseconds (so a value survives a round trip unchanged).
    pub fn now() -> Self {
        Timestamp(Utc::now().trunc_subsecs(3))
    }

    /// Builds from milliseconds since the Unix epoch; refuses values outside 1970-01-01..9999-12-31.
    pub fn from_unix_millis(ms: i64) -> Result<Self, TypesError> {
        if !(0..=MAX_UNIX_MILLIS).contains(&ms) {
            return Err(TypesError::TimestampNotRfc3339MsUtc);
        }
        DateTime::<Utc>::from_timestamp_millis(ms)
            .map(Timestamp)
            .ok_or(TypesError::TimestampNotRfc3339MsUtc)
    }

    pub fn unix_millis(&self) -> i64 {
        self.0.timestamp_millis()
    }

    /// `YYYY-MM-DDTHH:MM:SS.mmmZ`.
    pub fn to_wire(&self) -> String {
        self.0.to_rfc3339_opts(SecondsFormat::Millis, true)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_wire())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = <std::borrow::Cow<'de, str>>::deserialize(d)?;
        parse_rfc3339_ms_utc(&raw)
            .map(Timestamp)
            .map_err(serde::de::Error::custom)
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_wire())
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Timestamp {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Timestamp".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "format": "date-time",
            "pattern": r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$"
        })
    }
}

/// The shape `YYYY-MM-DDTHH:MM:SS.mmmZ`, byte by byte: `d` a digit, any other byte literal.
const WIRE_SHAPE: &[u8; 24] = b"dddd-dd-ddTdd:dd:dd.dddZ";

/// Refuses anything but `YYYY-MM-DDTHH:MM:SS.mmmZ` (`TS_NOT_RFC3339_MS_UTC`).
pub fn parse_rfc3339_ms_utc(raw: &str) -> Result<DateTime<Utc>, TypesError> {
    // (a) the exact shape, checked without a regular-expression library.
    let bytes = raw.as_bytes();
    if bytes.len() != WIRE_SHAPE.len() {
        return Err(TypesError::TimestampNotRfc3339MsUtc);
    }
    let shaped = bytes.iter().zip(WIRE_SHAPE).all(|(b, s)| match s {
        b'd' => b.is_ascii_digit(),
        literal => b == literal,
    });
    if !shaped {
        return Err(TypesError::TimestampNotRfc3339MsUtc);
    }
    // (b) the calendar check (month 13, 30 February); (c) to UTC.
    DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| TypesError::TimestampNotRfc3339MsUtc)
}
