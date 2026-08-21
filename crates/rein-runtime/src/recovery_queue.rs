//! The recovery console's queue (§8): typed anomalies detected from the
//! ledger, each with a diagnosis and exactly three safe actions. Forbidden:
//! force success — there is no code path for it here or anywhere.
//!
//! Invariant 25: the stale-run check is tolerant of the boundary's own
//! latency — the threshold defaults far above any local scheduling jitter,
//! "or the check measures the latency instead of the property," and a
//! warning that fires on every run burns the credibility of every other
//! warning.

use crate::store::{Store, StoreError};
use rein_core::ids::AttemptId;
use rein_core::receipts::ReceiptBody;
use rein_core::state::{resolve_state, AnomalyKind, AttemptState};
use rein_core::time::Timestamp;
use serde::Serialize;

/// Default staleness threshold: 10 minutes — far above any async-boundary
/// latency this runtime can produce (invariant 25's tolerance).
pub const DEFAULT_STALE_AFTER_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, Serialize)]
pub struct AnomalyReport {
    pub attempt_id: String,
    pub anomaly: AnomalyKind,
    pub state: String,
    pub diagnosis: String,
    /// The three safe actions, always; nothing else exists.
    pub actions: [&'static str; 3],
}

fn ms_between(a: Timestamp, b: Timestamp) -> i64 {
    b.unix_millis() - a.unix_millis()
}

/// Scan the ledger for typed anomalies.
pub fn recovery_queue(
    store: &Store,
    now: Timestamp,
    stale_after_ms: i64,
) -> Result<Vec<AnomalyReport>, StoreError> {
    let log = store.load_full_log()?;
    let mut out = Vec::new();

    for row in store.list_attempts()? {
        let aid: AttemptId = row.attempt_id.clone();
        let Ok(state) = resolve_state(&log, &aid) else {
            out.push(AnomalyReport {
                attempt_id: aid.as_str().to_string(),
                anomaly: AnomalyKind::StaleRun,
                state: "unresolvable".to_string(),
                diagnosis: "receipt chain does not resolve — the ledger is the authority and it is inconsistent".to_string(),
                actions: ACTIONS,
            });
            continue;
        };
        let last_at = log
            .for_attempt(&aid)
            .map(|e| e.at)
            .max()
            .unwrap_or(row.created_at);
        let age_ms = ms_between(last_at, now);

        match state {
            AttemptState::RecoveryPending => {
                // Already in recovery: surface with its recorded anomaly.
                let anomaly = log
                    .for_attempt(&aid)
                    .filter_map(|e| match &e.body {
                        ReceiptBody::Transition {
                            cause:
                                rein_core::state::TransitionCauseRecord::RecoveryEntered { anomaly },
                            ..
                        } => Some(anomaly.clone()),
                        _ => None,
                    })
                    .last()
                    .unwrap_or(AnomalyKind::UnknownAfterDisconnect);
                out.push(AnomalyReport {
                    attempt_id: aid.as_str().to_string(),
                    anomaly: anomaly.clone(),
                    state: format!("{state:?}"),
                    diagnosis: format!(
                        "in recovery_pending for {age_ms}ms; fence generation {} current; no verdict may be inferred (invariant 5)",
                        rein_core::fence::current_generation(&log, &aid).unwrap_or(0)
                    ),
                    actions: ACTIONS,
                });
            }
            AttemptState::Running
            | AttemptState::CommitPending
            | AttemptState::Validating
            | AttemptState::Classifying => {
                // A live pipeline phase with no fresh receipts: stale run —
                // but only beyond the tolerance (invariant 25).
                if age_ms > stale_after_ms {
                    out.push(AnomalyReport {
                        attempt_id: aid.as_str().to_string(),
                        anomaly: AnomalyKind::StaleRun,
                        state: format!("{state:?}"),
                        diagnosis: format!(
                            "no receipts for {age_ms}ms (tolerance {stale_after_ms}ms) in {state:?} — worker liveness unknown; artifacts uncertain"
                        ),
                        actions: ACTIONS,
                    });
                }
            }
            AttemptState::Created | AttemptState::Admitted | AttemptState::Preparing => {
                // Pre-run limbo. Found 2026-08-21 by a killed process leaving
                // an attempt in `Preparing`: the console answered "nothing to
                // recover" about an attempt that could never move again. The
                // whole point of this queue is that no attempt is left in
                // limbo, so the pre-run states are surfaced too — and their
                // diagnosis is *better* than the running case, because no
                // hand ever started: no artifacts can exist, so resume-commit
                // has nothing to resume and retry is unambiguously safe.
                if age_ms > stale_after_ms {
                    out.push(AnomalyReport {
                        attempt_id: aid.as_str().to_string(),
                        anomaly: AnomalyKind::StaleRun,
                        state: format!("{state:?}"),
                        diagnosis: format!(
                            "no receipts for {age_ms}ms (tolerance {stale_after_ms}ms) in {state:?} — the worker died BEFORE the hand started; no artifacts can exist, so retry is safe and resume-commit has nothing to resume"
                        ),
                        actions: ACTIONS,
                    });
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

pub const ACTIONS: [&str; 3] = [
    "resume-commit (new fence generation; old generations may not commit)",
    "retry (same ContextPack, byte-identical; new attempt)",
    "close-as-unknown (explicit; unknown never defaults)",
];
