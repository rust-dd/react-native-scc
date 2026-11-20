use std::fs::File;
use std::io::{ErrorKind, IoSlice, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, AeadInOut, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use crate::error::{Error, Result};

pub(crate) const MAGIC: [u8; 6] = *b"SCCKV\x01";
pub(crate) const HEADER_LEN: usize = 8;
pub(crate) const FLAG_ENCRYPTED: u8 = 1;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
const FRAME_HEADER_LEN: usize = 4 + NONCE_LEN;
pub(crate) const MAX_FRAME_PLAINTEXT: usize = crate::record::MAX_PAYLOAD as usize + 8;
const MAX_FRAME_CIPHERTEXT: usize = MAX_FRAME_PLAINTEXT + TAG_LEN;

/// Derives a 32-byte cipher key from an arbitrary passphrase (SHA-256).
pub fn derive_encryption_key(passphrase: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    Sha256::digest(passphrase).into()
}

pub(crate) fn header_bytes(encrypted: bool) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[..6].copy_from_slice(&MAGIC);
    h[6] = if encrypted { FLAG_ENCRYPTED } else { 0 };
    h
}

pub(crate) enum FileFormat {
    /// Pre-header files: raw records from byte 0, always plaintext.
    Legacy,
    V1 {
        encrypted: bool,
    },
}

pub(crate) fn parse_header(data: &[u8]) -> (FileFormat, usize) {
    if data.len() >= HEADER_LEN && data[..6] == MAGIC {
        (
            FileFormat::V1 {
                encrypted: data[6] & FLAG_ENCRYPTED != 0,
            },
            HEADER_LEN,
        )
    } else {
        (FileFormat::Legacy, 0)
    }
}

pub(crate) struct Cipher(ChaCha20Poly1305);

pub(crate) enum FrameOutcome {
    Frame { plaintext: Vec<u8>, consumed: usize },
    NeedMore,
    Corrupt,
}

impl Cipher {
    pub(crate) fn new(key: &[u8; 32]) -> Cipher {
        Cipher(ChaCha20Poly1305::new(&Key::from(*key)))
    }

    /// Appends one frame: `[u32 ct_len LE][12B nonce][ciphertext + tag]`.
    pub(crate) fn encrypt_frame(&self, plaintext: &[u8], out: &mut Vec<u8>) -> Result<()> {
        let start = out.len();
        out.extend_from_slice(&[0; FRAME_HEADER_LEN]);
        out.extend_from_slice(plaintext);
        match self.seal_in_place(&mut out[start + FRAME_HEADER_LEN..]) {
            Ok((header, tag)) => {
                out[start..start + FRAME_HEADER_LEN].copy_from_slice(&header);
                out.extend_from_slice(&tag);
                Ok(())
            }
            Err(e) => {
                out.truncate(start);
                Err(e)
            }
        }
    }

    /// Encrypts `plaintext` in place and returns the frame header and tag around it.
    fn seal_in_place(
        &self,
        plaintext: &mut [u8],
    ) -> Result<([u8; FRAME_HEADER_LEN], [u8; TAG_LEN])> {
        let ct_len = checked_ciphertext_len(plaintext.len())?;
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
        let tag = self
            .0
            .encrypt_inout_detached(&Nonce::from(nonce), &[], plaintext.into())
            .map_err(|_| Error::Crypto("encryption failed".to_string()))?;
        let mut header = [0u8; FRAME_HEADER_LEN];
        header[..4].copy_from_slice(&ct_len.to_le_bytes());
        header[4..].copy_from_slice(&nonce);
        let mut tag_bytes = [0u8; TAG_LEN];
        tag_bytes.copy_from_slice(&tag);
        Ok((header, tag_bytes))
    }

    /// Decrypts the frame at the start of `data`. Authentication failure is
    /// `Corrupt`; a partial frame is `NeedMore` (torn tail).
    pub(crate) fn decrypt_frame(&self, data: &[u8]) -> FrameOutcome {
        if data.len() < 4 + NONCE_LEN {
            return FrameOutcome::NeedMore;
        }
        let ct_len = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if ct_len == 0 || ct_len as usize > MAX_FRAME_CIPHERTEXT {
            return FrameOutcome::Corrupt;
        }
        let total = 4 + NONCE_LEN + ct_len as usize;
        if data.len() < total {
            return FrameOutcome::NeedMore;
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&data[4..4 + NONCE_LEN]);
        let ct = &data[4 + NONCE_LEN..total];
        match self.0.decrypt(&Nonce::from(nonce), ct) {
            Ok(plaintext) => FrameOutcome::Frame {
                plaintext,
                consumed: total,
            },
            Err(_) => FrameOutcome::Corrupt,
        }
    }
}

