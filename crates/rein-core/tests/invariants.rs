//! The M0 invariant mutation-test manifest (design §13 M0): one reddening
//! test per invariant owed at M0 — 1–9, 22–24, 28 (schema-side), 30.
//!
//! Mechanism: each test *uses* the production symbol named in its test name,
//! so deleting the symbol reddens compilation and mutating the behavior
//! reddens the assertion. The map invariant → symbol → test also lives in
//! docs/INVARIANTS.md.
//!
//! Test names carry `invNN__<symbol>__<claim>` — the double underscores are
//! load-bearing separators, hence the lint allowance.
#![allow(non_snake_case)]

mod common;

use common::*;
use rein_core::axes::{Axis, AxisReport, ExternalAxis};
use rein_core::canon::{parse_canon_json, CanonError, Sha256Digest};
use rein_core::capture::Utf8StreamDecoder;
use rein_core::classify::{classify, ClassifyError};
use rein_core::context_pack::{SEMANTIC_EXCLUDED, TOP_LEVEL_KEYS};
use rein_core::entities::Attempt;
use rein_core::fakes::{DeterministicA, Exit0Empty, SecretLeak, CJK_TEXT};
use rein_core::fence;
use rein_core::hand::ModelIdentity;
use rein_core::idempotency::{
    admit, AdmissionOutcome, AdmitError, AttemptRequest, IdempotencyKey, RequestKind,
};
use rein_core::ids::{AttemptId, IdGen, SecretRefId, ValidatorRef};
use rein_core::outcome::{ReasonCode, TerminalOutcome};
use rein_core::pins::ProviderPin;
use rein_core::receipts::*;
use rein_core::recovery::{self, RecoveryAction};
use rein_core::selection::{
    assemble_bundle_manifest, resolve_attempt_ref, select_and_record, task_satisfied, JoinKeyError,
};
use rein_core::state::{
    apply_transition, resolve_state, AttemptState, TransitionCauseRecord, TransitionError,
};

/// Invariant 1 — six claim vocabularies, never one badge; external axes render
/// recorded state or "not adjudicated here", never a blank.
/// Symbol: `axes::AxisReport` (with `axes::ExternalAxis`).
#[test]
fn inv01__axes_axisreport__six_vocabularies_never_collapse_and_external_axes_never_blank() {
    // The canonical disagreement (§6 exit0-empty): process says clean exit,
    // artifact says absent, outcome says artifact_invalid.
    let r = run_fixture_pipeline(&Exit0Empty);
    let report = AxisReport::derive(&r.log, &r.attempt_id, &r.pack.task_ref);

    // Six separate fields, read independently — the compiler holds the shape.
    let AxisReport {
        process,
        artifact,
        outcome,
        satisfaction,
        research_acceptance,
        system_admission,
    } = report;

    match &process {
        Axis::Recorded(p) => assert_eq!(p.last_child_exit, Some(0), "process axis: exit 0"),
        Axis::NotYetRecorded => panic!("process was captured"),
    }
    match &artifact {
        Axis::Recorded(a) => {
            assert_eq!(a.missing, 2, "artifact axis: both required absent");
            assert_eq!(a.verified, 0);
        }
        Axis::NotYetRecorded => panic!("commit receipt exists"),
    }
    match &outcome {
        Axis::Recorded(o) => assert_eq!(o.outcome, TerminalOutcome::ArtifactInvalid),
        Axis::NotYetRecorded => panic!("terminal receipt exists"),
    }
    // The disagreeing axes coexist — nothing collapsed exit 0 into success.

    // Stated absence, never blank (invariant 31's schema-side seed).
    assert!(!format!("{satisfaction}").is_empty());
    assert_eq!(
        format!("{research_acceptance}"),
        "external: not adjudicated here"
    );
    assert_eq!(
        format!("{system_admission}"),
        "external: not adjudicated here"
    );
    assert!(matches!(
        research_acceptance,
        ExternalAxis::NotAdjudicatedHere
    ));
}

