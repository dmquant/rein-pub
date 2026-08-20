//! The TUI's data layer: pure snapshots from the domain core (§10 — the TUI
//! and CLI consume the same core; the TUI never parses CLI output). Pure and
//! headless-testable; every judgment it shows names the receipt it derives
//! from (invariant 32), and absence is stated, never blank (invariant 31).

use rein_core::axes::AxisReport;
use rein_core::ids::AttemptId;
use rein_core::receipts::{ReceiptBody, ValidatorVerdict};
use rein_core::state::resolve_state;
use rein_runtime::clock::Clock;
use rein_runtime::recovery_queue::AnomalyReport;
use rein_runtime::store::Store;
use rein_runtime::workspace::Workspace;

pub struct TaskRow {
    pub task_ref: String,
    pub task_type: String,
    pub satisfied: bool,
}

#[allow(dead_code)] // fields feed panes as they grow; tests read them today
pub struct AttemptRow {
    pub attempt_id: String,
    pub task_ref: String,
    pub state: String,
    /// Outcome + the receipt it derives from — "every status names the
    /// receipt" (invariant 32).
    pub outcome: Option<(String, String)>,
}

pub struct CurrentTruth {
    pub epoch: String,
    pub pit_mode: String,
    pub source_cutoff: String,
    pub providers_lock: String,
}

#[allow(dead_code)]
pub struct UiSnapshot {
    pub workspace: String,
    pub missions: Vec<(String, String)>,
    pub tasks: Vec<TaskRow>,
    pub attempts: Vec<AttemptRow>,
    pub queue: Vec<AnomalyReport>,
    pub validator_failures: Vec<String>,
    pub truth: CurrentTruth,
}

pub fn load_snapshot(ws: &Workspace, store: &Store) -> Result<UiSnapshot, String> {
    let log = store.load_full_log().map_err(|e| e.to_string())?;
    let missions = store
        .list_missions()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(m, status)| (m.mission_ref.as_str().to_string(), status))
        .collect();
    let tasks = store
        .list_tasks()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| TaskRow {
            satisfied: rein_core::selection::task_satisfied(&log, &t.task_ref),
            task_ref: t.task_ref.as_str().to_string(),
            task_type: t.task_type,
        })
        .collect();
    let mut attempts = Vec::new();
    let mut validator_failures = Vec::new();
    for row in store.list_attempts().map_err(|e| e.to_string())? {
        let state = resolve_state(&log, &row.attempt_id)
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|_| "unresolvable".into());
        let mut outcome = None;
        for e in log.for_attempt(&row.attempt_id) {
            match &e.body {
                ReceiptBody::Terminal { outcome: o, .. } => {
                    outcome = Some((format!("{o:?}"), e.receipt_id.as_str().to_string()))
                }
                ReceiptBody::Validation {
                    artifact_name,
                    validator,
                    verdict,
                    ..
                } if !matches!(verdict, ValidatorVerdict::Passed) => {
                    validator_failures.push(format!(
                        "{}: {artifact_name} {validator}",
                        row.attempt_id.as_str()
                    ));
                }
                _ => {}
            }
        }
        attempts.push(AttemptRow {
            attempt_id: row.attempt_id.as_str().to_string(),
            task_ref: row.task_ref.as_str().to_string(),
            state,
            outcome,
        });
    }
    let queue = rein_runtime::recovery_queue::recovery_queue(
        store,
        rein_runtime::clock::SystemClock.now(),
        rein_runtime::recovery_queue::DEFAULT_STALE_AFTER_MS,
    )
    .map_err(|e| e.to_string())?;
    let (epoch, pit_mode, cutoff) = store
        .list_epochs()
        .map_err(|e| e.to_string())?
        .last()
        .map(|(e, sealed)| {
            (
                format!(
                    "{}{}",
                    e.epoch_ref.as_str(),
                    if *sealed { " [sealed]" } else { " [open]" }
                ),
                format!("{:?}", e.pit_mode),
                e.source_cutoff.canonical(),
            )
        })
        .unwrap_or_else(|| ("(no epoch)".into(), "—".into(), "—".into()));
    let providers_lock = std::fs::read(ws.providers_lock())
        .map(|b| rein_core::canon::Sha256Digest::of_bytes(&b).to_string())
        .unwrap_or_else(|_| "(no providers.lock)".into());

    Ok(UiSnapshot {
        workspace: ws.root.display().to_string(),
        missions,
        tasks,
        attempts,
        queue,
        validator_failures,
        truth: CurrentTruth {
            epoch,
            pit_mode,
            source_cutoff: cutoff,
            providers_lock,
        },
    })
}

