//! Secrets are references in durable state (invariant 28, decision C6).
//!
//! There is no secret-value type in this crate's serialized surface at all:
//! [`crate::ids::SecretRefId`] is the only secret-shaped thing a schema can
//! carry, and it is a name. The [`Redactor`] holds live values transiently for
//! scrubbing and is deliberately **not** serializable — the compiler enforces
//! what a convention would merely request.

use crate::entities::RedactionReport;
use crate::ids::SecretRefId;

/// Scrubs known secret values out of capture streams, producing a redaction
/// report (counts per ref, never values).
#[derive(Debug)]
pub struct Redactor {
    entries: Vec<(SecretRefId, String)>,
}

impl Redactor {
    pub fn new(entries: Vec<(SecretRefId, String)>) -> Self {
        Self { entries }
    }

    /// Replace every occurrence of every known secret value with
    /// `«redacted:<ref>»`.
    pub fn scrub(&self, text: &str) -> (String, RedactionReport) {
        let mut out = text.to_string();
        let mut report = RedactionReport::default();
        for (id, value) in &self.entries {
            if value.is_empty() {
                continue;
            }
            let count = out.matches(value.as_str()).count() as u64;
            if count > 0 {
                out = out.replace(value.as_str(), &format!("«redacted:{id}»"));
                *report.replacements.entry(id.to_string()).or_insert(0) += count;
            }
        }
        (out, report)
    }

    /// Does this text still carry any known secret value? The M0 pure core of
    /// the `secret-scan` validator: a hit quarantines the artifact
    /// (a validator verdict plus a receipt — not a lifecycle state).
    pub fn scan(&self, text: &str) -> Option<SecretRefId> {
        self.entries
            .iter()
            .find(|(_, value)| !value.is_empty() && text.contains(value.as_str()))
            .map(|(id, _)| id.clone())
    }
}
