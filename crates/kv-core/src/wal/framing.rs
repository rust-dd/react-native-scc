use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::crypto::{Cipher, MAX_FRAME_PLAINTEXT};
use crate::error::{Error, Result};

pub(super) fn write_encrypted_frames(
    file: &mut File,
    path: &Path,
    cipher: &Cipher,
    records: &[u8],
) -> Result<u64> {
    let mut framed = Vec::new();
    let mut frame_start = 0usize;
    let mut record_start = 0usize;
    let mut written = 0u64;
    while record_start < records.len() {
        let record_end = encoded_record_end(records, record_start)?;
        if record_end - record_start > MAX_FRAME_PLAINTEXT {
            return Err(Error::Crypto(format!(
                "encoded WAL record exceeds encryption frame limit: {} bytes",
                record_end - record_start
            )));
        }
        if record_start > frame_start && record_end - frame_start > MAX_FRAME_PLAINTEXT {
            let frame_len = write_frame(
                file,
                path,
                cipher,
                &records[frame_start..record_start],
                &mut framed,
            )?;
            written = written
                .checked_add(frame_len)
                .ok_or_else(|| Error::Crypto("encrypted WAL length overflow".to_string()))?;
            frame_start = record_start;
        }
        record_start = record_end;
    }
    if frame_start < records.len() {
        let frame_len = write_frame(file, path, cipher, &records[frame_start..], &mut framed)?;
        written = written
            .checked_add(frame_len)
            .ok_or_else(|| Error::Crypto("encrypted WAL length overflow".to_string()))?;
    }
    Ok(written)
}

fn encoded_record_end(records: &[u8], offset: usize) -> Result<usize> {
    let header_end = offset
        .checked_add(8)
        .ok_or_else(|| Error::Crypto("WAL record length overflow".to_string()))?;
    let header = records
        .get(offset..header_end)
        .ok_or_else(|| Error::Crypto("truncated encoded WAL record".to_string()))?;
    let payload_len = u32::from_le_bytes(header[..4].try_into().unwrap());
    if payload_len == 0 || payload_len > crate::record::MAX_PAYLOAD {
        return Err(Error::Crypto(format!(
            "invalid encoded WAL record length: {payload_len}"
        )));
    }
    let record_end = header_end
        .checked_add(payload_len as usize)
        .ok_or_else(|| Error::Crypto("WAL record length overflow".to_string()))?;
    if record_end > records.len() {
        return Err(Error::Crypto("truncated encoded WAL record".to_string()));
    }
    Ok(record_end)
}

fn write_frame(
    file: &mut File,
    path: &Path,
    cipher: &Cipher,
    plaintext: &[u8],
    framed: &mut Vec<u8>,
) -> Result<u64> {
    framed.clear();
    cipher.encrypt_frame(plaintext, framed)?;
    file.write_all(framed).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(framed.len() as u64)
}
