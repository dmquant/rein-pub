//! State-machine properties (§3, invariant 22): the edge table is exhaustive,
//! walks only take legal edges, every applied transition appends exactly one
//! receipt, and replaying the ledger reconstructs the state.

mod common;

use common::*;
use proptest::prelude::*;
use rein_core::idempotency::{admit, AdmissionOutcome, AttemptRequest, RequestKind};
use rein_core::ids::{AttemptId, IdGen};
use rein_core::outcome::ReasonCode;
use rein_core::receipts::{AbortKind, ReceiptBody, ReceiptLog};
use rein_core::recovery;
use rein_core::state::{
    abort_to_classifying, apply_transition, resolve_state, AnomalyKind, AttemptState,
    TransitionCauseRecord,
};

fn fresh_attempt(log: &mut ReceiptLog, ids: &mut IdGen) -> AttemptId {
    let pack = sealed_sample_pack();
    let at = t("2026-08-19T08:00:00Z");
    let AdmissionOutcome::New { attempt, .. } = admit(
        log,
        ids,
        &AttemptRequest {
            task_ref: pack.task_ref.clone(),
            context_pack: pack,
            kind: RequestKind::Fresh,
        },
        at,
    )
    .unwrap() else {
        panic!()
    };
    attempt.attempt_id
}

/// Position an attempt at a given state by walking real edges.
fn position_at(log: &mut ReceiptLog, ids: &mut IdGen, state: AttemptState) -> AttemptId {
    use AttemptState as S;
    let at = t("2026-08-19T08:00:00Z");
    let aid = fresh_attempt(log, ids);
    let advance_chain: &[S] = match state {
        S::Created => &[],
        S::Admitted => &[S::Admitted],
        S::Preparing => &[S::Admitted, S::Preparing],
        S::Running => &[S::Admitted, S::Preparing, S::Running],
        S::CommitPending => &[S::Admitted, S::Preparing, S::Running, S::CommitPending],
        S::Validating => &[
            S::Admitted,
            S::Preparing,
            S::Running,
            S::CommitPending,
            S::Validating,
        ],
        S::Classifying => &[
            S::Admitted,
            S::Preparing,
            S::Running,
            S::CommitPending,
            S::Validating,
            S::Classifying,
        ],
        S::Terminal | S::Closed => &[
            S::Admitted,
            S::Preparing,
            S::Running,
            S::CommitPending,
            S::Validating,
            S::Classifying,
        ],
        S::RecoveryPending => &[S::Admitted, S::Preparing, S::Running],
    };
    for to in advance_chain {
        advance(log, ids, &aid, *to, at);
    }
    match state {
        AttemptState::RecoveryPending => {
            recovery::enter_recovery(log, ids, &aid, AnomalyKind::StaleRun, at).unwrap();
        }
        AttemptState::Terminal | AttemptState::Closed => {
            let terminal_receipt = log.append(
                ids,
                &aid,
                at,
                ReceiptBody::Terminal {
                    outcome: rein_core::outcome::TerminalOutcome::Failure,
                    reason: ReasonCode::mandatory_validator_failed(),
                    supporting: vec![],
                },
            );
            apply_transition(
                log,
                ids,
                &aid,
                AttemptState::Terminal,
                TransitionCauseRecord::ClassificationComplete { terminal_receipt },
                at,
            )
            .unwrap();
            if state == AttemptState::Closed {
                apply_transition(
                    log,
                    ids,
                    &aid,
                    AttemptState::Closed,
                    TransitionCauseRecord::Close,
                    at,
                )
                .unwrap();
            }
        }
        _ => {}
    }
    assert_eq!(resolve_state(log, &aid).unwrap(), state);
    aid
}

/// The complete Advance-cause edge table, checked exhaustively over all
/// 10×10 pairs. Adding, removing, or rerouting an edge reddens this test.
#[test]
fn advance_edges_are_exactly_the_drawn_pipeline() {
    use AttemptState as S;
    let advance_edges = [
        (S::Created, S::Admitted),
        (S::Admitted, S::Preparing),
        (S::Preparing, S::Running),
        (S::Running, S::CommitPending),
        (S::CommitPending, S::Validating),
        (S::Validating, S::Classifying),
    ];
    let at = t("2026-08-19T08:00:00Z");
    for from in AttemptState::ALL {
        for to in AttemptState::ALL {
            let mut log = ReceiptLog::new();
            let mut ids = IdGen::new();
            let aid = position_at(&mut log, &mut ids, from);
            let result = apply_transition(
                &mut log,
                &mut ids,
                &aid,
                to,
                TransitionCauseRecord::Advance,
                at,
            );
            let expected = advance_edges.contains(&(from, to));
            assert_eq!(
                result.is_ok(),
                expected,
                "Advance {from:?} → {to:?} legality"
            );
        }
    }
}

