//! Provider pins (invariant 8): exact where bytes exist; declared method where not.
//!
//! By construction there is no third shape — a pin either carries a digest or
//! names its pin method (with the served version recorded per call as evidence,
//! at the runtime). A bare coordinate does not deserialize.

use crate::canon::Sha256Digest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderPin {
    /// Coordinate + digest — binaries, artifacts, corpora.
    Digest {
        coordinate: String,
        digest: Sha256Digest,
    },
    /// A remote service that cannot be digest-pinned names its method; the
    /// served version header is recorded per call in receipts (runtime, M2).
    Service {
        coordinate: String,
        pin_method: String,
    },
}

impl ProviderPin {
    pub fn coordinate(&self) -> &str {
        match self {
            Self::Digest { coordinate, .. } | Self::Service { coordinate, .. } => coordinate,
        }
    }

    /// True when the pin is exact (digest-backed).
    pub fn is_exact(&self) -> bool {
        matches!(self, Self::Digest { .. })
    }
}
