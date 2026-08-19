//! Strict replay (§9 `rein replay attempt --strict --compare-to-original`).
//!
//! Re-derives what can be re-derived and compares against the ledger:
//! - re-verifies every committed artifact's bytes in the CAS (read-back
//!   through a fresh handle, rehash);
//! - for a deterministic hand, re-runs the hand from the frozen ContextPack
//!   in a scratch sandbox and compares required-artifact digests;
//! - re-runs classification over the recorded receipts and compares the
//!   outcome.
//!
//! Differences are reported per §10's classes where derivable at M1:
//! `nonsemantic-receipt` (ids/timestamps), `output` (digest divergence),
//! `unexplained`. A nondeterministic hand's replay covers the first and third
//! legs and says so — stated, not silent.

use crate::engine::EngineError;
use crate::hands::{HandContext, HandRegistry};
use crate::store::Store;
use crate::workspace::Workspace;
use rein_core::canon::Sha256Digest;
use rein_core::classify::classify;
use rein_core::hand::HandRequest;
use rein_core::idempotency::IdempotencyKey;
use rein_core::ids::{AttemptId, GrantId, IdGen};
use rein_core::receipts::ReceiptBody;
use rein_core::time::LogicalMs;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiffClass {
    Output,
    Outcome,
    CasIntegrity,
    Unexplained,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayDifference {
    pub class: DiffClass,
    pub subject: String,
    pub original: String,
    pub replayed: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplayReport {
    pub attempt_id: String,
    pub hand: String,
    pub deterministic_hand: bool,
    pub artifacts_reverified: usize,
    pub differences: Vec<ReplayDifference>,
    pub notes: Vec<String>,
}

impl ReplayReport {
    pub fn matches(&self) -> bool {
        self.differences.is_empty()
    }
}

pub fn replay_attempt(
    workspace: &Workspace,
    store: &Store,
    hands: &HandRegistry,
    attempt_id: &AttemptId,
) -> Result<ReplayReport, EngineError> {
    let row = store.get_attempt(attempt_id)?;
    let pack = store.get_pack(&row.context_pack_id)?;
    let log = store.load_attempt_log(attempt_id)?;
    let cas = crate::cas::Cas::new(workspace.objects());

    let mut differences = Vec::new();
    let mut notes = Vec::new();

    // Leg 1: CAS re-verification of every committed digest.
    let mut committed: BTreeMap<String, Sha256Digest> = BTreeMap::new();
    let mut original_outcome = None;
    for e in log.iter() {
        match &e.body {
            ReceiptBody::Commit { artifacts, .. } => {
                for a in artifacts {
                    if let Some(d) = &a.readback_digest {
                        committed.insert(a.name.clone(), d.clone());
                    }
                }
            }
            ReceiptBody::Terminal { outcome, .. } => original_outcome = Some(*outcome),
            _ => {}
        }
    }
    for (name, digest) in &committed {
        if let Err(e) = cas.verify(digest) {
            differences.push(ReplayDifference {
                class: DiffClass::CasIntegrity,
                subject: name.clone(),
                original: digest.to_string(),
                replayed: format!("{e}"),
            });
        }
    }

    // Leg 2: deterministic re-execution.
    let deterministic = pack.hand.selector.starts_with("fake:");
    if deterministic {
        let scratch = workspace
            .tmp()
            .join(format!("replay-{}", attempt_id.as_str()));
        let inputs = scratch.join("inputs");
        let output = scratch.join("output");
        for d in [&inputs, &output] {
            std::fs::create_dir_all(d).map_err(|source| EngineError::Io {
                path: d.clone(),
                source,
            })?;
        }
        let mut ids = IdGen::new();
        let request = HandRequest {
            attempt_id: attempt_id.clone(),
            run_id: ids.run(),
            fence_generation: 1,
            sequence: 0,
            idempotency_key: IdempotencyKey::derive(
                &row.task_ref,
                &row.context_hash,
                row.generation,
            ),
            capability_ref: GrantId::parse("grant_replay").expect("static"),
            trace: ids.trace(),
            deadline: LogicalMs(pack.budget.per_step_timeout_ms * u64::from(pack.budget.max_steps)),
            internal_retries_disabled: true,
        };
        let env = BTreeMap::new();
        let hand = hands.get(&pack.hand.selector)?;
        hand.run(&HandContext {
            request: &request,
            contract: &pack.output_contract,
            budget: &pack.budget,
            inputs_dir: &inputs,
            output_dir: &output,
            env: &env,
        })?;
        for artifact in &pack.output_contract.required_artifacts {
            let replayed = std::fs::read(output.join(&artifact.name))
                .ok()
                .map(|b| Sha256Digest::of_bytes(&b));
            match (committed.get(&artifact.name), replayed) {
                (Some(orig), Some(new)) if orig != &new => differences.push(ReplayDifference {
                    class: DiffClass::Output,
                    subject: artifact.name.clone(),
                    original: orig.to_string(),
                    replayed: new.to_string(),
                }),
                (Some(orig), None) => differences.push(ReplayDifference {
                    class: DiffClass::Output,
                    subject: artifact.name.clone(),
                    original: orig.to_string(),
                    replayed: "(absent on replay)".to_string(),
                }),
                (None, Some(new)) => differences.push(ReplayDifference {
                    class: DiffClass::Output,
                    subject: artifact.name.clone(),
                    original: "(absent in original)".to_string(),
                    replayed: new.to_string(),
                }),
                _ => {}
            }
        }
        let _ = std::fs::remove_dir_all(&scratch);
    } else {
        notes.push(
            "hand is not deterministic: replay covers CAS re-verification and reclassification only — stated, not silent"
                .to_string(),
        );
    }

    // Leg 3: reclassification from the recorded receipts.
    match classify(&log, attempt_id, &pack.output_contract) {
        Ok(c) => {
            if let Some(orig) = original_outcome {
                if orig != c.outcome {
                    differences.push(ReplayDifference {
                        class: DiffClass::Outcome,
                        subject: "terminal outcome".to_string(),
                        original: format!("{orig:?}"),
                        replayed: format!("{:?}", c.outcome),
                    });
                }
            }
        }
        Err(_) => {
            // Attempts closed through recovery classify only via their
            // explicit terminal receipt; nothing to re-derive.
            if original_outcome.is_none() {
                differences.push(ReplayDifference {
                    class: DiffClass::Unexplained,
                    subject: "classification".to_string(),
                    original: "(no terminal receipt)".to_string(),
                    replayed: "(insufficient evidence)".to_string(),
                });
            } else {
                notes.push(
                    "outcome was set by explicit close (recovery); reclassification not derivable"
                        .to_string(),
                );
            }
        }
    }

    Ok(ReplayReport {
        attempt_id: attempt_id.as_str().to_string(),
        hand: pack.hand.selector.clone(),
        deterministic_hand: deterministic,
        artifacts_reverified: committed.len(),
        differences,
        notes,
    })
}