/// Invariant 2 — process exit and model self-report are evidence, not terminal
/// classification. Symbol: `classify::classify`.
#[test]
fn inv02__classify_classify__exit_zero_and_self_report_cannot_reach_success() {
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();
    let AdmissionOutcome::New { attempt, .. } = admit(
        &mut log,
        &mut ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack.clone(),
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };
    let aid = attempt.attempt_id;

    // A confident capture: exit 0, cheerful stdout. Nothing else.
    let run_id = ids.run();
    log.append(
        &mut ids,
        &aid,
        at,
        ReceiptBody::Capture {
            run_id,
            capture: rein_core::capture::CaptureArtifact {
                exit_code: Some(0),
                stdout: "all done, success!".into(),
                stderr: String::new(),
                side_channels: vec![],
                captured_via: "pipe".into(),
                tool_versions: Default::default(),
            },
        },
    );

    // classify sees receipts only — and refuses: no commit evidence. It does
    // not return success, and it does not default to unknown.
    let got = classify(&log, &aid, &pack.output_contract);
    assert!(
        matches!(got, Err(ClassifyError::InsufficientEvidence { .. })),
        "exit 0 alone must classify nothing: {got:?}"
    );

    // With a commit receipt recording absence, exit 0 still cannot be success.
    let records = vec![
        evaluate_artifact_commit("valuation.json", None, None, None),
        evaluate_artifact_commit("memo.md", None, None, None),
    ];
    log.append(
        &mut ids,
        &aid,
        at,
        ReceiptBody::Commit {
            fence_generation: 1,
            artifacts: records,
        },
    );
    let c = classify(&log, &aid, &pack.output_contract).unwrap();
    assert_eq!(c.outcome, TerminalOutcome::ArtifactInvalid);
    assert_eq!(c.reason, ReasonCode::required_artifact_absent());
}

