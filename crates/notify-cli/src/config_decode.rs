//! Private draft JSON boundary; no schema, defaults, lookup or installed behavior.
use serde_core::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::{cell::Cell, fmt};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Bytes,
    Syntax,
    Depth,
    Structure,
    Duplicate,
    Integer,
    Root,
}
// No Debug/Clone: decoded keys and values can include rejected private metadata.
pub(super) enum Value {
    Null,
    Bool(bool),
    Integer(u64),
    Text(Zeroizing<String>),
    Array(Vec<Value>),
    Object(Vec<(Zeroizing<String>, Value)>),
}

fn preflight(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() || bytes.len() > 65536 {
        return Err(Error::Bytes);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Syntax)?;
    if text.starts_with('\u{feff}') {
        return Err(Error::Syntax);
    }
    let (mut quoted, mut escaped, mut depth, mut i) = (false, false, 0usize, 0);
    while i < bytes.len() {
        let byte = bytes[i];
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > 8 {
                        return Err(Error::Depth);
                    }
                }
                b']' | b'}' => depth = depth.checked_sub(1).ok_or(Error::Syntax)?,
                b'-' | b'0'..=b'9' => {
                    let start = i;
                    while i < bytes.len() && !b" \t\r\n,]}:".contains(&bytes[i]) {
                        i += 1;
                    }
                    let token = &text[start..i];
                    if token.starts_with('-')
                        || (token.len() > 1 && token.starts_with('0'))
                        || !token.bytes().all(|byte| byte.is_ascii_digit())
                        || token.parse::<u64>().is_err()
                    {
                        return Err(Error::Integer);
                    }
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    Ok(())
}

struct Seed<'a>(&'a Cell<Option<Error>>);
impl Seed<'_> {
    fn fail<E: serde_core::de::Error>(&self, category: Error) -> E {
        self.0.set(Some(category));
        E::custom("invalid private configuration")
    }
}
impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Value;
    fn deserialize<D: serde_core::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded private JSON value")
    }
    fn visit_unit<E: serde_core::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: serde_core::de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_u64<E: serde_core::de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Integer(value))
    }
    fn visit_i64<E: serde_core::de::Error>(self, _: i64) -> Result<Value, E> {
        Err(self.fail(Error::Integer))
    }
    fn visit_f64<E: serde_core::de::Error>(self, _: f64) -> Result<Value, E> {
        Err(self.fail(Error::Integer))
    }
    fn visit_str<E: serde_core::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(Value::Text(Zeroizing::new(value.to_owned())))
    }
    fn visit_string<E: serde_core::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(Value::Text(Zeroizing::new(value)))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element_seed(Seed(self.0))? {
            if values.len() == 128 {
                return Err(self.fail(Error::Structure));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut values: Vec<(Zeroizing<String>, Value)> = Vec::new();
        while let Some(key) = map.next_key::<String>()? {
            let key = Zeroizing::new(key);
            if values.len() == 128 {
                return Err(self.fail(Error::Structure));
            }
            if values.iter().any(|(other, _)| **other == *key) {
                return Err(self.fail(Error::Duplicate));
            }
            let value = map.next_value_seed(Seed(self.0))?;
            values.push((key, value));
        }
        Ok(Value::Object(values))
    }
}

pub(super) fn decode(bytes: &[u8]) -> Result<Value, Error> {
    preflight(bytes)?;
    let category = Cell::new(None);
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let result = Seed(&category)
        .deserialize(&mut parser)
        .map_err(|_| category.get().unwrap_or(Error::Syntax))?;
    parser.end().map_err(|_| Error::Syntax)?;
    if !matches!(result, Value::Object(_)) {
        return Err(Error::Root);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rejected(bytes: &[u8], expected: Error) {
        assert!(matches!(decode(bytes), Err(actual) if actual == expected));
    }
    #[test]
    fn decodes_examples_and_owned_scalar_values() {
        for example in [
            include_bytes!("../../../docs/proposals/examples/local-webhook-config.json").as_slice(),
            include_bytes!("../../../docs/proposals/examples/local-mixed-config.json").as_slice(),
        ] {
            assert!(matches!(decode(example), Ok(Value::Object(_))));
        }
        let source =
            br#"{"s":"\u0061","b":true,"n":18446744073709551615,"a":[null,false]}"#.to_vec();
        let value = decode(&source).unwrap();
        drop(source);
        let Value::Object(fields) = value else {
            panic!("object")
        };
        assert!(matches!(&fields[0].1, Value::Text(s) if s.as_str() == "a"));
        assert!(matches!(fields[1].1, Value::Bool(true)));
        assert!(matches!(fields[2].1, Value::Integer(u64::MAX)));
        assert!(
            matches!(&fields[3].1, Value::Array(a) if matches!(a.as_slice(), [Value::Null, Value::Bool(false)]))
        );
    }
    #[test]
    fn rejects_duplicates_numbers_and_syntax_without_echoing_input() {
        rejected(br#"{"private":"x","\u0070rivate":"y"}"#, Error::Duplicate);
        for token in ["-0", "-1", "01", "1.0", "1e0", "18446744073709551616"] {
            rejected(format!("{{\"n\":{token}}}").as_bytes(), Error::Integer);
        }
        for source in [
            b"\xff".as_slice(),
            b"\xef\xbb\xbf{}",
            b"{}{}",
            br#"{"s":"\uD800"}"#,
            b"{]",
            b"{",
            b"{\"n\":NaN}",
        ] {
            rejected(source, Error::Syntax);
        }
        rejected(b"[]", Error::Root);
        assert_eq!(
            format!("{:?}", decode(br#"{"secret":"unclosed"#).err().unwrap()),
            "Syntax"
        );
    }
    #[test]
    fn enforces_byte_depth_and_collection_boundaries() {
        rejected(b"", Error::Bytes);
        let mut source = b"{}".to_vec();
        source.resize(65536, b' ');
        assert!(decode(&source).is_ok());
        source.push(b' ');
        rejected(&source, Error::Bytes);
        for (depth, ok) in [(7, true), (8, false)] {
            let text = format!("{{\"a\":{}null{}}}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(decode(text.as_bytes()).is_ok(), ok);
        }
        assert!(decode(br#"{"s":"[[[[[[[[[\\\""}"#).is_ok());
        for (count, ok) in [(128, true), (129, false)] {
            let array = format!("{{\"a\":[{}]}}", vec!["null"; count].join(","));
            assert_eq!(decode(array.as_bytes()).is_ok(), ok);
            let object = format!(
                "{{{}}}",
                (0..count)
                    .map(|n| format!("\"k{n}\":null"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            assert_eq!(decode(object.as_bytes()).is_ok(), ok);
        }
    }
}
