//! Filesystem content-addressed store (`.rein/objects/`).
//!
//! Write path: stream to a temp file, fsync, rename into place — the digest
//! is the address. Read-back for commit verification goes through
//! [`Cas::read_verified`], which opens a **fresh handle by digest path** (a
//! handle the writer did not own, invariant 3) and rehashes what it actually
//! read.

use rein_core::canon::Sha256Digest;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum CasError {
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("object {0} is absent")]
    Absent(Sha256Digest),
    #[error(
        "object {digest} read back as {actual} — store corruption, evidence retained at {path}"
    )]
    Corrupt {
        digest: Sha256Digest,
        actual: Sha256Digest,
        path: PathBuf,
    },
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> CasError + '_ {
    move |source| CasError::Io {
        path: path.to_path_buf(),
        source,
    }
}

#[derive(Debug, Clone)]
pub struct Cas {
    root: PathBuf,
}

impl Cas {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn path_of(&self, digest: &Sha256Digest) -> PathBuf {
        let hex = digest.as_str().trim_start_matches("sha256:");
        self.root.join("sha256").join(&hex[..2]).join(&hex[2..])
    }

    pub fn contains(&self, digest: &Sha256Digest) -> bool {
        self.path_of(digest).exists()
    }

    /// Store bytes; idempotent (same bytes, same address).
    pub fn put(&self, bytes: &[u8]) -> Result<Sha256Digest, CasError> {
        let digest = Sha256Digest::of_bytes(bytes);
        let dest = self.path_of(&digest);
        if dest.exists() {
            return Ok(digest);
        }
        let parent = dest.parent().expect("cas path has parent");
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        let tmp = parent.join(format!(
            ".staging-{}",
            digest.as_str().trim_start_matches("sha256:")
        ));
        {
            let mut f = std::fs::File::create(&tmp).map_err(io_err(&tmp))?;
            f.write_all(bytes).map_err(io_err(&tmp))?;
            f.sync_all().map_err(io_err(&tmp))?;
        }
        std::fs::rename(&tmp, &dest).map_err(io_err(&dest))?;
        Ok(digest)
    }

    /// Read through a fresh handle and rehash: the digest of what was
    /// *actually read*, compared against the address. This is the read-back
    /// leg of invariant 3 — never trust the writer's memory of the bytes.
    pub fn read_verified(&self, digest: &Sha256Digest) -> Result<Vec<u8>, CasError> {
        let path = self.path_of(digest);
        let mut f = std::fs::File::open(&path).map_err(|_| CasError::Absent(digest.clone()))?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes).map_err(io_err(&path))?;
        let actual = Sha256Digest::of_bytes(&bytes);
        if &actual != digest {
            return Err(CasError::Corrupt {
                digest: digest.clone(),
                actual,
                path,
            });
        }
        Ok(bytes)
    }

    /// Verify without materializing (doctor / evidence verify).
    pub fn verify(&self, digest: &Sha256Digest) -> Result<(), CasError> {
        self.read_verified(digest).map(|_| ())
    }
}
