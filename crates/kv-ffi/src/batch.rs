use kv_core::Value;

pub(crate) struct PackedKeys<'a> {
    data: &'a [u8],
    off: usize,
    remaining: usize,
}

impl<'a> PackedKeys<'a> {
    /// Validates `count` packed `[u32 key_len][key]` entries before lookups begin.
    pub(crate) fn validate(data: &'a [u8], count: usize) -> Result<PackedKeys<'a>, String> {
        let mut off = 0usize;
        let mut seen = 0usize;
        while off < data.len() {
            let key_len = read_u32(data, &mut off)? as usize;
            let key_end = advance(data, off, key_len, "key")?;
            std::str::from_utf8(&data[off..key_end])
                .map_err(|_| "key is not valid UTF-8".to_string())?;
            off = key_end;
            seen = seen
                .checked_add(1)
                .ok_or_else(|| "key count overflow".to_string())?;
        }
        if seen != count {
            return Err(format!(
                "key count mismatch: found {seen}, expected {count}"
            ));
        }
        Ok(PackedKeys {
            data,
            off: 0,
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
        let key_len =
            u32::from_le_bytes(self.data[self.off..self.off + 4].try_into().unwrap()) as usize;
        self.off += 4;
        let key_bytes = &self.data[self.off..self.off + key_len];
        self.off += key_len;
        self.remaining -= 1;
        // SAFETY: `validate` checked this exact slice before constructing the iterator.
        Some(unsafe { std::str::from_utf8_unchecked(key_bytes) })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for PackedKeys<'_> {}

pub(crate) struct PackedEntries<'a> {
    data: &'a [u8],
    off: usize,
    remaining: usize,
}

impl<'a> PackedEntries<'a> {
    /// Validates the framing of `count` packed `[u32 key_len][key][u32
    /// val_len][val]` entries so iteration can run unchecked afterwards.
    pub(crate) fn validate(data: &'a [u8], count: usize) -> Result<PackedEntries<'a>, String> {
        let mut off = 0usize;
        let mut seen = 0usize;
        while off < data.len() {
            let klen = read_u32(data, &mut off)? as usize;
            let key_end = advance(data, off, klen, "key")?;
            std::str::from_utf8(&data[off..key_end])
                .map_err(|_| "key is not valid UTF-8".to_string())?;
            off = key_end;
            let vlen = read_u32(data, &mut off)? as usize;
            let value_end = advance(data, off, vlen, "value")?;
            std::str::from_utf8(&data[off..value_end])
                .map_err(|_| "value is not valid UTF-8".to_string())?;
            off = value_end;
            seen += 1;
        }
        if seen != count {
            return Err(format!(
                "entry count mismatch: found {seen}, expected {count}"
            ));
        }
        Ok(PackedEntries {
            data,
            off: 0,
            remaining: count,
        })
    }
}

impl<'a> Iterator for PackedEntries<'a> {
    type Item = (&'a str, Value);

    fn next(&mut self) -> Option<Self::Item> {
        if self.off >= self.data.len() {
            return None;
        }
        let klen =
            u32::from_le_bytes(self.data[self.off..self.off + 4].try_into().unwrap()) as usize;
        self.off += 4;
        let key_bytes = &self.data[self.off..self.off + klen];
        self.off += klen;
        let vlen =
            u32::from_le_bytes(self.data[self.off..self.off + 4].try_into().unwrap()) as usize;
        self.off += 4;
        let val_bytes = &self.data[self.off..self.off + vlen];
        self.off += vlen;
        debug_assert!(std::str::from_utf8(key_bytes).is_ok(), "key must be UTF-8");
        debug_assert!(
            std::str::from_utf8(val_bytes).is_ok(),
            "value must be UTF-8"
        );
        let key = unsafe { std::str::from_utf8_unchecked(key_bytes) };
        let value = unsafe { kv_core::CompactString::from_utf8_unchecked(val_bytes) };
        self.remaining -= 1;
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
