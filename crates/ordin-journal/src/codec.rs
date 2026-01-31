//! Column values to and from the 2C types. Every value is stored as text or an integer (LD-01: a
//! `Vec`, struct or data-carrying enum is one JSON column); a stored value that does not decode is a
//! datastore fault (`JRNL_DATASTORE_UNAVAILABLE`), never a panic.

use ordin_types::time::{parse_rfc3339_ms_utc, Timestamp};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::JournalError;

/// A stored timestamp (`YYYY-MM-DDTHH:MM:SS.mmmZ` on both engines).
pub(crate) fn ts(raw: &str) -> Result<Timestamp, JournalError> {
    parse_rfc3339_ms_utc(raw)
        .map(Timestamp)
        .map_err(|_| JournalError::Unavailable)
}

pub(crate) fn opt_ts(raw: Option<String>) -> Result<Option<Timestamp>, JournalError> {
    raw.as_deref().map(ts).transpose()
}

/// A stored identifier. Read without the version-7 check: the row was written by this crate, and
/// `data.run_id` of an event may be any UUID (Internal Logic 4.3).
pub(crate) fn uuid(raw: &str) -> Result<uuid::Uuid, JournalError> {
    uuid::Uuid::try_parse(raw).map_err(|_| JournalError::Unavailable)
}

pub(crate) fn opt_uuid(raw: Option<String>) -> Result<Option<uuid::Uuid>, JournalError> {
    raw.as_deref().map(uuid).transpose()
}

/// A JSON column.
pub(crate) fn from_json<T: DeserializeOwned>(raw: &str) -> Result<T, JournalError> {
    serde_json::from_str(raw).map_err(|_| JournalError::Unavailable)
}

/// The JSON text of a value to store.
pub(crate) fn to_json<T: Serialize>(value: &T) -> Result<String, JournalError> {
    serde_json::to_string(value).map_err(|_| JournalError::Unavailable)
}

/// A unit enum stored as its snake_case name (`requested`, `unknown_outcome`).
pub(crate) fn name_of<T: Serialize>(value: &T) -> Result<String, JournalError> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => Ok(name),
        _ => Err(JournalError::Unavailable),
    }
}

/// The inverse of [`name_of`].
pub(crate) fn from_name<T: DeserializeOwned>(raw: &str) -> Result<T, JournalError> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| JournalError::Unavailable)
}

/// A non-negative stored integer as `u32`.
pub(crate) fn u32_of(raw: i64) -> Result<u32, JournalError> {
    u32::try_from(raw).map_err(|_| JournalError::Unavailable)
}

/// A non-negative stored integer as `u64`.
pub(crate) fn u64_of(raw: i64) -> Result<u64, JournalError> {
    u64::try_from(raw).map_err(|_| JournalError::Unavailable)
}

/// A count or sequence number to bind.
pub(crate) fn i64_of(value: u64) -> Result<i64, JournalError> {
    i64::try_from(value).map_err(|_| JournalError::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordin_types::domain::runs::ExternalActionState;

    #[test]
    fn stored_values_round_trip_and_bad_ones_are_datastore_faults() {
        let state = name_of(&ExternalActionState::UnknownOutcome).unwrap();
        assert_eq!(state, "unknown_outcome");
        assert_eq!(
            from_name::<ExternalActionState>(&state).unwrap(),
            ExternalActionState::UnknownOutcome
        );
        assert!(matches!(
            from_name::<ExternalActionState>("lost"),
            Err(JournalError::Unavailable)
        ));
        assert!(matches!(ts("yesterday"), Err(JournalError::Unavailable)));
        assert!(matches!(uuid("x"), Err(JournalError::Unavailable)));
        assert!(matches!(
            from_json::<Vec<u64>>("{"),
            Err(JournalError::Unavailable)
        ));
        assert!(matches!(name_of(&1), Err(JournalError::Unavailable)));
        assert!(matches!(u32_of(-1), Err(JournalError::Unavailable)));
        assert!(matches!(u64_of(-1), Err(JournalError::Unavailable)));
        assert!(matches!(i64_of(u64::MAX), Err(JournalError::Unavailable)));
        assert_eq!(opt_ts(None).unwrap(), None);
        assert_eq!(opt_uuid(None).unwrap(), None);
    }
}
