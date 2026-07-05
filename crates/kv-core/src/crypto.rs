use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use crate::error::{Error, Result};

pub(crate) const MAGIC: [u8; 6] = *b"SCCKV\x01";
pub(crate) const HEADER_LEN: usize = 8;
pub(crate) const FLAG_ENCRYPTED: u8 = 1;
const NONCE_LEN: usize = 12;
const MAX_FRAME: u32 = 64 * 1024 * 1024;

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
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|e| Error::Crypto(e.to_string()))?;
        let ct = self
            .0
            .encrypt(&Nonce::from(nonce), plaintext)
            .map_err(|_| Error::Crypto("encryption failed".to_string()))?;
        out.extend_from_slice(&(ct.len() as u32).to_le_bytes());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(())
    }

    /// Decrypts the frame at the start of `data`. Authentication failure is
    /// `Corrupt`; a partial frame is `NeedMore` (torn tail).
    pub(crate) fn decrypt_frame(&self, data: &[u8]) -> FrameOutcome {
        if data.len() < 4 + NONCE_LEN {
            return FrameOutcome::NeedMore;
        }
        let ct_len = u32::from_le_bytes(data[0..4].try_into().unwrap());
        if ct_len == 0 || ct_len > MAX_FRAME {
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
}
