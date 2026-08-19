//! Hand-protocol semantics (§6) and the failure matrix at M0 depth: every
//! fixture through the pure pipeline ends in its prescribed outcome, reason
//! and exit code. (The fully wired matrix — real ledger, real CAS — is M3's
//! acceptance; the rows are pinned here first.)

mod common;

use common::*;
use rein_core::fakes::*;
use rein_core::hand::{
    per_step_breach, EventLedger, HandEvent, IngestError, IngestOutcome, SequencedEvent,
};
use rein_core::ids::IdGen;
use rein_core::outcome::{ReasonCode, TerminalOutcome};
use rein_core::state::AttemptState;
use rein_core::time::LogicalMs;

fn ev(run: &rein_core::ids::RunId, seq: u64, event: HandEvent) -> SequencedEvent {
    SequencedEvent {
        run_id: run.clone(),
        seq,
        at: LogicalMs(seq),
        event,
    }
}

#[test]
fn event_ledger_duplicates_idempotent_gaps_surfaced_conflicts_refused() {
    let mut ids = IdGen::new();
    let run = ids.run();
    let mut ledger = EventLedger::new(run.clone());

    assert_eq!(
        ledger
            .ingest(ev(&run, 0, HandEvent::StepStarted { step: 1 }))
            .unwrap(),
        IngestOutcome::Accepted
    );
    // Identical duplicate: idempotent.
    assert_eq!(
        ledger
            .ingest(ev(&run, 0, HandEvent::StepStarted { step: 1 }))
            .unwrap(),
        IngestOutcome::DuplicateIgnored
    );
    // Same seq, different payload: surfaced as a conflict, never absorbed.
    assert_eq!(
        ledger.ingest(ev(&run, 0, HandEvent::StepStarted { step: 2 })),
        Err(IngestError::ConflictingDuplicate { seq: 0 })
    );
    // A gap is surfaced with exactly the missing sequence numbers.
    assert_eq!(
        ledger
            .ingest(ev(&run, 3, HandEvent::StepCompleted { step: 1 }))
            .unwrap(),
        IngestOutcome::AcceptedWithGap {
            missing: vec![1, 2]
        }
    );
    assert_eq!(ledger.gaps(), vec![1, 2]);
    // Filling a gap.
    assert_eq!(
        ledger
            .ingest(ev(&run, 1, HandEvent::StepCompleted { step: 9 }))
            .unwrap(),
        IngestOutcome::Accepted
    );
    assert_eq!(ledger.gaps(), vec![2]);
}

/// The §6 failure matrix, M0 form. Exit codes per §9's total mapping — with
/// objection O1's accepted resolution pinned: the bare `secret-leak` run maps
/// `failure → 10` (13 is reserved to `--require validation-passed`, M1+).
#[test]
fn failure_matrix_every_fixture_ends_in_its_prescribed_row() {
    struct Row {
        fixture: Box<dyn FakeHand>,
        outcome: TerminalOutcome,
        reason: ReasonCode,
        exit: i32,
    }
    let rows = vec![
        Row {
            fixture: Box::new(DeterministicA),
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            exit: 0,
        },
        Row {
            fixture: Box::new(DeterministicB),
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            exit: 0,
        },
        Row {
            fixture: Box::new(Exit0Empty),
            outcome: TerminalOutcome::ArtifactInvalid,
            reason: ReasonCode::required_artifact_absent(),
            exit: 12,
        },
        Row {
            fixture: Box::new(HashMismatch),
            outcome: TerminalOutcome::ArtifactInvalid,
            reason: ReasonCode::readback_digest_mismatch(),
            exit: 12,
        },
        Row {
            fixture: Box::new(DuplicateCallback),
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            exit: 0,
        },
        Row {
            fixture: Box::new(TimeoutFake),
            outcome: TerminalOutcome::TimedOut,
            reason: ReasonCode::per_step_budget_exceeded(),
            exit: 14,
        },
        Row {
            fixture: Box::new(SecretLeak),
            outcome: TerminalOutcome::Failure,
            reason: ReasonCode::artifact_quarantined_secret(),
            exit: 10, // O1: outcome mapping wins on a bare run
        },
        Row {
            fixture: Box::new(PartialOutput),
            outcome: TerminalOutcome::PartialSuccess,
            reason: ReasonCode::some_required_valid(),
            exit: 10,
        },
        Row {
            fixture: Box::new(UnknownAfterDisconnect),
            outcome: TerminalOutcome::Unknown,
            reason: ReasonCode::run_lost_no_evidence(),
            exit: 11,
        },
        Row {
            fixture: Box::new(CjkSplitter),
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            exit: 0,
        },
    ];
    assert_eq!(rows.len(), 10, "nine PDF fixtures + the Rein addition");

    for row in rows {
        let name = row.fixture.name();
        let r = run_fixture_pipeline(row.fixture.as_ref());
        let (outcome, reason) = r
            .outcome
            .clone()
            .unwrap_or_else(|| panic!("{name}: no terminal receipt"));
        assert_eq!(outcome, row.outcome, "{name}: outcome");
        assert_eq!(reason, row.reason, "{name}: reason code");
        assert_eq!(outcome.exit_code().code(), row.exit, "{name}: exit code");
    }
}

