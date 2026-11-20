use kv_core::Value;

/// Splits `[u32 len LE][bytes]` fields. Validation and iteration both use it, so they always agree on boundaries.
struct Fields<'a> {
    data: &'a [u8],
    off: usize,
}

impl<'a> Fields<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, off: 0 }
    }

    fn done(&self) -> bool {
        self.off >= self.data.len()
    }

    fn next(&mut self, what: &str) -> Result<&'a [u8], String> {
        let len = read_u32(self.data, &mut self.off)? as usize;
        let end = advance(self.data, self.off, len, what)?;
        let field = &self.data[self.off..end];
        self.off = end;
        Ok(field)
    }

    fn next_str(&mut self, what: &str) -> Result<&'a str, String> {
        std::str::from_utf8(self.next(what)?).map_err(|_| format!("{what} is not valid UTF-8"))
    }
}

fn validate_count(found: usize, expected: usize, what: &str) -> Result<(), String> {
    if found == expected {
        Ok(())
    } else {
        Err(format!(
            "{what} count mismatch: found {found}, expected {expected}"
        ))
    }
}

pub(crate) struct PackedKeys<'a> {
    fields: Fields<'a>,
    remaining: usize,
}

impl<'a> PackedKeys<'a> {
    /// Validates `count` packed `[u32 key_len][key]` entries before lookups begin.
    pub(crate) fn validate(data: &'a [u8], count: usize) -> Result<PackedKeys<'a>, String> {
        let mut fields = Fields::new(data);
        let mut seen = 0usize;
        while !fields.done() {
            fields.next_str("key")?;
            seen += 1;
        }
        validate_count(seen, count, "key")?;
        Ok(PackedKeys {
            fields: Fields::new(data),
            remaining: count,
        })
    }
}

impl<'a> Iterator for PackedKeys<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let key = self.fields.next("key").ok()?;
        self.remaining -= 1;
        // SAFETY: validate ran the same splitter over the same buffer and checked each field is UTF-8.
        Some(unsafe { std::str::from_utf8_unchecked(key) })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for PackedKeys<'_> {}

pub(crate) struct PackedEntries<'a> {
    fields: Fields<'a>,
    remaining: usize,
}

impl<'a> PackedEntries<'a> {
    /// Validates `count` packed `[u32 key_len][key][u32 val_len][val]` entries.
    pub(crate) fn validate(data: &'a [u8], count: usize) -> Result<PackedEntries<'a>, String> {
        let mut fields = Fields::new(data);
        let mut seen = 0usize;
        while !fields.done() {
            fields.next_str("key")?;
            fields.next_str("value")?;
            seen += 1;
        }
        validate_count(seen, count, "entry")?;
        Ok(PackedEntries {
            fields: Fields::new(data),
            remaining: count,
        })
    }
}

impl<'a> Iterator for PackedEntries<'a> {
    type Item = (&'a str, Value);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let key = self.fields.next("key").ok()?;
        let value = self.fields.next("value").ok()?;
        self.remaining -= 1;
        // SAFETY: validate ran the same splitter over the same buffer and checked each field is UTF-8.
        let (key, value) = unsafe {
            (
                std::str::from_utf8_unchecked(key),
                kv_core::CompactString::from_utf8_unchecked(value),
            )
        };
        Some((key, Value::Str(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for PackedEntries<'_> {}

fn read_u32(data: &[u8], off: &mut usize) -> Result<u32, String> {
    let end = off
        .checked_add(4)
        .ok_or_else(|| "offset overflow".to_string())?;
    if end > data.len() {
        return Err("truncated length field".to_string());
    }
    let v = u32::from_le_bytes(data[*off..end].try_into().unwrap());
    *off = end;
    Ok(v)
}

fn advance(data: &[u8], off: usize, by: usize, what: &str) -> Result<usize, String> {
    match off.checked_add(by) {
        Some(end) if end <= data.len() => Ok(end),
        _ => Err(format!("truncated {what}")),
    }
}

/// Decodes the transaction batch buffer built by the TS layer: `[u32 count]`
/// then `count` ops, each `[u8 kind][u32 key_len][key]` and, for `kind == 1`
/// (set), `[u8 tag][u32 val_len][val]`. `kind == 0` is delete.
pub(crate) fn decode_batch(data: &[u8]) -> Result<Vec<kv_core::BatchOp>, String> {
    let mut off = 0usize;
    let count = read_u32(data, &mut off)? as usize;
    let mut ops = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        let kind = *data.get(off).ok_or("truncated batch op kind")?;
        off += 1;
        let klen = read_u32(data, &mut off)? as usize;
        let key_end = advance(data, off, klen, "batch key")?;
        let key = std::str::from_utf8(&data[off..key_end])
            .map_err(|_| "batch key is not UTF-8".to_string())?
            .to_string();
        off = key_end;
        match kind {
            0 => ops.push(kv_core::BatchOp::Delete { key }),
            1 => {
                let tag = *data.get(off).ok_or("truncated batch tag")?;
                off += 1;
                let vlen = read_u32(data, &mut off)? as usize;
                let val_end = advance(data, off, vlen, "batch value")?;
                let value = kv_core::Value::decode(tag, &data[off..val_end])
                    .ok_or("invalid batch value encoding")?;
                off = val_end;
                ops.push(kv_core::BatchOp::Set { key, value });
            }
            _ => return Err(format!("unknown batch op kind {kind}")),
        }
    }
    if off != data.len() {
        return Err("trailing bytes in batch buffer".to_string());
    }
    Ok(ops)
}
