//! X1: one test per row of SharedTypes.md's *Error Table*, provoking the condition and asserting the
//! code and the retryability (no error of this crate is retryable).

use ordin_types::encoding::b64url_decode_exact;
use ordin_types::prelude::*;
use ordin_types::text::{
    check_abs_path, check_display_name, check_https_url, check_len, AreaCodes,
};
use ordin_types::time::parse_rfc3339_ms_utc;
use ordin_types::yaml::{read_restricted, YamlError, YamlFeature};
use ordin_types::{MemoryLinkId, MemoryThingId};

/// The code and the retry flag, through the `CodedError` contract a program reads.
fn assert_permanent(error: &dyn CodedError, code: &str) {
    assert_eq!(error.code(), code);
    assert!(!error.retryable(), "{code} is not retryable");
    assert_eq!(error.repair(), None);
}

const PLACE: AreaCodes = AreaCodes {
    name_empty: "PLACE_NAME_INVALID",
    name_too_long: "PLACE_NAME_INVALID",
    path_invalid: "PLACE_PATH_INVALID",
    url_invalid: "CONF_URL_INVALID",
};

// Row: identifier not 36 characters, uppercase, unparseable or not version 7.
#[test]
fn identifier_not_uuidv7() {
    for raw in [
        "0192a1c2-3344-7556-8677-8899aabbccd",  // 35 characters
        "0192A1C2-3344-7556-8677-8899AABBCCDD", // uppercase
        "0192a1c2-3344-7556-8677-8899aabbccdz", // unparseable
        "0192a1c2x3344-7556-8677-8899aabbccdd", // a hyphen missing
        "0192a1c2-3344-4556-8677-8899aabbccdd", // version 4
        "{0192a1c2-3344-7556-8677-8899aabbcc}", // braces, 36 characters
    ] {
        let refusal = RunId::parse(raw).unwrap_err();
        assert_eq!(refusal, TypesError::IdNotUuidV7, "{raw}");
        assert_permanent(&refusal, "ID_NOT_UUIDV7");
    }
    assert!(RunId::parse("0192a1c2-3344-7556-8677-8899aabbccdd").is_ok());
}

// Row: timestamp not `YYYY-MM-DDTHH:MM:SS.mmmZ`, or an impossible date.
#[test]
fn timestamp_not_rfc3339_ms_utc() {
    for raw in [
        "2026-01-10T09:41:07Z",
        "2026-01-10T09:41:07.31Z",
        "2026-01-10T09:41:07.312+00:00",
        "January 2026 09:41:07.312Z",
        "2026-13-19T09:41:07.312Z",
        "2026-02-30T09:41:07.312Z",
        "2026-01-10T09:41:07.312z",
        "２026-10-19T09:41:07.312Z",
    ] {
        let refusal = parse_rfc3339_ms_utc(raw).unwrap_err();
        assert_permanent(&refusal, "TS_NOT_RFC3339_MS_UTC");
    }
    assert!(parse_rfc3339_ms_utc("2026-01-10T09:41:07.312Z").is_ok());
}

// Row: `from_unix_millis` outside 1970-01-01..9999-12-31.
#[test]
fn from_unix_millis_out_of_range() {
    for ms in [-1, 253_402_300_800_000, i64::MAX, i64::MIN] {
        let refusal = Timestamp::from_unix_millis(ms).unwrap_err();
        assert_permanent(&refusal, "TS_NOT_RFC3339_MS_UTC");
    }
    assert_eq!(
        Timestamp::from_unix_millis(0).unwrap().to_wire(),
        "1970-01-01T00:00:00.000Z"
    );
    let last = Timestamp::from_unix_millis(253_402_300_799_999).unwrap();
    assert_eq!(last.to_wire(), "9999-12-31T23:59:59.999Z");
    assert_eq!(last.unix_millis(), 253_402_300_799_999);
}

// Row: hash not exactly 64 lowercase hex characters.
#[test]
fn hash_not_sha256_hex() {
    let good = "9f2c1e7a40b3d85c6e1f0a2b4c6d8e0f11223344556677889900aabbccddeeff";
    for raw in [
        &good[..63],
        &good.to_uppercase(),
        &format!("{good}0"),
        &good.replace('9', "g"),
    ] {
        let refusal = Sha256Hex::parse(raw).unwrap_err();
        assert_permanent(&refusal, "HASH_NOT_SHA256_HEX");
    }
    assert_eq!(Sha256Hex::parse(good).unwrap().0, good);
    assert_eq!(Sha256Hex::from_digest(&[0xab; 32]).0, "ab".repeat(32));
    assert!(serde_json::from_str::<Sha256Hex>("\"abc\"").is_err());
}