/// Abort edges exist from exactly {created, admitted, preparing} (objection
/// O2's accepted resolution) and nowhere else.
#[test]
fn abort_edges_exist_from_exactly_the_three_pre_run_states() {
    use AttemptState as S;
    let at = t("2026-08-19T08:00:00Z");
    for from in AttemptState::ALL {
        let mut log = ReceiptLog::new();
        let mut ids = IdGen::new();
        let aid = position_at(&mut log, &mut ids, from);
        let result = abort_to_classifying(
            &mut log,
            &mut ids,
            &aid,
            AbortKind::Cancelled {
                by: "operator".into(),
            },
            "test abort",
            at,
        );
        let expected = matches!(from, S::Created | S::Admitted | S::Preparing);
        assert_eq!(result.is_ok(), expected, "abort from {from:?}");
        if expected {
            assert_eq!(resolve_state(&log, &aid).unwrap(), S::Classifying);
            // The abort-cause receipt landed with the transition.
            assert!(log
                .for_attempt(&aid)
                .any(|e| matches!(e.body, ReceiptBody::AbortCause { .. })));
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Action {
    Advance(AttemptState),
    Abort,
    EnterRecovery,
    ResumeFromRecovery,
    CloseAsUnknown,
    Close,
}

fn arb_action() -> impl Strategy<Value = Action> {
    prop_oneof![
        prop::sample::select(AttemptState::ALL.to_vec()).prop_map(Action::Advance),
        Just(Action::Abort),
        Just(Action::EnterRecovery),
        Just(Action::ResumeFromRecovery),
        Just(Action::CloseAsUnknown),
        Just(Action::Close),
    ]
}

/// The model oracle: what each action should do from each state.
fn model_next(state: AttemptState, action: Action) -> Option<AttemptState> {
    use AttemptState as S;
    match action {
        Action::Advance(to) => {
            let ok = matches!(
                (state, to),
                (S::Created, S::Admitted)
                    | (S::Admitted, S::Preparing)
                    | (S::Preparing, S::Running)
                    | (S::Running, S::CommitPending)
                    | (S::CommitPending, S::Validating)
                    | (S::Validating, S::Classifying)
            );
            ok.then_some(to)
        }
        Action::Abort => {
            matches!(state, S::Created | S::Admitted | S::Preparing).then_some(S::Classifying)
        }
        Action::EnterRecovery => matches!(state, S::Running).then_some(S::RecoveryPending),
        Action::ResumeFromRecovery => matches!(state, S::RecoveryPending).then_some(S::Preparing),
        Action::CloseAsUnknown => matches!(state, S::RecoveryPending).then_some(S::Terminal),
        Action::Close => matches!(state, S::Terminal).then_some(S::Closed),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Random walks: the implementation agrees with the model at every step,
    /// legal steps append receipts, refused steps append nothing, and the
    /// resolved state always comes from replaying the ledger.
    #[test]
    fn random_walks_agree_with_the_model(actions in prop::collection::vec(arb_action(), 1..24)) {
        let mut log = ReceiptLog::new();
        let mut ids = IdGen::new();
        let at = t("2026-08-19T08:00:00Z");
        let aid = fresh_attempt(&mut log, &mut ids);
        let mut model = AttemptState::Created;

        for action in actions {
            let before_len = log.len();
            let expected = model_next(model, action);
            let result: Result<(), ()> = match action {
                Action::Advance(to) => apply_transition(
                    &mut log, &mut ids, &aid, to, TransitionCauseRecord::Advance, at,
                ).map(|_| ()).map_err(|_| ()),
                Action::Abort => abort_to_classifying(
                    &mut log, &mut ids, &aid,
                    AbortKind::BudgetDenied { detail: "walk".into() }, "walk", at,
                ).map(|_| ()).map_err(|_| ()),
                Action::EnterRecovery => recovery::enter_recovery(
                    &mut log, &mut ids, &aid, AnomalyKind::DuplicateCallback, at,
                ).map(|_| ()).map_err(|_| ()),
                Action::ResumeFromRecovery => recovery::resume_commit_new_generation(
                    &mut log, &mut ids, &aid, at,
                ).map(|_| ()).map_err(|_| ()),
                Action::CloseAsUnknown => recovery::close_attempt_as_unknown(
                    &mut log, &mut ids, &aid, ReasonCode::closed_as_unknown_by_operator(), vec![], at,
                ).map(|_| ()).map_err(|_| ()),
                Action::Close => apply_transition(
                    &mut log, &mut ids, &aid, AttemptState::Closed, TransitionCauseRecord::Close, at,
                ).map(|_| ()).map_err(|_| ()),
            };

            match expected {
                Some(next) => {
                    prop_assert!(result.is_ok(), "{action:?} from {model:?} should be legal");
                    prop_assert!(log.len() > before_len, "legal transitions append receipts");
                    model = next;
                }
                None => {
                    prop_assert!(result.is_err(), "{action:?} from {model:?} should be refused");
                    prop_assert_eq!(log.len(), before_len, "refusals append nothing");
                }
            }
            prop_assert_eq!(resolve_state(&log, &aid).unwrap(), model);
        }

        // Terminal-and-closed are absorbing: nothing ever leaves Closed.
        if model == AttemptState::Closed {
            for to in AttemptState::ALL {
                let r = apply_transition(
                    &mut log, &mut ids, &aid, to, TransitionCauseRecord::Advance, at,
                );
                prop_assert!(r.is_err());
            }
        }
    }
}