/// Encrypts `records` in place and writes them as frames of at most `limit`
/// plaintext bytes, split on record boundaries. Empty input still writes one frame.
pub(crate) fn write_encrypted_records(
    file: &mut File,
    path: &Path,
    cipher: &Cipher,
    records: &mut [u8],
    limit: usize,
) -> Result<u64> {
    if records.is_empty() {
        return write_sealed_frame(file, path, cipher, records);
    }
    let mut frame_start = 0usize;
    let mut record_start = 0usize;
    let mut written = 0u64;
    while record_start < records.len() {
        let record_end = encoded_record_end(records, record_start)?;
        if record_end - record_start > limit {
            return Err(Error::Crypto(format!(
                "encoded record exceeds encryption frame limit: {} bytes",
                record_end - record_start
            )));
        }
        if record_start > frame_start && record_end - frame_start > limit {
            written +=
                write_sealed_frame(file, path, cipher, &mut records[frame_start..record_start])?;
            frame_start = record_start;
        }
        record_start = record_end;
    }
    written += write_sealed_frame(file, path, cipher, &mut records[frame_start..])?;
    Ok(written)
}

fn encoded_record_end(records: &[u8], offset: usize) -> Result<usize> {
    let header_end = offset
        .checked_add(8)
        .ok_or_else(|| Error::Crypto("record length overflow".to_string()))?;
    let header = records
        .get(offset..header_end)
        .ok_or_else(|| Error::Crypto("truncated encoded record".to_string()))?;
    let payload_len = u32::from_le_bytes(header[..4].try_into().unwrap());
    if payload_len == 0 || payload_len > crate::record::MAX_PAYLOAD {
        return Err(Error::Crypto(format!(
            "invalid encoded record length: {payload_len}"
        )));
    }
    let record_end = header_end
        .checked_add(payload_len as usize)
        .ok_or_else(|| Error::Crypto("record length overflow".to_string()))?;
    if record_end > records.len() {
        return Err(Error::Crypto("truncated encoded record".to_string()));
    }
    Ok(record_end)
}

fn write_sealed_frame(
    file: &mut File,
    path: &Path,
    cipher: &Cipher,
    plaintext: &mut [u8],
) -> Result<u64> {
    let (header, tag) = cipher.seal_in_place(plaintext)?;
    let mut parts = [
        IoSlice::new(&header),
        IoSlice::new(plaintext),
        IoSlice::new(&tag),
    ];
    write_all_vectored(file, &mut parts).map_err(|source| Error::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok((FRAME_HEADER_LEN + plaintext.len() + TAG_LEN) as u64)
}

fn write_all_vectored(file: &mut File, mut parts: &mut [IoSlice<'_>]) -> std::io::Result<()> {
    IoSlice::advance_slices(&mut parts, 0);
    while !parts.is_empty() {
        match file.write_vectored(parts) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => IoSlice::advance_slices(&mut parts, n),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn checked_ciphertext_len(plaintext_len: usize) -> Result<u32> {
    if plaintext_len > MAX_FRAME_PLAINTEXT {
        return Err(Error::Crypto(format!(
            "encryption frame is too large: {plaintext_len} bytes"
        )));
    }
    let ciphertext_len = plaintext_len
        .checked_add(TAG_LEN)
        .ok_or_else(|| Error::Crypto("encryption frame length overflow".to_string()))?;
    u32::try_from(ciphertext_len)
        .map_err(|_| Error::Crypto("encryption frame length overflow".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip_and_tamper_detection() {
        let key = derive_encryption_key(b"secret");
        let cipher = Cipher::new(&key);
        let mut out = Vec::new();
        cipher.encrypt_frame(b"hello records", &mut out).unwrap();
        cipher.encrypt_frame(b"second", &mut out).unwrap();

        let FrameOutcome::Frame {
            plaintext,
            consumed,
        } = cipher.decrypt_frame(&out)
        else {
            panic!("expected frame");
        };
        assert_eq!(plaintext, b"hello records");
        let FrameOutcome::Frame { plaintext, .. } = cipher.decrypt_frame(&out[consumed..]) else {
            panic!("expected second frame");
        };
        assert_eq!(plaintext, b"second");

        let mut tampered = out.clone();
        let mid = 4 + 12 + 3;
        tampered[mid] ^= 0xff;
        assert!(matches!(
            cipher.decrypt_frame(&tampered),
            FrameOutcome::Corrupt
        ));
        assert!(matches!(
            cipher.decrypt_frame(&out[..10]),
            FrameOutcome::NeedMore
        ));

        let wrong = Cipher::new(&derive_encryption_key(b"other"));
        assert!(matches!(wrong.decrypt_frame(&out), FrameOutcome::Corrupt));
    }

    #[test]
    fn header_parses() {
        let h = header_bytes(true);
        let (fmt, off) = parse_header(&h);
        assert!(matches!(fmt, FileFormat::V1 { encrypted: true }));
        assert_eq!(off, HEADER_LEN);
        let (fmt, off) = parse_header(b"garbage!");
        assert!(matches!(fmt, FileFormat::Legacy));
        assert_eq!(off, 0);
    }

    #[test]
    fn frame_length_accepts_a_maximum_record_and_rejects_overflow() {
        assert_eq!(
            checked_ciphertext_len(MAX_FRAME_PLAINTEXT).unwrap() as usize,
            MAX_FRAME_CIPHERTEXT
        );
        assert!(checked_ciphertext_len(MAX_FRAME_PLAINTEXT + 1).is_err());
        assert!(checked_ciphertext_len(usize::MAX).is_err());
    }
}
