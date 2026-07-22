use std::path::Path;

use crate::crypto::{self, Cipher, FileFormat, FrameOutcome};
use crate::error::{Error, Result};
use crate::record::{self, DecodeOutcome};
use crate::snapshot;

pub(super) fn replay_wal(
    path: &Path,
    map: &crate::ValueMap,
    cipher: Option<&Cipher>,
) -> Result<u64> {
    let Some(mapped) = snapshot::map_file(path)? else {
        return Ok(0);
    };
    let data = mapped.as_ref();
    let (format, header_len) = crypto::parse_header(data);
    let encrypted = match format {
        FileFormat::Legacy => false,
        FileFormat::V1 { encrypted } => encrypted,
    };
    snapshot::check_key_matches(path, encrypted, cipher)?;
    let mut offset = header_len;
    if let Some(cipher) = cipher {
        let mut decrypted_any = false;
        while offset < data.len() {
            match cipher.decrypt_frame(&data[offset..]) {
                FrameOutcome::Frame {
                    plaintext,
                    consumed,
                } => {
                    decrypted_any = true;
                    let mut rec_off = 0usize;
                    let mut ok = true;
                    while rec_off < plaintext.len() {
                        match record::decode(&plaintext[rec_off..]) {
                            DecodeOutcome::Record { op, consumed } => {
                                record::apply(map, op);
                                rec_off += consumed;
                            }
                            DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if !ok {
                        break;
                    }
                    offset += consumed;
                }
                FrameOutcome::NeedMore => break,
                FrameOutcome::Corrupt => {
                    // An unauthenticatable first frame cannot be distinguished from a wrong key.
                    if !decrypted_any {
                        return Err(Error::Crypto(format!(
                            "cannot decrypt {} — wrong encryption key?",
                            path.display()
                        )));
                    }
                    break;
                }
            }
        }
    } else {
        while offset < data.len() {
            match record::decode(&data[offset..]) {
                DecodeOutcome::Record { op, consumed } => {
                    record::apply(map, op);
                    offset += consumed;
                }
                DecodeOutcome::NeedMore | DecodeOutcome::Corrupt => break,
            }
        }
    }
    let total = data.len();
    drop(mapped);
    if offset < total {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| Error::Io {
                path: path.to_path_buf(),
                source: e,
            })?;
        file.set_len(offset as u64).map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        file.sync_all().map_err(|e| Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    }
    Ok(offset as u64)
}