// Row: decimal string not matching the pattern.
#[test]
fn num_not_decimal() {
    for raw in [
        "1e5",
        "",
        "-",
        ".5",
        "5.",
        "+1",
        "1.12345678901",
        &"9".repeat(29),
        "1,5",
    ] {
        let refusal = DecimalString::parse(raw).unwrap_err();
        assert_permanent(&refusal, "NUM_NOT_DECIMAL");
    }
    for raw in ["-12.50", "0", &"9".repeat(28), "1.1234567890"] {
        assert!(DecimalString::parse(raw).is_ok(), "{raw}");
    }
}

// Row: `MemoryThingId` or `MemoryLinkId` without its prefix or with a malformed UUID (ADR-111).
#[test]
fn memory_ref_invalid() {
    let uuid = "0192a2d0-1c3e-7a10-8b2f-5d6e7f809a1b";
    for raw in [
        uuid.to_owned(),
        format!("relationship:{uuid}"),
        format!("entity:{}", uuid.to_uppercase()),
        format!("entity:{}", &uuid[..35]),
        format!("entity:{}", uuid.replace('-', "x")),
    ] {
        assert_permanent(
            &MemoryThingId::parse(&raw).unwrap_err(),
            "ID_MEMORY_REF_INVALID",
        );
    }
    assert_permanent(
        &MemoryLinkId::parse(&format!("entity:{uuid}")).unwrap_err(),
        "ID_MEMORY_REF_INVALID",
    );
    // Any UUID version is accepted (v5 here).
    let v5 = "5c1f0e2a-8b3d-5e4f-9a6b-7c8d9e0f1a2b";
    assert_eq!(
        MemoryLinkId::parse(&format!("relationship:{v5}"))
            .unwrap()
            .0,
        format!("relationship:{v5}")
    );
    let thing: MemoryThingId = serde_json::from_str(&format!("\"entity:{uuid}\"")).unwrap();
    assert_eq!(
        serde_json::to_string(&thing).unwrap(),
        format!("\"entity:{uuid}\"")
    );
    assert!(serde_json::from_str::<MemoryLinkId>(&format!("\"entity:{uuid}\"")).is_err());
}

// Row: text contains a control character other than `\n`, `\t`.
#[test]
fn text_control_char() {
    for raw in ["a\rb", "a\u{0}b", "a\u{7}b", "a\u{1b}b", "a\u{85}b"] {
        assert_permanent(
            &ordin_types::text::normalise_text(raw).unwrap_err(),
            "TEXT_CONTROL_CHAR",
        );
    }
    assert_eq!(
        ordin_types::text::normalise_text("a\n\tb").unwrap(),
        "a\n\tb"
    );
}

// Row: length, display-name, path, URL or base64 rule fails (the caller's code).
#[test]
fn caller_rule_fails() {
    let rule = |code| TypesError::Rule { code };
    assert_eq!(
        check_len("", 1, 120, "INPUT_NAME_INVALID").unwrap_err(),
        rule("INPUT_NAME_INVALID")
    );
    assert_eq!(
        check_len(&"x".repeat(121), 1, 120, "INPUT_NAME_INVALID").unwrap_err(),
        rule("INPUT_NAME_INVALID")
    );
    assert_permanent(
        &check_len("", 1, 2, "INPUT_NAME_INVALID").unwrap_err(),
        "INPUT_NAME_INVALID",
    );

    assert_eq!(
        check_display_name("   ", 120, PLACE).unwrap_err(),
        rule("PLACE_NAME_INVALID")
    );
    assert_eq!(
        check_display_name(&"é".repeat(121), 120, PLACE).unwrap_err(),
        rule("PLACE_NAME_INVALID")
    );
    assert_eq!(
        check_display_name("a\u{7}", 120, PLACE).unwrap_err(),
        TypesError::TextControlChar
    );
    assert_eq!(
        check_display_name("  Quarterly reports ", 120, PLACE).unwrap(),
        "Quarterly reports"
    );

    for raw in [
        "",
        "relative/path",
        "/with\0nul",
        &format!("/{}", "a".repeat(4096)),
    ] {
        assert_eq!(
            check_abs_path(raw, PLACE).unwrap_err(),
            rule("PLACE_PATH_INVALID"),
            "{raw:?}"
        );
    }
    assert!(check_abs_path(&format!("/{}", "a".repeat(4095)), PLACE).is_ok());

    for (raw, loopback) in [
        ("http://example.org/", false),
        ("http://example.org/", true),
        ("http://localhost:4318/", false),
        ("ftp://example.org/", false),
        ("https://user:pass@example.org/", false),
        ("https://user@example.org/", false),
        ("not a url", false),
        ("data:text/plain,hello", false),
    ] {
        assert_eq!(
            check_https_url(raw, loopback, PLACE).unwrap_err(),
            rule("CONF_URL_INVALID"),
            "{raw}"
        );
    }
    let long = format!("https://example.org/{}", "a".repeat(2048));
    assert_eq!(
        check_https_url(&long, false, PLACE).unwrap_err(),
        rule("CONF_URL_INVALID")
    );
    assert!(check_https_url("https://example.org/a", false, PLACE).is_ok());
    assert!(check_https_url("http://localhost:4318/", true, PLACE).is_ok());
    assert!(check_https_url("http://127.0.0.1:8080/", true, PLACE).is_ok());

    // base64url: padding, a character outside the alphabet, the wrong decoded length.
    for raw in ["AAAAAAAAAAAAAAAAAAAAAA==", "AAAAAAAAAAAAAAAAAAAA+A", "AAAA"] {
        assert_eq!(
            b64url_decode_exact(raw, 16, "CARD_REF_INVALID").unwrap_err(),
            rule("CARD_REF_INVALID")
        );
    }
    let card = ordin_types::encoding::b64url_encode(&[7u8; 16]);
    assert_eq!(card.len(), 22);
    assert_eq!(
        b64url_decode_exact(&card, 16, "CARD_REF_INVALID").unwrap(),
        vec![7u8; 16]
    );
}