/// Invariant 3 — success requires committed+read-back artifacts AND validators
/// AND no unresolved policy failure AND a classifier receipt.
/// Symbol: `classify::classify` (with `receipts::evaluate_artifact_commit`).
#[test]
fn inv03__classify_classify__success_requires_readback_validators_and_policy_resolution() {
    // The full pipeline through a well-behaved hand: success, with receipts.
    let r = run_fixture_pipeline(&DeterministicA);
    let (outcome, reason) = r.outcome.clone().unwrap();
    assert_eq!(outcome, TerminalOutcome::Success);
    assert_eq!(reason, ReasonCode::required_outputs_valid());

    // The terminal receipt exists and cites supporting receipts.
    let supporting_nonempty = r.log.for_attempt(&r.attempt_id).any(|e| {
        matches!(&e.body, ReceiptBody::Terminal { outcome: TerminalOutcome::Success, supporting, .. } if !supporting.is_empty())
    });
    assert!(supporting_nonempty, "success must cite its evidence");

    // Mutation arms: withhold each leg and success must vanish.
    let at = t("2026-08-19T09:00:00Z");
    let pack = sealed_sample_pack();
    let build = |mutator: &dyn Fn(&mut ReceiptLog, &mut IdGen, &AttemptId)| {
        let mut ids = IdGen::new();
        let mut log = ReceiptLog::new();
        let AdmissionOutcome::New { attempt, .. } = admit(
            &mut log,
            &mut ids,
            &AttemptRequest {
                task_ref: pack.task_ref.clone(),
                context_pack: pack.clone(),
                kind: RequestKind::Fresh,
            },
            at,
        )
        .unwrap() else {
            panic!()
        };
        let aid = attempt.attempt_id;
        mutator(&mut log, &mut ids, &aid);
        classify(&log, &aid, &pack.output_contract).map(|c| c.outcome)
    };
    let all_validators =
        |log: &mut ReceiptLog, ids: &mut IdGen, aid: &AttemptId, skip: &[usize]| {
            let contract = sample_contract();
            let mut n = 0;
            for a in &contract.required_artifacts {
                for v in &contract.validators {
                    n += 1;
                    if skip.contains(&n) {
                        continue;
                    }
                    log.append(
                        ids,
                        aid,
                        at,
                        ReceiptBody::Validation {
                            artifact_name: a.name.clone(),
                            validator: v.clone(),
                            over_digest: Some(Sha256Digest::of_bytes(b"x")),
                            verdict: ValidatorVerdict::Passed,
                        },
                    );
                }
            }
        };
    let full_commit = |log: &mut ReceiptLog, ids: &mut IdGen, aid: &AttemptId| {
        let bytes = b"content".to_vec();
        let records = vec![
            evaluate_artifact_commit(
                "valuation.json",
                None,
                Some(bytes.as_slice()),
                Some(bytes.as_slice()),
            ),
            evaluate_artifact_commit(
                "memo.md",
                None,
                Some(bytes.as_slice()),
                Some(bytes.as_slice()),
            ),
        ];
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Commit {
                fence_generation: 1,
                artifacts: records,
            },
        );
    };

    // (a) all legs present → success
    let ok = build(&|log, ids, aid| {
        full_commit(log, ids, aid);
        all_validators(log, ids, aid, &[]);
    });
    assert_eq!(ok.unwrap(), TerminalOutcome::Success);

    // (b) a mandatory validator missing on one artifact → the whole cannot be
    // success (the other artifact still counts: partial).
    let one_missing = build(&|log, ids, aid| {
        full_commit(log, ids, aid);
        all_validators(log, ids, aid, &[1]);
    });
    assert_eq!(one_missing.unwrap(), TerminalOutcome::PartialSuccess);
    // …and missing on every artifact → artifact_invalid.
    let all_missing = build(&|log, ids, aid| {
        full_commit(log, ids, aid);
        all_validators(log, ids, aid, &[1, 3]);
    });
    assert_eq!(all_missing.unwrap(), TerminalOutcome::ArtifactInvalid);

    // (c) read-back mismatch → not success ("evidence retained": the records stay)
    let mismatch = build(&|log, ids, aid| {
        let good = b"written".to_vec();
        let wrong_claim = Sha256Digest::of_bytes(b"claimed something else");
        let records = vec![
            evaluate_artifact_commit(
                "valuation.json",
                Some(&wrong_claim),
                Some(good.as_slice()),
                Some(good.as_slice()),
            ),
            evaluate_artifact_commit(
                "memo.md",
                Some(&wrong_claim),
                Some(good.as_slice()),
                Some(good.as_slice()),
            ),
        ];
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Commit {
                fence_generation: 1,
                artifacts: records,
            },
        );
        all_validators(log, ids, aid, &[]);
    });
    assert_eq!(mismatch.unwrap(), TerminalOutcome::ArtifactInvalid);

    // (d) unresolved policy failure (quarantine) → failure, not success
    let quarantined = build(&|log, ids, aid| {
        full_commit(log, ids, aid);
        all_validators(log, ids, aid, &[]);
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Quarantine {
                artifact_name: "valuation.json".into(),
                validator: ValidatorRef::parse("secret-scan@1").unwrap(),
                withheld_from_selection: true,
            },
        );
    });
    assert_eq!(quarantined.unwrap(), TerminalOutcome::Failure);
}

/// Invariant 4 — a successful Attempt does not satisfy the Task; a
/// TaskSelectionReceipt does. Symbol: `selection::task_satisfied`.
#[test]
fn inv04__selection_task_satisfied__successful_attempt_alone_never_satisfies() {
    let mut r = run_fixture_pipeline(&DeterministicA);
    let (outcome, _) = r.outcome.clone().unwrap();
    assert_eq!(outcome, TerminalOutcome::Success);

    // Success is in the ledger — and the task is still not satisfied.
    assert!(
        !task_satisfied(&r.log, &r.pack.task_ref),
        "success without a selection receipt must not satisfy (invariant 4)"
    );

    let at = t("2026-08-19T10:00:00Z");
    select_and_record(
        &mut r.log,
        &mut r.ids,
        &r.pack.task_ref.clone(),
        &[r.attempt_id.clone()],
        at,
    );
    assert!(task_satisfied(&r.log, &r.pack.task_ref));
}

