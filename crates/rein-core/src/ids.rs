//! Typed identifiers and references (§3, §5).
//!
//! Identifiers are minted only through an injected [`IdGen`] — there is no
//! ambient randomness in this crate, so every M0 path is deterministic.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IdError {
    #[error("expected prefix `{expected}` on `{got}`")]
    WrongPrefix { expected: &'static str, got: String },
    #[error("empty suffix after prefix `{prefix}`")]
    EmptySuffix { prefix: &'static str },
    #[error("validator ref `{0}` must be `name@version` with both parts non-empty")]
    BadValidatorRef(String),
}

macro_rules! typed_ref {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn parse(s: &str) -> Result<Self, IdError> {
                let Some(rest) = s.strip_prefix($prefix) else {
                    return Err(IdError::WrongPrefix { expected: $prefix, got: s.to_string() });
                };
                if rest.is_empty() {
                    return Err(IdError::EmptySuffix { prefix: $prefix });
                }
                Ok(Self(s.to_string()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                Self::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

typed_ref!(WorkspaceRef, "ws:");
typed_ref!(MissionRef, "mission:");
typed_ref!(EpochRef, "epoch:");
typed_ref!(PlanRef, "plan:");
typed_ref!(
    /// A task *version* reference, e.g. `task:dcf-nvda@2`. A semantic change is
    /// a new version, never a retry (invariant 6).
    TaskRef, "task:");
typed_ref!(AttemptId, "attempt_");
typed_ref!(RunId, "run_");
typed_ref!(ContextPackId, "ctx_");
typed_ref!(GrantId, "grant_");
typed_ref!(ArtifactRef, "artifact:");
typed_ref!(ReceiptId, "rcpt_");
typed_ref!(HandRef, "hand:");
typed_ref!(SecretRefId, "secret-ref:");
typed_ref!(TraceId, "trace_");

/// `name@version`, e.g. `citation-closure@1`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ValidatorRef(String);

impl ValidatorRef {
    pub fn parse(s: &str) -> Result<Self, IdError> {
        match s.split_once('@') {
            Some((name, ver)) if !name.is_empty() && !ver.is_empty() => Ok(Self(s.to_string())),
            _ => Err(IdError::BadValidatorRef(s.to_string())),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ValidatorRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ValidatorRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Deterministic id mint. The only constructor of fresh identifiers in M0.
#[derive(Debug, Default)]
pub struct IdGen {
    counter: u64,
}

impl IdGen {
    pub fn new() -> Self {
        Self::default()
    }

    fn bump(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }

    pub fn attempt(&mut self) -> AttemptId {
        AttemptId(format!("attempt_{:06}", self.bump()))
    }

    pub fn run(&mut self) -> RunId {
        RunId(format!("run_{:06}", self.bump()))
    }

    pub fn receipt(&mut self) -> ReceiptId {
        ReceiptId(format!("rcpt_{:06}", self.bump()))
    }

    pub fn context_pack(&mut self) -> ContextPackId {
        ContextPackId(format!("ctx_{:06}", self.bump()))
    }

    pub fn trace(&mut self) -> TraceId {
        TraceId(format!("trace_{:06}", self.bump()))
    }
}