// Row: cursor over 1,024 characters, bad base64url, or wrong key shape.
#[test]
fn bad_cursor() {
    let cursor = |raw: &str| serde_json::from_value::<Cursor>(serde_json::json!(raw)).unwrap();
    let too_long = "A".repeat(1025);
    let wrong_shape = Cursor::encode(&"text");
    for refused in [cursor(&too_long), cursor("not*base64"), wrong_shape] {
        let refusal = refused.decode::<(u64, String)>().unwrap_err();
        assert_eq!(refusal, TypesError::BadCursor);
        assert_permanent(&refusal, "SRV_BAD_CURSOR");
    }
    let key = (17u64, "b".to_owned());
    assert_eq!(Cursor::encode(&key).decode::<(u64, String)>().unwrap(), key);
}

// Row: YAML uses an anchor, an alias, a merge key or a tag outside the core schema.
#[test]
fn yaml_feature_refused() {
    for (text, feature) in [
        ("a: &x 1\n", YamlFeature::Anchor),
        // An alias always follows its anchor, so the anchor is the refusal reported first.
        ("a: &x [1]\nb: *x\n", YamlFeature::Anchor),
        (
            "base: {a: 1}\nother:\n  <<: {b: 2}\n",
            YamlFeature::MergeKey,
        ),
        ("a: !custom 1\n", YamlFeature::CustomTag),
        ("a: !!binary aGk=\n", YamlFeature::CustomTag),
    ] {
        let refusal = read_restricted::<serde_json::Value>(text, 1024).unwrap_err();
        match &refusal {
            YamlError::FeatureRefused {
                feature: found,
                line,
            } => {
                assert_eq!(*found, feature, "{text}");
                assert!(*line >= 1);
            }
            other => panic!("{text}: {other:?}"),
        }
        assert_permanent(&refusal, "YAML_FEATURE_REFUSED");
        assert!(!refusal.to_string().contains(text.trim()));
    }
}

// Row: YAML not well-formed, several documents, duplicate or non-scalar key, nesting over 64, a
// non-finite float, or not the shape of `T`.
#[test]
fn yaml_malformed() {
    let deep = format!("{}{}", "[".repeat(65), "]".repeat(65));
    for text in [
        "a: [1, 2\n",
        "a: 1\n---\nb: 2\n",
        "a: 1\na: 2\n",
        "? [a]\n: 1\n",
        deep.as_str(),
        "a: .inf\n",
        "a: -.inf\n",
        "a: .nan\n",
        "a: !!int x\n",
        "a: 99999999999999999999\n",
    ] {
        let refusal = read_restricted::<serde_json::Value>(text, 4096).unwrap_err();
        assert!(
            matches!(refusal, YamlError::Malformed { .. }),
            "{text}: {refusal:?}"
        );
        assert_permanent(&refusal, "YAML_MALFORMED");
    }
    // Not the shape of `T`.
    let refusal = read_restricted::<Vec<u32>>("a: 1\n", 1024).unwrap_err();
    assert_eq!(refusal, YamlError::Malformed { line: 0 });
    assert_eq!(refusal.to_string(), "the YAML is malformed at line 0");
}

// Row: YAML text longer than `max_bytes`.
#[test]
fn yaml_too_large() {
    let refusal = read_restricted::<serde_json::Value>("a: 1\n", 4).unwrap_err();
    assert_eq!(refusal, YamlError::TooLarge { max_bytes: 4 });
    assert_permanent(&refusal, "YAML_TOO_LARGE");
    assert!(read_restricted::<serde_json::Value>("a: 1\n", 5).is_ok());
}