/// Invariant 5 — unknown stays unknown; administrative force-success does not
/// exist. Symbol: `recovery::RecoveryAction` (with `state::apply_transition`'s
/// recovery-close gate).
#[test]
fn inv05__recovery_recoveryaction__exactly_three_actions_no_force_success() {
    // The whole action vocabulary: three. A fourth variant reddens this line.
    let [a, b, c] = RecoveryAction::ALL;
    assert_eq!(
        [a, b, c],
        [
            RecoveryAction::ResumeCommitNewGeneration,
            RecoveryAction::RetrySameContextPack,
            RecoveryAction::CloseAsUnknown,
        ]
    );
    for action in RecoveryAction::ALL {
        let name = serde_json::to_string(&action).unwrap();
        assert!(
            !name.contains("force") && !name.contains("success"),
            "no recovery action may spell force-success: {name}"
        );
    }

    // From recovery_pending, closing with a *success* terminal receipt is
    // structurally rejected: the console cannot manufacture success.
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();
    let AdmissionOutcome::New { attempt, .. } = admit(
        &mut log,
        &mut ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack.clone(),
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };
    let aid = attempt.attempt_id;
    for to in [
        AttemptState::Admitted,
        AttemptState::Preparing,
        AttemptState::Running,
    ] {
        advance(&mut log, &mut ids, &aid, to, at);
    }
    recovery::enter_recovery(
        &mut log,
        &mut ids,
        &aid,
        rein_core::state::AnomalyKind::StaleRun,
        at,
    )
    .unwrap();

    let forged = log.append(
        &mut ids,
        &aid,
        at,
        ReceiptBody::Terminal {
            outcome: TerminalOutcome::Success,
            reason: ReasonCode::required_outputs_valid(),
            supporting: vec![],
        },
    );
    let refused = apply_transition(
        &mut log,
        &mut ids,
        &aid,
        AttemptState::Terminal,
        TransitionCauseRecord::ClassificationComplete {
            terminal_receipt: forged,
        },
        at,
    );
    assert!(
        matches!(
            refused,
            Err(TransitionError::RecoveryCloseNotUnknown {
                got: TerminalOutcome::Success
            })
        ),
        "recovery close must demand outcome unknown (or a separately authorized exception): {refused:?}"
    );

    // The sanctioned path: close as unknown.
    recovery::close_attempt_as_unknown(
        &mut log,
        &mut ids,
        &aid,
        ReasonCode::closed_as_unknown_by_operator(),
        vec![],
        at,
    )
    .unwrap();
    assert_eq!(resolve_state(&log, &aid).unwrap(), AttemptState::Terminal);

    // The exception receipt schema demands its authorization.
    let json = serde_json::to_value(ReceiptBody::Exception {
        authorization_ref: "operator-grant:2026-08-19".into(),
        scope: "test".into(),
        note: String::new(),
    })
    .unwrap();
    assert_eq!(json["kind"], "exception");
    assert_eq!(json["authorization_ref"], "operator-grant:2026-08-19");
}

/// Invariant 6 — one immutable ContextPack per attempt; retry is byte-identical;
/// a semantic change is rejected toward a new TaskVersion.
/// Symbols: `recovery::retry_same_context_pack`, `idempotency::admit`.
#[test]
fn inv06__attempt_retry__reuses_context_hash_and_semantic_change_is_rejected() {
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();
    let AdmissionOutcome::New { attempt, .. } = admit(
        &mut log,
        &mut ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack.clone(),
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };

    // Retry: new attempt, same hash, next generation.
    let AdmissionOutcome::New { attempt: again, .. } =
        recovery::retry_same_context_pack(&mut log, &mut ids, &attempt.attempt_id, &pack, at)
            .unwrap()
    else {
        panic!("retry mints a new attempt")
    };
    assert_ne!(again.attempt_id, attempt.attempt_id);
    assert_eq!(
        again.context_hash, attempt.context_hash,
        "byte-identical pack"
    );
    assert_eq!(again.generation, attempt.generation + 1);

    // Semantic change: rejected, redirected — the harness refuses to call it a retry.
    let mut changed = pack.clone();
    changed.universe.push("security:amd".into());
    changed.seal().unwrap();
    let refused =
        recovery::retry_same_context_pack(&mut log, &mut ids, &attempt.attempt_id, &changed, at);
    match refused {
        Err(rein_core::recovery::RecoveryError::Admit(AdmitError::SemanticChangeRejected {
            prior,
            offered,
        })) => {
            assert_eq!(prior, attempt.context_hash);
            assert_ne!(offered, attempt.context_hash);
        }
        other => panic!("semantic change must be rejected: {other:?}"),
    }
    let msg = format!(
        "{}",
        AdmitError::SemanticChangeRejected {
            prior: attempt.context_hash.clone(),
            offered: again.context_hash.clone(),
        }
    );
    assert!(
        msg.contains("TaskVersion"),
        "rejection directs to a new TaskVersion: {msg}"
    );
}

