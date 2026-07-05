use kv_core::Value;

pub(crate) struct PackedEntries<'a> {
    data: &'a [u8],
    off: usize,
}

impl<'a> PackedEntries<'a> {
    /// Validates the framing of `count` packed `[u32 key_len][key][u32
    /// val_len][val]` entries so iteration can run unchecked afterwards.
    pub(crate) fn validate(data: &'a [u8], count: usize) -> Result<PackedEntries<'a>, String> {
        let mut off = 0usize;
        let mut seen = 0usize;
        while off < data.len() {
            let klen = read_u32(data, &mut off)? as usize;
            off = advance(data, off, klen, "key")?;
            let vlen = read_u32(data, &mut off)? as usize;
            off = advance(data, off, vlen, "value")?;
            seen += 1;
        }
        if seen != count {
            return Err(format!(
                "entry count mismatch: found {seen}, expected {count}"
            ));
        }
        Ok(PackedEntries { data, off: 0 })
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
        Some((key, Value::Str(value)))
    }
}

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
