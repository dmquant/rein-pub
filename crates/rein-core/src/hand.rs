//! The Hand protocol (§6), transport-independent semantics: every request
//! carries attempt id, run id, fence generation, sequence, idempotency key,
//! capability reference, trace, deadline. Events carry monotonic per-run
//! sequence numbers; duplicates are idempotent; gaps are surfaced.
//!
//! `ModelIdentity` records model_id as **two fields, requested and served** —
//! "a fallback string is not diffable and the diff is the alarm" (invariant 8,
//! decision C5). Hands are constructed with internal retries disabled; the run
//! record carries `attempts` (invariant 11 — schema here, adapter path M2).

use crate::canon::Sha256Digest;
use crate::context_pack::Budget;
use crate::idempotency::IdempotencyKey;
use crate::ids::{AttemptId, GrantId, RunId, TraceId};
use crate::time::LogicalMs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub requested: String,
    pub served: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandRequest {
    pub attempt_id: AttemptId,
    pub run_id: RunId,
    pub fence_generation: u64,
    pub sequence: u64,
    pub idempotency_key: IdempotencyKey,
    pub capability_ref: GrantId,
    pub trace: TraceId,
    pub deadline: LogicalMs,
    /// Internal retries disabled at construction (invariant 11); the record
    /// carries how many attempts the hand actually made — 1 or the alarm.
    pub internal_retries_disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelfClaim {
    Success,
    Failure,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandEvent {
    RunStarted {
        identity: ModelIdentity,
        attempts: u32,
    },
    StepStarted {
        step: u32,
    },
    StepCompleted {
        step: u32,
    },
    OutputChunk {
        stream: crate::capture::StdStream,
        bytes: Vec<u8>,
    },
    ArtifactDeclared {
        name: String,
        claimed_digest: Sha256Digest,
    },
    /// Evidence, not terminal classification (invariant 2).
    SelfReport {
        claim: SelfClaim,
    },
    RunCompleted {
        child_exit: Option<i32>,
    },
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SequencedEvent {
    pub run_id: RunId,
    pub seq: u64,
    pub at: LogicalMs,
    pub event: HandEvent,
}

#[derive(Debug, PartialEq)]
pub enum IngestOutcome {
    Accepted,
    /// Same sequence, identical payload: idempotent, ignored.
    DuplicateIgnored,
    /// The sequence jumped; the gap is recorded and surfaced, never papered
    /// over.
    AcceptedWithGap {
        missing: Vec<u64>,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum IngestError {
    #[error("event for run `{expected_run}` arrived tagged `{got_run}`")]
    WrongRun { expected_run: RunId, got_run: RunId },
    #[error("sequence {seq} seen twice with *different* payloads — duplicate is not idempotent, surfacing as conflict")]
    ConflictingDuplicate { seq: u64 },
}

/// Per-run event ledger enforcing the protocol's sequence semantics.
#[derive(Debug)]
pub struct EventLedger {
    run_id: RunId,
    events: BTreeMap<u64, SequencedEvent>,
}

impl EventLedger {
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            events: BTreeMap::new(),
        }
    }

    pub fn ingest(&mut self, ev: SequencedEvent) -> Result<IngestOutcome, IngestError> {
        if ev.run_id != self.run_id {
            return Err(IngestError::WrongRun {
                expected_run: self.run_id.clone(),
                got_run: ev.run_id,
            });
        }
        if let Some(existing) = self.events.get(&ev.seq) {
            return if existing == &ev {
                Ok(IngestOutcome::DuplicateIgnored)
            } else {
                Err(IngestError::ConflictingDuplicate { seq: ev.seq })
            };
        }
        let expected = self.events.keys().next_back().map_or(0, |k| k + 1);
        let seq = ev.seq;
        self.events.insert(seq, ev);
        if seq > expected {
            Ok(IngestOutcome::AcceptedWithGap {
                missing: (expected..seq).collect(),
            })
        } else {
            Ok(IngestOutcome::Accepted)
        }
    }

    /// All sequence numbers currently missing below the high-water mark.
    pub fn gaps(&self) -> Vec<u64> {
        match self.events.keys().next_back() {
            None => Vec::new(),
            Some(&max) => (0..=max).filter(|s| !self.events.contains_key(s)).collect(),
        }
    }

    pub fn events(&self) -> impl Iterator<Item = &SequencedEvent> {
        self.events.values()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BudgetBreach {
    pub step: u32,
    pub elapsed_ms: u64,
    pub limit_ms: u64,
}

/// Pure per-step budget check over an event stream (invariant 10's per-step
/// axis): a step whose completion (or next activity) lands beyond
/// `per_step_timeout_ms` after its start is named — the budget buys
/// *attribution*, and the innocent next stage is never blamed.
pub fn per_step_breach(events: &[SequencedEvent], budget: &Budget) -> Option<BudgetBreach> {
    let mut open: Option<(u32, u64)> = None;
    for ev in events {
        match &ev.event {
            HandEvent::StepStarted { step } => open = Some((*step, ev.at.0)),
            HandEvent::StepCompleted { step } => {
                if let Some((s, started)) = open {
                    if s == *step {
                        let elapsed = ev.at.0.saturating_sub(started);
                        if elapsed > budget.per_step_timeout_ms {
                            return Some(BudgetBreach {
                                step: s,
                                elapsed_ms: elapsed,
                                limit_ms: budget.per_step_timeout_ms,
                            });
                        }
                        open = None;
                    }
                }
            }
            _ => {
                if let Some((s, started)) = open {
                    let elapsed = ev.at.0.saturating_sub(started);
                    if elapsed > budget.per_step_timeout_ms {
                        return Some(BudgetBreach {
                            step: s,
                            elapsed_ms: elapsed,
                            limit_ms: budget.per_step_timeout_ms,
                        });
                    }
                }
            }
        }
    }
    None
}