pub struct AttemptDetail {
    pub attempt_id: String,
    pub task_ref: String,
    pub context_hash: String,
    pub axes: AxisReport,
    pub receipts: Vec<(String, String)>,
    pub validations: Vec<(String, String, String)>,
}

pub fn attempt_detail(store: &Store, aid: &AttemptId) -> Result<AttemptDetail, String> {
    let row = store.get_attempt(aid).map_err(|e| e.to_string())?;
    let log = store.load_attempt_log(aid).map_err(|e| e.to_string())?;
    let axes = AxisReport::derive(&log, aid, &row.task_ref);
    let mut receipts = Vec::new();
    let mut validations = Vec::new();
    for e in log.for_attempt(aid) {
        let kind = serde_json::to_value(&e.body)
            .ok()
            .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
            .unwrap_or_else(|| "?".into());
        receipts.push((e.receipt_id.as_str().to_string(), kind));
        if let ReceiptBody::Validation {
            artifact_name,
            validator,
            verdict,
            ..
        } = &e.body
        {
            let v = match verdict {
                ValidatorVerdict::Passed => "passed".to_string(),
                ValidatorVerdict::Failed { reason } => format!("failed: {reason}"),
                ValidatorVerdict::Quarantined { reason } => format!("quarantined: {reason}"),
            };
            validations.push((artifact_name.clone(), validator.to_string(), v));
        }
    }
    Ok(AttemptDetail {
        attempt_id: aid.as_str().to_string(),
        task_ref: row.task_ref.as_str().to_string(),
        context_hash: row.context_hash.to_string(),
        axes,
        receipts,
        validations,
    })
}

// ---- compare (§10 screen 4): six difference classes, complete --------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffClass {
    ExpectedEnvironmental,
    NonsemanticReceipt,
    SemanticInput,
    Output,
    Policy,
    Unexplained,
}

impl DiffClass {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ExpectedEnvironmental => "expected-environmental",
            Self::NonsemanticReceipt => "nonsemantic-receipt",
            Self::SemanticInput => "semantic-input",
            Self::Output => "output",
            Self::Policy => "policy",
            Self::Unexplained => "unexplained",
        }
    }

    #[allow(dead_code)] // the completeness contract; asserted by the M4 tests
    pub const ALL: [DiffClass; 6] = [
        Self::ExpectedEnvironmental,
        Self::NonsemanticReceipt,
        Self::SemanticInput,
        Self::Output,
        Self::Policy,
        Self::Unexplained,
    ];
}

pub struct CompareRow {
    pub subject: String,
    pub a: String,
    pub b: String,
    pub class: DiffClass,
}

pub struct CompareReport {
    pub a: String,
    pub b: String,
    pub rows: Vec<CompareRow>,
}

