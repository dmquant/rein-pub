//! Optional AGORA publish (§12: the only Agora surface Rein grows —
//! `evidence publish`). Publication is never required for an Attempt to run
//! or close; an AGORA outage cannot stop authorized execution.
//!
//! The party key is read from a path in configRoot (never the workspace,
//! invariant 27), sent as a bearer token, and never logged.

use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    #[error("no AGORA key at {0} — set agora_key_path in configRoot config.toml")]
    NoKey(String),
    #[error("tls: {0}")]
    Tls(String),
    #[error("publish failed: {0}")]
    Request(String),
}

pub struct AgoraClient {
    hub: String,
    key: String,
    agent: ureq::Agent,
}

impl AgoraClient {
    pub fn new(hub: &str, key_path: &Path) -> Result<Self, PublishError> {
        let key = std::fs::read_to_string(key_path)
            .map_err(|_| PublishError::NoKey(key_path.display().to_string()))?
            .trim()
            .to_string();
        let tls = native_tls::TlsConnector::new().map_err(|e| PublishError::Tls(e.to_string()))?;
        Ok(Self {
            hub: hub.trim_end_matches('/').to_string(),
            key,
            agent: ureq::AgentBuilder::new()
                .tls_connector(Arc::new(tls))
                .timeout(std::time::Duration::from_secs(20))
                .build(),
        })
    }

    fn redact(&self, text: &str) -> String {
        text.replace(&self.key, "«redacted:agora-key»")
    }

    /// Post a typed message to a room. Returns the hub's response.
    pub fn post_message(
        &self,
        room_id: &str,
        kind: &str,
        body_md: &str,
        evidence_json: Value,
    ) -> Result<Value, PublishError> {
        let payload = json!({
            "kind": kind,
            "body_md": body_md,
            "evidence_json": evidence_json,
        });
        let resp = self
            .agent
            .post(&format!("{}/api/rooms/{room_id}/messages", self.hub))
            .set("authorization", &format!("Bearer {}", self.key))
            .set("content-type", "application/json")
            .set("user-agent", "rein/0.1 (evidence publish)")
            .send_string(&payload.to_string())
            .map_err(|e| PublishError::Request(self.redact(&e.to_string())))?;
        resp.into_json()
            .map_err(|e| PublishError::Request(self.redact(&e.to_string())))
    }
}

/// Build the publish payload for an evidence bundle: the summary a room can
/// verify — bundle digest, manifest digests, outcome, what would refute it.
pub fn bundle_publish_body(
    attempt_id: &str,
    outcome: &str,
    bundle_path: &str,
    bundle_digest: &str,
    artifacts: &[(String, String)],
) -> (String, Value) {
    let mut artifact_lines = String::new();
    for (name, digest) in artifacts {
        artifact_lines.push_str(&format!("- `{name}` `{digest}`\n"));
    }
    let body = format!(
        "**Evidence bundle: {attempt_id}** — outcome `{outcome}`.\n\n\
         Bundle `{bundle_path}` sha256 `{bundle_digest}`.\n\nArtifacts:\n{artifact_lines}\n\
         **What would refute this:** `rein evidence verify` on the bundle reporting any digest, \
         sequence or receipt-chain problem."
    );
    let evidence = json!({
        "attempt": attempt_id,
        "outcome": outcome,
        "bundle_sha256": bundle_digest,
        "artifacts": artifacts.iter().map(|(n, d)| json!({"name": n, "digest": d})).collect::<Vec<_>>(),
    });
    (body, evidence)
}
