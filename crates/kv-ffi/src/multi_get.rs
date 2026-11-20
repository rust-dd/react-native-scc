use std::mem::size_of;

use kv_core::Value;

use crate::batch::PackedKeys;
use crate::buffers::vec_to_raw;
use crate::{SccKvStore, data_slice, guard, store};

const MISSING_VALUE: u32 = u32::MAX;

/// Reads packed `[u32 key_len LE][key]` keys into `[u32 value_len LE][value]` entries (free with
/// `scc_kv_free`); `0xFFFF_FFFF` marks missing, expired or non-string. 0 ok, -1 error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn scc_kv_get_many_str(
    h: *mut SccKvStore,
    data: *const u8,
    len: usize,
    count: usize,
    out_data: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    guard(-1, || {
        if out_data.is_null() || out_len.is_null() {
            return Err("output pointer is null".to_string());
        }
        unsafe {
            *out_data = std::ptr::null_mut();
            *out_len = 0;
        }

        let s = unsafe { store(h) }?;
        let data = unsafe { data_slice(data, len) }?;
        let keys = PackedKeys::validate(data, count)?;
        let minimum_len = count
            .checked_mul(size_of::<u32>())
            .ok_or_else(|| "packed multi-get output length overflow".to_string())?;
        let mut output = Vec::new();
        output
            .try_reserve(minimum_len)
            .map_err(|e| format!("packed multi-get output allocation failed: {e}"))?;

        for key in keys {
            match s.store.with_value(key, |value| match value {
                Value::Str(value) => append_result(&mut output, Some(value.as_bytes())),
                _ => append_result(&mut output, None),
            }) {
                Some(result) => result?,
                None => append_result(&mut output, None)?,
            }
        }

        let (ptr, len) = vec_to_raw(output);
        unsafe {
            *out_data = ptr;
            *out_len = len;
        }
        Ok(0)
    })
}

fn append_result(output: &mut Vec<u8>, value: Option<&[u8]>) -> Result<(), String> {
    let (encoded_len, bytes) = match value {
        Some(bytes) => {
            let encoded_len = u32::try_from(bytes.len())
                .map_err(|_| "string value exceeds the packed multi-get limit".to_string())?;
            if encoded_len == MISSING_VALUE {
                return Err("string value exceeds the packed multi-get limit".to_string());
            }
            (encoded_len, bytes)
        }
        None => (MISSING_VALUE, &[][..]),
    };
    let additional = size_of::<u32>()
        .checked_add(bytes.len())
        .ok_or_else(|| "packed multi-get output length overflow".to_string())?;
    output
        .len()
        .checked_add(additional)
        .ok_or_else(|| "packed multi-get output length overflow".to_string())?;
    output
        .try_reserve(additional)
        .map_err(|e| format!("packed multi-get output allocation failed: {e}"))?;
    output.extend_from_slice(&encoded_len.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}