pub fn compare_attempts(
    store: &Store,
    a: &AttemptId,
    b: &AttemptId,
) -> Result<CompareReport, String> {
    let da = attempt_detail(store, a)?;
    let db = attempt_detail(store, b)?;
    let ra = store.get_attempt(a).map_err(|e| e.to_string())?;
    let rb = store.get_attempt(b).map_err(|e| e.to_string())?;
    let log_a = store.load_attempt_log(a).map_err(|e| e.to_string())?;
    let log_b = store.load_attempt_log(b).map_err(|e| e.to_string())?;

    let mut rows = Vec::new();

    // Semantic input: the ContextPack hash.
    rows.push(CompareRow {
        subject: "context_hash".into(),
        a: ra.context_hash.to_string(),
        b: rb.context_hash.to_string(),
        // Equal or not, the pack hash is the semantic-input axis.
        class: DiffClass::SemanticInput,
    });

    // Nonsemantic receipt identity: ids/timestamps always differ; classed so.
    rows.push(CompareRow {
        subject: "receipt ids".into(),
        a: format!("{} receipts", da.receipts.len()),
        b: format!("{} receipts", db.receipts.len()),
        class: DiffClass::NonsemanticReceipt,
    });

    // Environment receipts: expected-environmental.
    let env = |log: &rein_core::receipts::ReceiptLog, id: &AttemptId| {
        log.for_attempt(id)
            .filter(|e| matches!(e.body, ReceiptBody::Environment { .. }))
            .count()
    };
    rows.push(CompareRow {
        subject: "environment receipts".into(),
        a: format!("{}", env(&log_a, a)),
        b: format!("{}", env(&log_b, b)),
        class: DiffClass::ExpectedEnvironmental,
    });

    // Outputs: artifact digests per name.
    let digests = |log: &rein_core::receipts::ReceiptLog, id: &AttemptId| {
        let mut m = std::collections::BTreeMap::new();
        for e in log.for_attempt(id) {
            if let ReceiptBody::Commit { artifacts, .. } = &e.body {
                for art in artifacts {
                    if let Some(d) = &art.readback_digest {
                        m.insert(art.name.clone(), d.to_string());
                    }
                }
            }
        }
        m
    };
    let da_map = digests(&log_a, a);
    let db_map = digests(&log_b, b);
    let mut names: Vec<&String> = da_map.keys().chain(db_map.keys()).collect();
    names.sort();
    names.dedup();
    for name in names {
        rows.push(CompareRow {
            subject: format!("artifact {name}"),
            a: da_map
                .get(name)
                .cloned()
                .unwrap_or_else(|| "(absent)".into()),
            b: db_map
                .get(name)
                .cloned()
                .unwrap_or_else(|| "(absent)".into()),
            class: DiffClass::Output,
        });
    }

    // Policy: quarantine presence.
    let quarantines = |log: &rein_core::receipts::ReceiptLog, id: &AttemptId| {
        log.for_attempt(id)
            .filter(|e| matches!(e.body, ReceiptBody::Quarantine { .. }))
            .count()
    };
    rows.push(CompareRow {
        subject: "quarantines".into(),
        a: format!("{}", quarantines(&log_a, a)),
        b: format!("{}", quarantines(&log_b, b)),
        class: DiffClass::Policy,
    });

    // Outcome: if it differs while inputs and outputs agree, it is
    // unexplained — the class that demands investigation.
    let oa = da.axes.outcome.clone();
    let ob = db.axes.outcome.clone();
    let (sa, sb) = (format!("{oa}"), format!("{ob}"));
    let outputs_agree = da_map == db_map;
    rows.push(CompareRow {
        subject: "terminal outcome".into(),
        a: sa.clone(),
        b: sb.clone(),
        class: if sa == sb {
            DiffClass::NonsemanticReceipt
        } else if !outputs_agree {
            DiffClass::Output
        } else if quarantines(&log_a, a) != quarantines(&log_b, b) {
            DiffClass::Policy
        } else {
            DiffClass::Unexplained
        },
    });

    Ok(CompareReport {
        a: a.as_str().to_string(),
        b: b.as_str().to_string(),
        rows,
    })
}

// ---- action gating (invariant 32): every disabled action explains itself ---

pub enum ActionState {
    Enabled,
    Disabled { explain: String },
}

/// Can this attempt's evidence be published (bundle → coordination room)?
/// Disabled states name the receipt the judgment derives from.
pub fn publish_action_state(detail: &AttemptDetail) -> ActionState {
    match &detail.axes.outcome {
        rein_core::axes::Axis::Recorded(o)
            if o.outcome == rein_core::outcome::TerminalOutcome::Success =>
        {
            ActionState::Enabled
        }
        rein_core::axes::Axis::Recorded(o) => ActionState::Disabled {
            explain: format!(
                "publish disabled: terminal outcome is {:?} ({}) — a success terminal receipt is required",
                o.outcome, o.reason.0
            ),
        },
        rein_core::axes::Axis::NotYetRecorded => ActionState::Disabled {
            explain: "publish disabled: no terminal receipt yet — classification has not run"
                .to_string(),
        },
    }
}