/// Invariant 7 — canonical encoding: key order, duplicate rejection, explicit
/// null vs absent, and the semantic exclusion set (decisions C1/C2).
/// Symbols: `canon::parse_canon_json` / `context_pack::ContextPack::semantic_hash`.
#[test]
fn inv07__canon_canonicalize__vectors_key_order_dup_rejection_exclusion_set() {
    // Key order never matters; bytes are sorted-by-codepoint.
    let a = parse_canon_json(r#"{"b":1,"a":"x"}"#).unwrap();
    let b = parse_canon_json(r#"{"a":"x","b":1}"#).unwrap();
    assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    assert_eq!(a.canonical_bytes().unwrap(), br#"{"a":"x","b":1}"#.to_vec());

    // Duplicate keys are rejected, not last-wins.
    let dup = parse_canon_json(r#"{"a":1,"a":2}"#);
    assert!(
        matches!(&dup, Err(CanonError::Parse(m)) if m.contains("duplicate object key")),
        "{dup:?}"
    );

    // Explicit null is not absence.
    let with_null = parse_canon_json(r#"{"a":null}"#).unwrap();
    let absent = parse_canon_json(r#"{}"#).unwrap();
    assert_ne!(
        with_null.canonical_bytes().unwrap(),
        absent.canonical_bytes().unwrap()
    );

    // Exclusion set: bookkeeping fields do not move the hash; semantics do.
    let sealed = sealed_sample_pack();
    let base = sealed.semantic_hash().unwrap();

    let mut bookkeeping = sealed.clone();
    bookkeeping.created_at = t("2031-01-01T00:00:00Z");
    bookkeeping.context_pack_id = rein_core::ids::ContextPackId::parse("ctx_999999").unwrap();
    bookkeeping.context_hash = None;
    assert_eq!(bookkeeping.semantic_hash().unwrap(), base);

    let mut semantic = sealed.clone();
    semantic.source_cutoff = t("2020-01-01T00:00:00Z");
    assert_ne!(semantic.semantic_hash().unwrap(), base);

    // C2 amendment (M1): the hand binding is execution binding, not semantic
    // content — rebinding the executor must not change the pack hash, or
    // M1's acceptance (same pack through fake-a and fake-b) is unsatisfiable
    // and recovery's "retry same ContextPack" dies with the hand.
    let mut rebound = sealed.clone();
    rebound.hand.selector = "fake:deterministic-b".into();
    assert_eq!(rebound.semantic_hash().unwrap(), base);

    assert_eq!(
        SEMANTIC_EXCLUDED,
        &["context_pack_id", "context_hash", "created_at", "hand"],
        "the exclusion set is a recorded decision (C2, amended at M1); changing it is a design change"
    );

    // No ambient environment fields: the serialized key set is the whitelist.
    let json = serde_json::to_value(&sealed).unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(|s| s.as_str())
        .collect();
    keys.sort_unstable();
    let mut expected: Vec<&str> = TOP_LEVEL_KEYS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        keys, expected,
        "context pack grew or lost a top-level field"
    );
}

/// Invariant 8 — pins are exact where bytes exist, declared where not; model
/// identity is two fields, requested and served.
/// Symbols: `pins::ProviderPin`, `hand::ModelIdentity`.
#[test]
fn inv08__pins_providerpin__digest_or_declared_method_and_two_model_id_fields() {
    let digest: ProviderPin = serde_json::from_str(
        r#"{"coordinate":"hand:agy@1.1.11","digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
    )
    .unwrap();
    assert!(digest.is_exact());

    let service: ProviderPin = serde_json::from_str(
        r#"{"coordinate":"fmp-api@v4","pin_method":"served version header, recorded per call"}"#,
    )
    .unwrap();
    assert!(!service.is_exact());

    // A bare coordinate is not a pin.
    assert!(serde_json::from_str::<ProviderPin>(r#"{"coordinate":"fmp-api@v4"}"#).is_err());
    // A malformed digest is not a pin either.
    assert!(
        serde_json::from_str::<ProviderPin>(r#"{"coordinate":"x","digest":"sha256:short"}"#)
            .is_err()
    );

    // Two model-id fields, both mandatory — a fallback string is not diffable.
    let id: ModelIdentity = serde_json::from_str(
        r#"{"requested":"gemini-3.6-flash-high","served":"gemini-3.6-flash"}"#,
    )
    .unwrap();
    assert_eq!(
        (id.requested.as_str(), id.served.as_str()),
        ("gemini-3.6-flash-high", "gemini-3.6-flash")
    );
    assert!(
        serde_json::from_str::<ModelIdentity>(r#"{"requested":"gemini-3.6-flash-high"}"#).is_err(),
        "served is mandatory"
    );
}

/// Invariant 9 — every proposed fact carries a resolvable join key to an
/// attempt record that exists. Symbol: `selection::resolve_attempt_ref`
/// (with `selection::assemble_bundle_manifest`).
#[test]
fn inv09__selection_resolve_attempt_ref__dangling_join_key_refuses_bundle() {
    let r = run_fixture_pipeline(&DeterministicA);
    resolve_attempt_ref(&r.log, &r.attempt_id).unwrap();

    let bundle = assemble_bundle_manifest(&r.log, &r.attempt_id, Default::default()).unwrap();
    assert_eq!(bundle.schema, "rein.evidence-bundle/v1");
    assert_eq!(bundle.attempt_ref, r.attempt_id);
    assert!(!bundle.receipts.is_empty());
    assert_eq!(bundle.context_hash, r.pack.context_hash.clone().unwrap());

    let dangling = AttemptId::parse("attempt_424242").unwrap();
    let refused = assemble_bundle_manifest(&r.log, &dangling, Default::default());
    assert!(
        matches!(
            refused,
            Err(rein_core::selection::BundleError::JoinKey(
                JoinKeyError::Dangling(_)
            ))
        ),
        "a dangling join key must refuse the bundle: {refused:?}"
    );
}

/// Invariant 22 — every state transition appends a receipt; state resolves
/// from the ledger, never from memory.
/// Symbols: `state::apply_transition`, `state::resolve_state`.
#[test]
fn inv22__state_transition_apply__every_transition_appends_receipt_and_state_resolves_from_ledger()
{
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();
    let AdmissionOutcome::New { attempt, .. } = admit(
        &mut log,
        &mut ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack.clone(),
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };
    let aid = attempt.attempt_id.clone();

    // The Attempt entity carries no state field — destructured exhaustively,
    // this reddens if one is ever added (state lives in the ledger only).
    let Attempt {
        attempt_id: _,
        task_ref: _,
        context_pack_id: _,
        context_hash: _,
        generation: _,
        created_at: _,
    } = attempt;

    let walk = [
        AttemptState::Admitted,
        AttemptState::Preparing,
        AttemptState::Running,
        AttemptState::CommitPending,
        AttemptState::Validating,
        AttemptState::Classifying,
    ];
    for to in walk {
        let before = log.len();
        apply_transition(
            &mut log,
            &mut ids,
            &aid,
            to,
            TransitionCauseRecord::Advance,
            at,
        )
        .unwrap();
        assert_eq!(log.len(), before + 1, "one transition, one receipt");
        assert_eq!(resolve_state(&log, &aid).unwrap(), to);
    }

    // An illegal edge appends nothing.
    let before = log.len();
    let illegal = apply_transition(
        &mut log,
        &mut ids,
        &aid,
        AttemptState::Running,
        TransitionCauseRecord::Advance,
        at,
    );
    assert!(matches!(illegal, Err(TransitionError::IllegalEdge { .. })));
    assert_eq!(log.len(), before, "refused transitions leave no receipt");

    // Terminal requires classifier evidence in the ledger — a made-up receipt
    // id cannot cross.
    let missing = apply_transition(
        &mut log,
        &mut ids,
        &aid,
        AttemptState::Terminal,
        TransitionCauseRecord::ClassificationComplete {
            terminal_receipt: rein_core::ids::ReceiptId::parse("rcpt_999999").unwrap(),
        },
        at,
    );
    assert!(matches!(
        missing,
        Err(TransitionError::MissingTerminalReceipt(_))
    ));
}

/// Invariant 23 — idempotency is scoped to the request; duplicate delivery
/// returns the original receipt; retry mints a new generation.
/// Symbol: `idempotency::IdempotencyKey` (with `idempotency::admit`).
#[test]
fn inv23__idempotency_key__duplicate_request_returns_original_receipt_retry_mints_generation() {
    let pack = sealed_sample_pack();
    let hash = pack.context_hash.clone().unwrap();
    let key = IdempotencyKey::derive(&pack.task_ref, &hash, 1);
    assert_eq!(
        key.as_str(),
        format!("task:dcf-nvda@2/context:{hash}/gen:1"),
        "the key is task/context-hash/attempt-generation"
    );

    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let request = AttemptRequest {
        task_ref: pack.task_ref.clone(),
        context_pack: pack.clone(),
        kind: RequestKind::Fresh,
    };
    let AdmissionOutcome::New {
        attempt,
        created_receipt,
    } = admit(&mut log, &mut ids, &request, at).unwrap()
    else {
        panic!()
    };

    // Duplicate delivery of the same request: the original receipt, no new
    // attempt, no new transition (§6 matrix, duplicate row).
    let before = log.len();
    let dup = admit(&mut log, &mut ids, &request, at).unwrap();
    assert_eq!(
        dup,
        AdmissionOutcome::Duplicate {
            original: created_receipt,
            attempt_id: attempt.attempt_id.clone(),
        }
    );
    assert_eq!(log.len(), before, "duplicate delivery appends nothing");

    // Retry is *not* answered by the old receipt: same pack, new generation.
    let retry = AttemptRequest {
        task_ref: pack.task_ref.clone(),
        context_pack: pack.clone(),
        kind: RequestKind::Retry {
            of: attempt.attempt_id.clone(),
        },
    };
    let AdmissionOutcome::New {
        attempt: second, ..
    } = admit(&mut log, &mut ids, &retry, at).unwrap()
    else {
        panic!("retry must mint a new attempt under the same ContextPack")
    };
    assert_eq!(second.generation, 2);
    assert_eq!(second.context_hash, attempt.context_hash);
}

/// Invariant 24 — recovery never changes a ContextPack; a new generation gets
/// a fence receipt and old generations may not commit.
/// Symbols: `fence::guard_commit`, `fence::issue_next_generation`.
#[test]
fn inv24__fence_generation__stale_generation_cannot_commit_and_recovery_never_edits_pack() {
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();
    let hash_before = pack.context_hash.clone().unwrap();
    let AdmissionOutcome::New { attempt, .. } = admit(
        &mut log,
        &mut ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack.clone(),
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };
    let aid = attempt.attempt_id.clone();

    assert_eq!(fence::current_generation(&log, &aid).unwrap(), 1);
    fence::guard_commit(&log, &aid, 1).unwrap();

    for to in [
        AttemptState::Admitted,
        AttemptState::Preparing,
        AttemptState::Running,
    ] {
        advance(&mut log, &mut ids, &aid, to, at);
    }
    recovery::enter_recovery(
        &mut log,
        &mut ids,
        &aid,
        rein_core::state::AnomalyKind::UncertainCommit,
        at,
    )
    .unwrap();
    let (generation, _) =
        recovery::resume_commit_new_generation(&mut log, &mut ids, &aid, at).unwrap();
    assert_eq!(generation, 2);

    // The old generation may not commit.
    let stale = fence::guard_commit(&log, &aid, 1);
    assert!(
        matches!(
            stale,
            Err(fence::FenceError::Stale {
                presented: 1,
                current: 2
            })
        ),
        "{stale:?}"
    );
    fence::guard_commit(&log, &aid, 2).unwrap();

    // The fence receipt exists, issued by the local ledger.
    let fence_receipts = log
        .for_attempt(&aid)
        .filter(|e| {
            matches!(
                e.body,
                ReceiptBody::FenceGeneration {
                    issuer: FenceIssuer::LocalLedger,
                    ..
                }
            )
        })
        .count();
    assert_eq!(fence_receipts, 2, "initial + recovery generations");

    // Recovery changed nothing semantic: the pack hash is untouched.
    assert_eq!(pack.verify_sealed().unwrap(), hash_before);

    // And a recovery resume that names a non-fence receipt is refused.
    recovery::enter_recovery(
        &mut log,
        &mut ids,
        &aid,
        rein_core::state::AnomalyKind::StaleRun,
        t("2026-08-19T08:05:00Z"),
    )
    .unwrap_err(); // currently preparing, not running — the state machine refuses
}

/// Invariant 28 (schema-side) — secrets are references in durable state;
/// quarantine is a validator verdict plus a receipt that withholds from
/// selection. Symbols: `secretref::Redactor`, `receipts::ReceiptBody::Quarantine`,
/// `selection::select_and_record`.
#[test]
fn inv28__secretref__value_unrepresentable_in_durable_state_and_quarantine_withholds_selection() {
    let redactor = fixture_redactor();
    let leaky = format!(
        "here is {} in output",
        rein_core::fakes::FIXTURE_SECRET_VALUE
    );
    let (scrubbed, report) = redactor.scrub(&leaky);
    assert!(!scrubbed.contains(rein_core::fakes::FIXTURE_SECRET_VALUE));
    assert!(scrubbed.contains("«redacted:secret-ref:fixture»"));
    assert_eq!(report.replacements.get("secret-ref:fixture"), Some(&1));
    assert!(redactor.scan(&leaky).is_some());
    assert!(redactor.scan(&scrubbed).is_none());

    // A SecretRefId serializes as a reference string — there is no value slot.
    let json = serde_json::to_string(&SecretRefId::parse("secret-ref:fmp-key").unwrap()).unwrap();
    assert_eq!(json, r#""secret-ref:fmp-key""#);

    // The secret-leak run: quarantine receipt exists, outcome failure, and the
    // attempt is withheld from selection.
    let mut r = run_fixture_pipeline(&SecretLeak);
    let (outcome, reason) = r.outcome.clone().unwrap();
    assert_eq!(outcome, TerminalOutcome::Failure);
    assert_eq!(reason, ReasonCode::artifact_quarantined_secret());
    assert!(r.log.for_attempt(&r.attempt_id).any(|e| matches!(
        e.body,
        ReceiptBody::Quarantine {
            withheld_from_selection: true,
            ..
        }
    )));
    // Its capture was scrubbed before it became durable.
    let capture_clean = r.log.for_attempt(&r.attempt_id).all(|e| match &e.body {
        ReceiptBody::Capture { capture, .. } => !capture
            .stdout
            .contains(rein_core::fakes::FIXTURE_SECRET_VALUE),
        _ => true,
    });
    assert!(capture_clean, "secret values never enter durable capture");

    let at = t("2026-08-19T10:00:00Z");
    let task = r.pack.task_ref.clone();
    let candidates = [r.attempt_id.clone()];
    select_and_record(&mut r.log, &mut r.ids, &task, &candidates, at);
    assert!(
        !task_satisfied(&r.log, &task),
        "a quarantined attempt cannot be selected"
    );
}

/// Invariant 30 — subprocess output is decoded incrementally, retaining
/// trailing partial sequences. Symbol: `capture::Utf8StreamDecoder`.
#[test]
fn inv30__capture_utf8streamdecoder__multibyte_survives_pathological_chunking() {
    let text = CJK_TEXT;
    let bytes = text.as_bytes();

    for chunk_size in 1..=7 {
        let mut decoder = Utf8StreamDecoder::new();
        let mut incremental = String::new();
        let mut lossy_per_chunk = String::new();
        for chunk in bytes.chunks(chunk_size) {
            incremental.push_str(&decoder.feed(chunk));
            lossy_per_chunk.push_str(&String::from_utf8_lossy(chunk));
        }
        incremental.push_str(&decoder.finish());

        assert_eq!(incremental, text, "chunk size {chunk_size}");
        if chunk_size < 3 {
            // The broken baseline this invariant exists to kill: per-chunk
            // lossy decode destroys every multi-byte character at small chunks.
            assert_ne!(lossy_per_chunk, text, "chunk size {chunk_size}");
        }
    }

    // A stream that ends mid-character states the loss, exactly once.
    let mut decoder = Utf8StreamDecoder::new();
    let partial = &"研".as_bytes()[..1];
    assert_eq!(decoder.feed(partial), "");
    assert_eq!(decoder.finish(), "\u{FFFD}");
}
