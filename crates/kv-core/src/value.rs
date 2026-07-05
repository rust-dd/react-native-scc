use compact_str::CompactString;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(CompactString),
    Num(f64),
    Bool(bool),
    Bytes(Vec<u8>),
    Json(CompactString),
}

impl Value {
    /// Wire tag for the FFI/WAL encoding: 0=Str, 1=Num, 2=Bool, 3=Bytes, 4=Json.
    pub fn tag(&self) -> u8 {
        match self {
            Value::Str(_) => 0,
            Value::Num(_) => 1,
            Value::Bool(_) => 2,
            Value::Bytes(_) => 3,
            Value::Json(_) => 4,
        }
    }

    /// Appends the value's wire bytes (Str/Json: UTF-8, Num: f64 LE, Bool: one byte, Bytes: raw).
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        match self {
            Value::Str(s) | Value::Json(s) => out.extend_from_slice(s.as_bytes()),
            Value::Num(n) => out.extend_from_slice(&n.to_le_bytes()),
            Value::Bool(b) => out.push(*b as u8),
            Value::Bytes(b) => out.extend_from_slice(b),
        }
    }

    /// Inverse of `encode_into`; `None` on unknown tag, bad length, or invalid UTF-8.
    pub fn decode(tag: u8, bytes: &[u8]) -> Option<Value> {
        match tag {
            0 => CompactString::from_utf8(bytes).ok().map(Value::Str),
            1 => Some(Value::Num(f64::from_le_bytes(bytes.try_into().ok()?))),
            2 => match bytes {
                [0] => Some(Value::Bool(false)),
                [1] => Some(Value::Bool(true)),
                _ => None,
            },
            3 => Some(Value::Bytes(bytes.to_vec())),
            4 => CompactString::from_utf8(bytes).ok().map(Value::Json),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(v: Value) {
        let mut buf = Vec::new();
        v.encode_into(&mut buf);
        assert_eq!(Value::decode(v.tag(), &buf), Some(v));
    }

    #[test]
    fn round_trips_every_variant() {
        round_trip(Value::Str("hello ünnep".into()));
        round_trip(Value::Num(1.2345678901234567));
        round_trip(Value::Num(f64::NEG_INFINITY));
        round_trip(Value::Bool(true));
        round_trip(Value::Bool(false));
        round_trip(Value::Bytes(vec![0, 255, 1, 2]));
        round_trip(Value::Json(r#"{"a":[1,2]}"#.into()));
        round_trip(Value::Str("".into()));
        round_trip(Value::Bytes(Vec::new()));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(Value::decode(9, b"x"), None);
        assert_eq!(Value::decode(1, b"short"), None);
        assert_eq!(Value::decode(2, &[2]), None);
        assert_eq!(Value::decode(2, &[]), None);
        assert_eq!(Value::decode(0, &[0xff, 0xfe]), None);
    }
}
