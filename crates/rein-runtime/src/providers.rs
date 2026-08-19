//! `providers.lock` (§11, invariant 8): the workspace's resolved pins.
//! Generation is deterministic except one labeled timestamp.

use rein_core::pins::ProviderPin;
use rein_core::time::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const LOCK_SCHEMA: &str = "rein.providers-lock/v1";

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("providers.lock: {0}")]
    Parse(String),
    #[error("pin `{name}`: file {path} hashes to {actual}, lock says {locked}")]
    DigestMismatch {
        name: String,
        path: String,
        actual: String,
        locked: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProvidersLock {
    pub schema: String,
    /// The one labeled nondeterministic field.
    pub generated_at: Option<Timestamp>,
    pub pins: BTreeMap<String, ProviderPin>,
    /// Free-form evidence, e.g. the sibling-estate git commit backing path deps
    /// (recorded per the Q2 position).
    #[serde(default)]
    pub notes: BTreeMap<String, String>,
}

impl ProvidersLock {
    pub fn new() -> Self {
        Self {
            schema: LOCK_SCHEMA.to_string(),
            generated_at: None,
            pins: BTreeMap::new(),
            notes: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, LockError> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| LockError::Parse(e.to_string()))
    }

    pub fn save(&self, path: &Path) -> Result<(), LockError> {
        let text =
            serde_json::to_string_pretty(self).map_err(|e| LockError::Parse(e.to_string()))?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Verify digest pins whose coordinate names a local file
    /// (`file:<path>`): rehash and compare. Service pins have nothing local
    /// to verify — their evidence is per-call (invariant 8).
    pub fn verify(&self) -> Result<Vec<String>, LockError> {
        let mut notes = Vec::new();
        for (name, pin) in &self.pins {
            match pin {
                ProviderPin::Digest { coordinate, digest } => {
                    if let Some(path) = coordinate.strip_prefix("file:") {
                        let bytes = std::fs::read(path)?;
                        let actual = rein_core::canon::Sha256Digest::of_bytes(&bytes);
                        if &actual != digest {
                            return Err(LockError::DigestMismatch {
                                name: name.clone(),
                                path: path.to_string(),
                                actual: actual.to_string(),
                                locked: digest.to_string(),
                            });
                        }
                        notes.push(format!("{name}: digest verified against {path}"));
                    } else {
                        notes.push(format!("{name}: digest pin (no local bytes to re-verify)"));
                    }
                }
                ProviderPin::Service { pin_method, .. } => {
                    notes.push(format!(
                        "{name}: service pin — method `{pin_method}`, evidence recorded per call"
                    ));
                }
            }
        }
        Ok(notes)
    }
}