#[test]
fn duplicate_callback_fixture_is_ingested_idempotently() {
    let r = run_fixture_pipeline(&DuplicateCallback);
    assert_eq!(
        r.duplicate_events, 1,
        "the duplicated RunCompleted is absorbed once"
    );
    let (outcome, _) = r.outcome.unwrap();
    assert_eq!(outcome, TerminalOutcome::Success);
}

#[test]
fn unknown_after_disconnect_enters_recovery_and_closes_without_inference() {
    let r = run_fixture_pipeline(&UnknownAfterDisconnect);
    assert_eq!(r.final_state, AttemptState::Terminal);
    // The path went through recovery_pending — visible in the transition record.
    let entered_recovery = r.log.for_attempt(&r.attempt_id).any(|e| {
        matches!(
            &e.body,
            rein_core::receipts::ReceiptBody::Transition {
                to: AttemptState::RecoveryPending,
                ..
            }
        )
    });
    assert!(entered_recovery, "recovery_pending must be entered");
}

#[test]
fn cjk_splitter_output_is_captured_byte_identical() {
    let r = run_fixture_pipeline(&CjkSplitter);
    assert_eq!(r.decoded_stdout, CJK_TEXT);
    let (outcome, _) = r.outcome.unwrap();
    assert_eq!(outcome, TerminalOutcome::Success);
}

/// M1's acceptance, previewed at M0's pure level: the same frozen ContextPack
/// through deterministic-a and deterministic-b yields identical required-
/// artifact digests, and the bytes are invariant across attempt generations
/// (a retry reproduces them).
#[test]
fn deterministic_a_and_b_yield_identical_artifact_digests_from_one_context_pack() {
    // Generation invariance of the deterministic content function itself.
    let pack = sealed_sample_pack();
    let hash = pack.context_hash.clone().unwrap();
    let mut ids = IdGen::new();
    let base = rein_core::hand::HandRequest {
        attempt_id: rein_core::ids::AttemptId::parse("attempt_000001").unwrap(),
        run_id: ids.run(),
        fence_generation: 1,
        sequence: 0,
        idempotency_key: rein_core::idempotency::IdempotencyKey::derive(&pack.task_ref, &hash, 1),
        capability_ref: rein_core::ids::GrantId::parse("grant_fixture").unwrap(),
        trace: ids.trace(),
        deadline: LogicalMs(1),
        internal_retries_disabled: true,
    };
    let mut retried = base.clone();
    retried.idempotency_key =
        rein_core::idempotency::IdempotencyKey::derive(&pack.task_ref, &hash, 2);
    assert_eq!(
        deterministic_artifact_bytes(&base, "valuation.json"),
        deterministic_artifact_bytes(&retried, "valuation.json"),
        "artifact bytes must not depend on the attempt generation"
    );
    let ra = run_fixture_pipeline(&DeterministicA);
    let rb = run_fixture_pipeline(&DeterministicB);

    let digests = |r: &PipelineResult| -> Vec<(String, String)> {
        let mut out = Vec::new();
        for e in r.log.for_attempt(&r.attempt_id) {
            if let rein_core::receipts::ReceiptBody::Commit { artifacts, .. } = &e.body {
                for a in artifacts {
                    out.push((
                        a.name.clone(),
                        a.readback_digest.clone().expect("verified").to_string(),
                    ));
                }
            }
        }
        out.sort();
        out
    };
    let da = digests(&ra);
    let db = digests(&rb);
    assert_eq!(da, db, "same pack, different hands: identical digests");
    assert!(!da.is_empty());

    // And the process shapes genuinely differed (b chatters on stdout, a is
    // silent) — the equality above is not an artifact of identical streams.
    assert_ne!(ra.decoded_stdout, rb.decoded_stdout);
}

#[test]
fn per_step_breach_names_the_guilty_step_not_the_next_stage() {
    let pack = sealed_sample_pack();
    let mut ids = IdGen::new();
    let run = ids.run();
    let over = pack.budget.per_step_timeout_ms + 1;
    let events = vec![
        ev(&run, 0, HandEvent::StepStarted { step: 7 }),
        SequencedEvent {
            run_id: run.clone(),
            seq: 1,
            at: LogicalMs(over),
            event: HandEvent::StepCompleted { step: 7 },
        },
    ];
    let breach = per_step_breach(&events, &pack.budget).expect("breach detected");
    assert_eq!(
        breach.step, 7,
        "the budget buys attribution: the guilty step is named"
    );
    assert_eq!(breach.limit_ms, pack.budget.per_step_timeout_ms);
}

#[test]
fn all_ten_fixtures_are_registered() {
    let names: Vec<&str> = all_fixtures().iter().map(|f| f.name()).collect();
    assert_eq!(
        names,
        vec![
            "fake:deterministic-a",
            "fake:deterministic-b",
            "fake:exit0-empty",
            "fake:hash-mismatch",
            "fake:duplicate-callback",
            "fake:timeout",
            "fake:secret-leak",
            "fake:partial-output",
            "fake:unknown-after-disconnect",
            "fake:cjk-splitter",
        ]
    );
}
