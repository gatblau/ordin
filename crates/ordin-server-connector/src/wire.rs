//! Reading the Remote API's `{"kind":"ok"|"error", …}` bodies (RemoteApi.md *Routes*).

use ordin_service_api::ServiceError;
use serde_json::Value;

/// What a body says, without the parts the connector does not need.
#[derive(Debug)]
pub(crate) enum Shape {
    Ok(Value),
    Error(ServiceError),
}

/// `None` when the bytes are not JSON, have no `kind`, or `kind` does not match its payload.
pub(crate) fn read_reply(bytes: &[u8]) -> Option<Shape> {
    let mut value: Value = serde_json::from_slice(bytes).ok()?;
    let object = value.as_object_mut()?;
    match object.get("kind")?.as_str()? {
        "ok" => object.remove("result").map(Shape::Ok),
        "error" => {
            let error = object.remove("error")?;
            serde_json::from_value(error).ok().map(Shape::Error)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_kinds_and_refuses_the_rest() {
        assert!(matches!(
            read_reply(br#"{"kind":"ok","result":{}}"#),
            Some(Shape::Ok(_))
        ));
        match read_reply(br#"{"kind":"error","error":{"code":"X","message":"m"}}"#) {
            Some(Shape::Error(e)) => assert_eq!(e.code, "X"),
            other => panic!("{other:?}"),
        }
        for bad in [
            &b"not json"[..],
            br#"[]"#,
            br#"{"kind":"ok"}"#,
            br#"{"kind":"error"}"#,
            br#"{"kind":"error","error":{}}"#,
            br#"{"kind":"other"}"#,
            br#"{"kind":1}"#,
            br#"{}"#,
        ] {
            assert!(
                read_reply(bad).is_none(),
                "{}",
                String::from_utf8_lossy(bad)
            );
        }
    }
}
