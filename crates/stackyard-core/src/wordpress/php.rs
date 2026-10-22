//! PHP's `serialize()` format, for the WordPress options and meta values
//! Stackyard reads and changes (payment gateway settings, a one-time login).
//! Objects are kept as raw bytes and written back unchanged.

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    /// Kept as written, so it is written back exactly.
    Float(String),
    Str(Vec<u8>),
    Array(Vec<(Value, Value)>),
    /// Anything else (objects, references), byte for byte.
    Raw(Vec<u8>),
}

impl Value {
    pub fn str(text: &str) -> Value {
        Value::Str(text.as_bytes().to_vec())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => std::str::from_utf8(s).ok(),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Str(s) => std::str::from_utf8(s).ok()?.trim().parse().ok(),
            _ => None,
        }
    }

    /// The value under a string key of an array.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Array(items) => items
                .iter()
                .find(|(k, _)| k.as_str() == Some(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }

    /// Sets a string key of an array, adding it at the end if missing.
    pub fn set(&mut self, key: &str, value: Value) {
        if let Value::Array(items) = self {
            if let Some(slot) = items.iter_mut().find(|(k, _)| k.as_str() == Some(key)) {
                slot.1 = value;
            } else {
                items.push((Value::str(key), value));
            }
        }
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut Vec<u8>) {
        match self {
            Value::Null => out.extend_from_slice(b"N;"),
            Value::Bool(b) => out.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
            Value::Int(n) => out.extend_from_slice(format!("i:{n};").as_bytes()),
            Value::Float(f) => out.extend_from_slice(format!("d:{f};").as_bytes()),
            Value::Str(s) => {
                out.extend_from_slice(format!("s:{}:\"", s.len()).as_bytes());
                out.extend_from_slice(s);
                out.extend_from_slice(b"\";");
            }
            Value::Array(items) => {
                out.extend_from_slice(format!("a:{}:{{", items.len()).as_bytes());
                for (k, v) in items {
                    k.write(out);
                    v.write(out);
                }
                out.push(b'}');
            }
            Value::Raw(raw) => out.extend_from_slice(raw),
        }
    }
}

/// Reads one whole serialized value; `None` if `data` is anything else.
pub fn unserialize(data: &[u8]) -> Option<Value> {
    let (value, end) = parse(data, 0, 0)?;
    (end == data.len()).then_some(value)
}

fn parse(data: &[u8], at: usize, depth: usize) -> Option<(Value, usize)> {
    if depth > 256 {
        return None;
    }
    let kind = *data.get(at)?;
    if kind == b'N' {
        return data
            .get(at + 1)
            .filter(|b| **b == b';')
            .map(|_| (Value::Null, at + 2));
    }
    if data.get(at + 1) != Some(&b':') {
        return None;
    }
    let field = |from: usize, stop: u8| -> Option<(&[u8], usize)> {
        let len = data.get(from..)?.iter().position(|&b| b == stop)?;
        Some((&data[from..from + len], from + len))
    };
    match kind {
        b'b' | b'i' | b'd' => {
            let (raw, end) = field(at + 2, b';')?;
            let text = std::str::from_utf8(raw).ok()?;
            let value = match kind {
                b'b' => Value::Bool(text == "1"),
                b'i' => Value::Int(text.parse().ok()?),
                _ => Value::Float(text.to_string()),
            };
            Some((value, end + 1))
        }
        b's' => {
            let (len, colon) = field(at + 2, b':')?;
            let len: usize = std::str::from_utf8(len).ok()?.parse().ok()?;
            let start = colon + 2;
            if data.get(colon + 1) != Some(&b'"') {
                return None;
            }
            let body = data.get(start..start + len)?.to_vec();
            if data.get(start + len..start + len + 2)? != b"\";" {
                return None;
            }
            Some((Value::Str(body), start + len + 2))
        }
        b'a' => {
            let (count, colon) = field(at + 2, b':')?;
            let count: usize = std::str::from_utf8(count).ok()?.parse().ok()?;
            if data.get(colon + 1) != Some(&b'{') {
                return None;
            }
            let mut pos = colon + 2;
            let mut items = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                let (k, next) = parse(data, pos, depth + 1)?;
                let (v, next) = parse(data, next, depth + 1)?;
                items.push((k, v));
                pos = next;
            }
            if data.get(pos) != Some(&b'}') {
                return None;
            }
            Some((Value::Array(items), pos + 1))
        }
        _ => {
            // Objects and the rest: find their end with the replace engine's
            // parser, which knows every kind, and keep them as they are.
            let mut out = Vec::new();
            let rules = super::replace::Rules::default();
            let end = super::replace::copy_serialized(data, at, &mut out, &rules)?;
            Some((Value::Raw(data[at..end].to_vec()), end))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_gateway_settings() {
        let raw = br#"a:4:{s:7:"enabled";s:3:"yes";s:8:"testmode";s:2:"no";s:5:"order";i:3;s:3:"obj";O:8:"stdClass":0:{}}"#;
        let mut value = unserialize(raw).unwrap();
        assert_eq!(value.get("enabled").and_then(Value::as_str), Some("yes"));
        assert_eq!(value.get("order").and_then(Value::as_int), Some(3));
        assert_eq!(value.serialize(), raw.to_vec());
        value.set("testmode", Value::str("yes"));
        value.set("new", Value::Bool(true));
        let again = unserialize(&value.serialize()).unwrap();
        assert_eq!(again.get("testmode").and_then(Value::as_str), Some("yes"));
        assert_eq!(again.get("new"), Some(&Value::Bool(true)));
        assert!(unserialize(b"not serialized").is_none());
        assert!(unserialize(b"s:5:\"abc\";").is_none());
    }
}
