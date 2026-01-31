//! The YAML value of the ordin format and its conversion from the restricted reader's node tree
//! (AgentModel.md *Internal Logic* 3.4–3.5).

use std::collections::BTreeMap;

use ordin_types::yaml::{read_restricted_node, YamlError, YamlNode};

/// The size limit of every file this crate parses: 1 MiB.
pub(crate) const MAX_FILE_BYTES: usize = 1_048_576;

/// A YAML value as the ordin format holds it. Non-integer numbers keep their source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YamlValue {
    Null,
    Bool(bool),
    Int(i64),
    Decimal(String),
    Str(String),
    Seq(Vec<YamlValue>),
    Map(BTreeMap<String, YamlValue>),
}

impl YamlValue {
    pub(crate) fn as_str(&self) -> Option<&str> {
        match self {
            YamlValue::Str(text) => Some(text),
            _ => None,
        }
    }

    pub(crate) fn as_map(&self) -> Option<&BTreeMap<String, YamlValue>> {
        match self {
            YamlValue::Map(map) => Some(map),
            _ => None,
        }
    }

    pub(crate) fn as_seq(&self) -> Option<&[YamlValue]> {
        match self {
            YamlValue::Seq(items) => Some(items),
            _ => None,
        }
    }
}

/// `^[-+]?[0-9]+$`.
pub(crate) fn is_int_text(text: &str) -> bool {
    let digits = text.strip_prefix(['-', '+']).unwrap_or(text);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// `^[-+]?([0-9]+\.[0-9]*|\.[0-9]+)([eE][-+]?[0-9]+)?$`.
pub(crate) fn is_decimal_text(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(at) => (&unsigned[..at], Some(&unsigned[at + 1..])),
        None => (unsigned, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        // `[0-9]+\.[0-9]*` needs a whole part, `\.[0-9]+` needs a fraction: `.` alone is neither.
        Some((whole, fraction)) => {
            digits(whole) && digits(fraction) && (!whole.is_empty() || !fraction.is_empty())
        }
        None => false,
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

/// Converts one plain (unquoted) scalar by step 3.5's rules.
pub(crate) fn plain_scalar(text: &str) -> YamlValue {
    match text {
        "null" | "~" | "" => return YamlValue::Null,
        "true" => return YamlValue::Bool(true),
        "false" => return YamlValue::Bool(false),
        _ => {}
    }
    if is_int_text(text) {
        if let Ok(n) = text.parse::<i64>() {
            return YamlValue::Int(n);
        }
    }
    if is_decimal_text(text) {
        return YamlValue::Decimal(text.to_owned());
    }
    YamlValue::Str(text.to_owned())
}

/// A key seen twice in one mapping. The restricted reader refuses duplicates itself; this is the
/// second guard of step 3.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DuplicateKey;

/// Step 3.5: the node tree as `YamlValue`.
pub(crate) fn from_node(node: YamlNode) -> Result<YamlValue, DuplicateKey> {
    Ok(match node {
        YamlNode::Null => YamlValue::Null,
        YamlNode::Scalar { text, quoted: true } => YamlValue::Str(text),
        YamlNode::Scalar {
            text,
            quoted: false,
        } => plain_scalar(&text),
        YamlNode::Seq(items) => {
            YamlValue::Seq(items.into_iter().map(from_node).collect::<Result<_, _>>()?)
        }
        YamlNode::Map(entries) => {
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                if map.insert(key, from_node(value)?).is_some() {
                    return Err(DuplicateKey);
                }
            }
            YamlValue::Map(map)
        }
    })
}

/// Why one document could not be read as a `YamlValue`, before the caller maps it to its own code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadFailure {
    NotUtf8,
    Yaml(YamlError),
    DuplicateKey,
}

/// Steps 3.4–3.5 over the whole text of a document.
pub(crate) fn read_value(bytes: &[u8]) -> Result<YamlValue, ReadFailure> {
    let text = std::str::from_utf8(bytes).map_err(|_| ReadFailure::NotUtf8)?;
    let node = read_restricted_node(text, MAX_FILE_BYTES).map_err(ReadFailure::Yaml)?;
    from_node(node).map_err(|_| ReadFailure::DuplicateKey)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_conversion() {
        assert_eq!(plain_scalar("~"), YamlValue::Null);
        assert_eq!(plain_scalar("true"), YamlValue::Bool(true));
        assert_eq!(plain_scalar("True"), YamlValue::Str("True".into()));
        assert_eq!(plain_scalar("-42"), YamlValue::Int(-42));
        assert_eq!(plain_scalar("+7"), YamlValue::Int(7));
        assert_eq!(
            plain_scalar("99999999999999999999"),
            YamlValue::Str("99999999999999999999".into())
        );
        for decimal in ["1.10", "5.", ".5", "1.5e3", "-2.0E-1"] {
            assert_eq!(plain_scalar(decimal), YamlValue::Decimal(decimal.into()));
        }
        for text in ["1.4.0", ".", "1e5", "0x1F", "e1", "1.5e"] {
            assert_eq!(plain_scalar(text), YamlValue::Str(text.into()), "{text}");
        }
    }

    #[test]
    fn node_conversion() {
        let value = read_value(b"a: '1'\nb: 1\nc: [x, \"y\"]\nd:\n").unwrap();
        let map = value.as_map().unwrap();
        assert_eq!(map["a"], YamlValue::Str("1".into()));
        assert_eq!(map["b"], YamlValue::Int(1));
        assert_eq!(
            map["c"],
            YamlValue::Seq(vec![YamlValue::Str("x".into()), YamlValue::Str("y".into())])
        );
        assert_eq!(map["d"], YamlValue::Null);
        assert_eq!(read_value(&[0xff, 0xfe]), Err(ReadFailure::NotUtf8));
        assert_eq!(
            from_node(YamlNode::Map(vec![
                ("k".into(), YamlNode::Null),
                ("k".into(), YamlNode::Null)
            ])),
            Err(DuplicateKey)
        );
    }
}
