use crate::value::Value;

pub(crate) const MAX_PAYLOAD: u32 = 64 * 1024 * 1024;

pub(crate) enum Op<'a> {
    Set {
        key: &'a str,
        value: &'a Value,
    },
    Delete {
        key: &'a str,
    },
    Clear,
    SetTtl {
        key: &'a str,
        value: &'a Value,
        expires_at_ms: u64,
    },
}

#[derive(Debug, PartialEq)]
pub(crate) enum OwnedOp {
    Set {
        key: String,
        value: Value,
    },
    Delete {
        key: String,
    },
    Clear,
    SetTtl {
        key: String,
        value: Value,
        expires_at_ms: u64,
    },
}

#[derive(Debug, PartialEq)]
pub(crate) enum DecodeOutcome {
    Record { op: OwnedOp, consumed: usize },
    NeedMore,
    Corrupt,
}

pub(crate) fn encode(op: &Op, out: &mut Vec<u8>) {
    let frame_start = out.len();
    out.extend_from_slice(&[0u8; 8]);
    let payload_start = out.len();
    match op {
        Op::Set { key, value } => {
            out.push(0);
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.push(value.tag());
            value.encode_into(out);
        }
        Op::Delete { key } => {
            out.push(1);
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
        }
        Op::Clear => {
            out.push(2);
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        Op::SetTtl {
            key,
            value,
            expires_at_ms,
        } => {
            out.push(3);
            out.extend_from_slice(&(key.len() as u32).to_le_bytes());
            out.extend_from_slice(key.as_bytes());
            out.extend_from_slice(&expires_at_ms.to_le_bytes());
            out.push(value.tag());
            value.encode_into(out);
        }
    }
    let payload_len = (out.len() - payload_start) as u32;
    let crc = crc32fast::hash(&out[payload_start..]);
    out[frame_start..frame_start + 4].copy_from_slice(&payload_len.to_le_bytes());
    out[frame_start + 4..frame_start + 8].copy_from_slice(&crc.to_le_bytes());
}

pub(crate) fn decode(buf: &[u8]) -> DecodeOutcome {
    if buf.len() < 8 {
        return DecodeOutcome::NeedMore;
    }
    let payload_len = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if payload_len == 0 || payload_len > MAX_PAYLOAD {
        return DecodeOutcome::Corrupt;
    }
    let crc = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let total = 8 + payload_len as usize;
    if buf.len() < total {
        return DecodeOutcome::NeedMore;
    }
    let payload = &buf[8..total];
    if crc32fast::hash(payload) != crc {
        return DecodeOutcome::Corrupt;
    }
    match parse_payload(payload) {
        Some(op) => DecodeOutcome::Record {
            op,
            consumed: total,
        },
        None => DecodeOutcome::Corrupt,
    }
}

fn parse_payload(payload: &[u8]) -> Option<OwnedOp> {
    if payload.len() < 5 {
        return None;
    }
    let op = payload[0];
    let key_len = u32::from_le_bytes(payload[1..5].try_into().unwrap()) as usize;
    let key_end = 5usize.checked_add(key_len)?;
    if payload.len() < key_end {
        return None;
    }
    let key = std::str::from_utf8(&payload[5..key_end]).ok()?;
    match op {
        0 => {
            let tag = *payload.get(key_end)?;
            let value = Value::decode(tag, &payload[key_end + 1..])?;
            Some(OwnedOp::Set {
                key: key.to_string(),
                value,
            })
        }
        1 if payload.len() == key_end => Some(OwnedOp::Delete {
            key: key.to_string(),
        }),
        2 if key_len == 0 && payload.len() == 5 => Some(OwnedOp::Clear),
        3 => {
            let ttl_end = key_end.checked_add(8)?;
            if payload.len() < ttl_end + 1 {
                return None;
            }
            let expires_at_ms = u64::from_le_bytes(payload[key_end..ttl_end].try_into().unwrap());
            let tag = payload[ttl_end];
            let value = Value::decode(tag, &payload[ttl_end + 1..])?;
            Some(OwnedOp::SetTtl {
                key: key.to_string(),
                value,
                expires_at_ms,
            })
        }
        _ => None,
    }
}

pub(crate) fn apply(map: &crate::ValueMap, op: OwnedOp) {
    match op {
        OwnedOp::Set { key, value } => insert_slot(map, key, value, 0),
        OwnedOp::SetTtl {
            key,
            value,
            expires_at_ms,
        } => insert_slot(map, key, value, expires_at_ms),
        OwnedOp::Delete { key } => {
            map.remove_sync(&key);
        }
        OwnedOp::Clear => {
            map.clear_sync();
        }
    }
}

fn insert_slot(map: &crate::ValueMap, key: String, value: Value, expires_at_ms: u64) {
    let slot = crate::Slot {
        value,
        expires_at_ms,
    };
    match map.entry_sync(key) {
        scc::hash_map::Entry::Occupied(mut o) => *o.get_mut() = slot,
        scc::hash_map::Entry::Vacant(v) => {
            v.insert_entry(slot);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_one(op: &Op) -> Vec<u8> {
        let mut buf = Vec::new();
        encode(op, &mut buf);
        buf
    }

    #[test]
    fn round_trips_all_ops() {
        let v = Value::Str("val".into());
        let cases = vec![
            (
                encode_one(&Op::Set {
                    key: "k1",
                    value: &v,
                }),
                OwnedOp::Set {
                    key: "k1".into(),
                    value: v.clone(),
                },
            ),
            (
                encode_one(&Op::Delete { key: "gone" }),
                OwnedOp::Delete { key: "gone".into() },
            ),
            (encode_one(&Op::Clear), OwnedOp::Clear),
        ];
        for (buf, expected) in cases {
            match decode(&buf) {
                DecodeOutcome::Record { op, consumed } => {
                    assert_eq!(op, expected);
                    assert_eq!(consumed, buf.len());
                }
                other => panic!("expected record, got {other:?}"),
            }
        }
    }

    #[test]
    fn decodes_consecutive_records() {
        let mut buf = Vec::new();
        let v = Value::Num(1.0);
        encode(
            &Op::Set {
                key: "a",
                value: &v,
            },
            &mut buf,
        );
        let first_len = buf.len();
        encode(&Op::Delete { key: "a" }, &mut buf);
        match decode(&buf) {
            DecodeOutcome::Record { consumed, .. } => assert_eq!(consumed, first_len),
            other => panic!("expected record, got {other:?}"),
        }
        match decode(&buf[first_len..]) {
            DecodeOutcome::Record { op, .. } => {
                assert_eq!(op, OwnedOp::Delete { key: "a".into() })
            }
            other => panic!("expected record, got {other:?}"),
        }
    }

    #[test]
    fn every_truncation_is_needmore() {
        let v = Value::Bytes(vec![1, 2, 3]);
        let buf = encode_one(&Op::Set {
            key: "key",
            value: &v,
        });
        for cut in 0..buf.len() {
            assert_eq!(decode(&buf[..cut]), DecodeOutcome::NeedMore, "cut at {cut}");
        }
    }

    #[test]
    fn bitflip_is_corrupt() {
        let v = Value::Str("value".into());
        let clean = encode_one(&Op::Set {
            key: "key",
            value: &v,
        });
        for i in 0..clean.len() {
            let mut buf = clean.clone();
            buf[i] ^= 0x01;
            match decode(&buf) {
                DecodeOutcome::Record { op, .. } => {
                    panic!("bitflip at {i} decoded as {op:?}")
                }
                DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => {}
            }
        }
    }

    #[test]
    fn insane_length_is_corrupt() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(MAX_PAYLOAD + 1).to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]);
        assert_eq!(decode(&buf), DecodeOutcome::Corrupt);
        let mut zero = Vec::new();
        zero.extend_from_slice(&0u32.to_le_bytes());
        zero.extend_from_slice(&[0u8; 4]);
        assert_eq!(decode(&zero), DecodeOutcome::Corrupt);
    }
}
