//! The validator registry. Validators run over **read-back bytes only** —
//! never staging files (§7) — and their verdicts are receipts, not opinions.
//!
//! M1 built-ins: `artifact-wellformed@1` (min-bytes + JSON well-formedness
//! for application/json), `secret-scan@1` (invariant 28, real). The finance
//! validators (input-closure, citation-closure, …) land with rein-finance at
//! M2 through the same trait.

use rein_core::context_pack::{ContextPack, RequiredArtifact};
use rein_core::ids::ValidatorRef;
use rein_core::receipts::ValidatorVerdict;
use rein_core::secretref::Redactor;
use std::collections::BTreeMap;

/// Everything a validator may look at: the read-back bytes of the artifact
/// under validation, its declaration, the full artifact set (read-back), and
/// the frozen pack.
pub struct ValidationInput<'a> {
    pub artifact: &'a RequiredArtifact,
    pub bytes: &'a [u8],
    pub all_artifacts: &'a BTreeMap<String, Vec<u8>>,
    pub pack: &'a ContextPack,
}

pub trait ArtifactValidator: Send + Sync {
    fn name(&self) -> &ValidatorRef;
    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict;
}

pub struct ValidatorRegistry {
    validators: Vec<Box<dyn ArtifactValidator>>,
}

impl ValidatorRegistry {
    pub fn empty() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    /// M1 built-ins. `redactor` carries the broker's known secret values.
    pub fn builtin(redactor: Redactor) -> Self {
        let mut r = Self::empty();
        r.register(Box::new(WellFormed {
            name: ValidatorRef::parse("artifact-wellformed@1").expect("static ref"),
        }));
        r.register(Box::new(SecretScan {
            name: ValidatorRef::parse("secret-scan@1").expect("static ref"),
            redactor,
        }));
        r
    }

    pub fn register(&mut self, v: Box<dyn ArtifactValidator>) {
        self.validators.push(v);
    }

    pub fn get(&self, name: &ValidatorRef) -> Option<&dyn ArtifactValidator> {
        self.validators
            .iter()
            .map(|v| v.as_ref())
            .find(|v| v.name() == name)
    }

    /// Run one declared validator. An undeclared/uninstalled validator is a
    /// *failure with its own words* — never silently passed (invariant 31's
    /// spirit at the validation layer).
    pub fn run(&self, name: &ValidatorRef, input: &ValidationInput<'_>) -> ValidatorVerdict {
        match self.get(name) {
            Some(v) => v.validate(input),
            None => ValidatorVerdict::Failed {
                reason: format!("validator `{name}` is not installed in this workspace"),
            },
        }
    }
}

struct WellFormed {
    name: ValidatorRef,
}

impl ArtifactValidator for WellFormed {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        if let Some(min) = input.artifact.min_bytes {
            if (input.bytes.len() as u64) < min {
                return ValidatorVerdict::Failed {
                    reason: format!("{} bytes < declared min_bytes {min}", input.bytes.len()),
                };
            }
        }
        if input.artifact.media_type == "application/json" {
            if let Err(e) = serde_json::from_slice::<serde_json::Value>(input.bytes) {
                return ValidatorVerdict::Failed {
                    reason: format!("declared application/json does not parse: {e}"),
                };
            }
        }
        ValidatorVerdict::Passed
    }
}

struct SecretScan {
    name: ValidatorRef,
    redactor: Redactor,
}

impl ArtifactValidator for SecretScan {
    fn name(&self) -> &ValidatorRef {
        &self.name
    }

    fn validate(&self, input: &ValidationInput<'_>) -> ValidatorVerdict {
        let text = String::from_utf8_lossy(input.bytes);
        match self.redactor.scan(&text) {
            Some(hit) => ValidatorVerdict::Quarantined {
                reason: format!("artifact carries the value of `{hit}`"),
            },
            None => ValidatorVerdict::Passed,
        }
    }
}
