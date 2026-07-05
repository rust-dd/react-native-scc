use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("i/o error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("corrupt {what} at offset {offset} in {path}")]
    Corrupt {
        what: &'static str,
        offset: u64,
        path: PathBuf,
    },
    #[error("store is closed")]
    Closed,
    #[error("background writer failed: {0}")]
    Background(String),
    #[error("encryption error: {0}")]
    Crypto(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_are_stable() {
        let e = Error::Corrupt {
            what: "snapshot",
            offset: 42,
            path: PathBuf::from("/tmp/x.snap"),
        };
        assert_eq!(
            e.to_string(),
            "corrupt snapshot at offset 42 in /tmp/x.snap"
        );
        assert_eq!(Error::Closed.to_string(), "store is closed");
    }
}
