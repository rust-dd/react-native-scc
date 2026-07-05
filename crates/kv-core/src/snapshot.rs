use std::fs;
use std::io::Write;
use std::path::Path;

use crate::crypto::{self, Cipher, FileFormat, FrameOutcome};
use crate::error::{Error, Result};
use crate::record::{self, DecodeOutcome, Op};

fn io_err(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

pub(crate) fn write_atomic(
    path: &Path,
    map: &crate::ValueMap,
    cipher: Option<&Cipher>,
) -> Result<u64> {
    let now = crate::now_ms();
    let mut records = Vec::new();
    map.iter_sync(|k, slot| {
        if !slot.is_expired(now) {
            if slot.expires_at_ms == 0 {
                record::encode(
                    &Op::Set {
                        key: k,
                        value: &slot.value,
                    },
                    &mut records,
                );
            } else {
                record::encode(
                    &Op::SetTtl {
                        key: k,
                        value: &slot.value,
                        expires_at_ms: slot.expires_at_ms,
                    },
                    &mut records,
                );
            }
        }
        true
    });
    let mut buf = Vec::with_capacity(crypto::HEADER_LEN + records.len() + 32);
    buf.extend_from_slice(&crypto::header_bytes(cipher.is_some()));
    match cipher {
        Some(cipher) => cipher.encrypt_frame(&records, &mut buf)?,
        None => buf.extend_from_slice(&records),
    }
    let tmp = path.with_extension("tmp");
    let mut file = fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
    file.write_all(&buf).map_err(|e| io_err(&tmp, e))?;
    file.sync_all().map_err(|e| io_err(&tmp, e))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| io_err(path, e))?;
    if let Some(dir) = path.parent()
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
    Ok(buf.len() as u64)
}

/// Maps a file read-only for recovery-time parsing without copying it into
/// memory first. `Ok(None)` when the file is missing or empty.
///
/// The mapping assumes no concurrent writer — guaranteed at open time because
/// the registry hands out a single store per (dir, id) and the WAL thread has
/// not started yet.
pub(crate) fn map_file(path: &Path) -> Result<Option<memmap2::Mmap>> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_err(path, e)),
    };
    let len = file.metadata().map_err(|e| io_err(path, e))?.len();
    if len == 0 {
        return Ok(None);
    }
    let mmap = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| io_err(path, e))?;
    Ok(Some(mmap))
}

pub(crate) fn check_key_matches(
    path: &Path,
    file_encrypted: bool,
    cipher: Option<&Cipher>,
) -> Result<()> {
    match (file_encrypted, cipher.is_some()) {
        (true, false) => Err(Error::Crypto(format!(
            "{} is encrypted but no encryption key was provided",
            path.display()
        ))),
        (false, true) => Err(Error::Crypto(format!(
            "{} is not encrypted but an encryption key was provided",
            path.display()
        ))),
        _ => Ok(()),
    }
}

fn decode_all(path: &Path, data: &[u8], base_offset: usize, map: &crate::ValueMap) -> Result<()> {
    let mut offset = 0usize;
    while offset < data.len() {
        match record::decode(&data[offset..]) {
            DecodeOutcome::Record { op, consumed } => {
                record::apply(map, op);
                offset += consumed;
            }
            DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => {
                return Err(Error::Corrupt {
                    what: "snapshot",
                    offset: (base_offset + offset) as u64,
                    path: path.to_path_buf(),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn load(path: &Path, map: &crate::ValueMap, cipher: Option<&Cipher>) -> Result<u64> {
    let Some(mapped) = map_file(path)? else {
        return Ok(0);
    };
    let data: &[u8] = &mapped;
    let (format, header_len) = crypto::parse_header(data);
    let encrypted = match format {
        FileFormat::Legacy => false,
        FileFormat::V1 { encrypted } => encrypted,
    };
    check_key_matches(path, encrypted, cipher)?;
    match cipher {
        Some(cipher) => {
            let mut offset = header_len;
            while offset < data.len() {
                match cipher.decrypt_frame(&data[offset..]) {
                    FrameOutcome::Frame {
                        plaintext,
                        consumed,
                    } => {
                        decode_all(path, &plaintext, offset, map)?;
                        offset += consumed;
                    }
                    FrameOutcome::NeedMore | FrameOutcome::Corrupt => {
                        return Err(Error::Corrupt {
                            what: "snapshot",
                            offset: offset as u64,
                            path: path.to_path_buf(),
                        });
                    }
                }
            }
        }
        None => decode_all(path, &data[header_len..], header_len, map)?,
    }
    Ok(data.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn sample_map() -> crate::ValueMap {
        let map = crate::new_value_map();
        let _ = map.insert_sync("a".to_string(), crate::slot(Value::Num(1.0)));
        let _ = map.insert_sync("b".to_string(), crate::slot(Value::Str("two".into())));
        let _ = map.insert_sync("c".to_string(), crate::slot(Value::Bytes(vec![3, 3, 3])));
        map
    }

    #[test]
    fn round_trips_map() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.snap");
        let written = write_atomic(&path, &sample_map(), None).unwrap();
        assert!(written > 0);
        assert!(!path.with_extension("tmp").exists());

        let loaded = crate::new_value_map();
        let read = load(&path, &loaded, None).unwrap();
        assert_eq!(read, written);
        assert_eq!(loaded.len(), 3);
        assert_eq!(
            loaded.read_sync("b", |_, s| s.value.clone()),
            Some(Value::Str("two".into()))
        );
    }

    #[test]
    fn missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let map = crate::new_value_map();
        assert_eq!(load(&dir.path().join("nope.snap"), &map, None).unwrap(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn overwrites_previous_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.snap");
        write_atomic(&path, &sample_map(), None).unwrap();
        let small = crate::new_value_map();
        let _ = small.insert_sync("only".to_string(), crate::slot(Value::Bool(true)));
        write_atomic(&path, &small, None).unwrap();
        let loaded = crate::new_value_map();
        load(&path, &loaded, None).unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(loaded.contains_sync("only"));
    }

    #[test]
    fn corrupt_snapshot_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.snap");
        write_atomic(&path, &sample_map(), None).unwrap();
        let mut data = fs::read(&path).unwrap();
        let mid = data.len() / 2;
        data[mid] ^= 0xff;
        fs::write(&path, &data).unwrap();
        let map = crate::new_value_map();
        assert!(matches!(
            load(&path, &map, None),
            Err(Error::Corrupt {
                what: "snapshot",
                ..
            })
        ));
    }

    #[test]
    fn truncated_snapshot_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.snap");
        write_atomic(&path, &sample_map(), None).unwrap();
        let data = fs::read(&path).unwrap();
        fs::write(&path, &data[..data.len() - 3]).unwrap();
        let map = crate::new_value_map();
        assert!(matches!(
            load(&path, &map, None),
            Err(Error::Corrupt { .. })
        ));
    }
}
