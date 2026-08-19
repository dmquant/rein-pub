//! M5 (§13): the two-track eval and the ops task types — verify / settle /
//! monitor — end to end. The M5 acceptance: a benchmark run produces scores
//! with bootstrap CIs and zero influence on any TerminalOutcome. Invariant
//! 21's aggregation face completes: direct and inherited evidence are never
//! summed.
#![allow(non_snake_case)]

use rein_core::context_pack::PitMode;
use rein_core::entities::{Epoch, Mission, Plan, PlanNode, TaskVersion};
use rein_core::ids::{ArtifactRef, MissionRef, PlanRef, TaskRef, ValidatorRef, WorkspaceRef};
use rein_core::outcome::TerminalOutcome;
use rein_core::time::Timestamp;
use rein_finance::eval::*;
use rein_finance::ops::*;
use rein_finance::validators::{register_finance_validators, FinanceContext};
use rein_runtime::cas::Cas;
use rein_runtime::clock::FixedClock;
use rein_runtime::engine::Engine;
use rein_runtime::store::{CaptureRow, Store};
use rein_runtime::workspace::{SecretBroker, Workspace};
use std::collections::BTreeMap;

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

#[test]
fn harsher_verdict_wins_total_order() {
    use Verdict::*;
    assert_eq!(harsher(Supports, Refutes), Refutes);
    assert_eq!(harsher(Refutes, Supports), Refutes);
    assert_eq!(harsher(Supports, Inconclusive), Inconclusive);
    assert_eq!(harsher(Supports, Supports), Supports);
}

#[test]
fn verdicts_discipline_challenger_isolation_and_coverage() {
    let row = |id: &str| VerdictRow {
        claim_id: id.into(),
        verdict: Verdict::Supports,
        refutation_condition: "counter-evidence X".into(),
        basis: EvidenceBasis::Direct { refs: vec![] },
    };
    let mut v = Verdicts {
        schema: VERDICTS_SCHEMA.into(),
        verified_attempt_ref: "rein:attempt_000001".into(),
        producer_hand: "agy".into(),
        challenger_hand: "finance:ops".into(),
        rows: vec![row("c1"), row("c2")],
    };
    check_verdicts(&v, &["c1".into(), "c2".into()]).unwrap();

    // Same hand: refused — verification requires independence (§4).
    v.challenger_hand = "agy".into();
    assert!(matches!(
        check_verdicts(&v, &["c1".into(), "c2".into()]),
        Err(OpsError::ChallengerNotIndependent(_))
    ));
    v.challenger_hand = "finance:ops".into();

    // Coverage: the denominator is the claims under test (invariant 20).
    assert!(matches!(
        check_verdicts(&v, &["c1".into(), "c2".into(), "c3".into()]),
        Err(OpsError::VerdictCoverage {
            expected: 3,
            got: 2
        })
    ));

    // A supports verdict still states what would refute it.
    v.rows[0].refutation_condition = "  ".into();
    assert!(matches!(
        check_verdicts(&v, &["c1".into(), "c2".into()]),
        Err(OpsError::NoRefutationCondition(_))
    ));
}

/// Invariant 21, aggregation face — direct and inherited are never summed.
/// Symbol: `ops::direct_score`.
#[test]
fn inv21__ops_direct_score__inherited_evidence_never_joins_the_sum() {
    let rows = vec![
        VerdictRow {
            claim_id: "c1".into(),
            verdict: Verdict::Supports,
            refutation_condition: "x".into(),
            basis: EvidenceBasis::Direct { refs: vec![] },
        },
        VerdictRow {
            claim_id: "c2".into(),
            verdict: Verdict::Supports,
            refutation_condition: "x".into(),
            basis: EvidenceBasis::Inherited {
                from: "verdicts of attempt_000009".into(),
            },
        },
    ];
    let s = direct_score(&rows);
    assert_eq!(s.direct_supports, 1, "the inherited support did not sum");
    assert_eq!(
        s.inherited_excluded.len(),
        1,
        "…and is reported, not dropped"
    );
}

#[test]
fn settle_verdicts_never_invented() {
    let realized = Realized {
        value: 250.0,
        as_of: t("2027-01-15T00:00:00Z"),
        basis_ref: "sha256:aa".into(),
    };
    // Claimed undervalued (implied > market), realized above market: confirmed.
    assert_eq!(
        settle_verdict(300.0, 220.0, Some(&realized)),
        SettleVerdict::Confirmed
    );
    // Claimed overvalued, price went up: contradicted.
    assert_eq!(
        settle_verdict(150.0, 220.0, Some(&realized)),
        SettleVerdict::Contradicted
    );
    // Nothing bears: expired_unobserved, never a guess.
    assert_eq!(
        settle_verdict(300.0, 220.0, None),
        SettleVerdict::ExpiredUnobserved
    );

    // Structural discipline: invented verdicts fail.
    let bad = Settlements {
        schema: SETTLEMENTS_SCHEMA.into(),
        rows: vec![SettleRow {
            subject: "security:nvda".into(),
            valuation_attempt_ref: "rein:attempt_000002".into(),
            horizon: t("2027-12-31T00:00:00Z"),
            implied_per_share: 300.0,
            market_at_valuation: 220.0,
            realized: None,
            verdict: SettleVerdict::Confirmed,
        }],
        coverage: SettleCoverage {
            due: 1,
            settled: 1,
            expired_unobserved: 0,
        },
    };
    let err = check_settlements(&bad, 1).unwrap_err();
    assert!(err.to_string().contains("never invented"));
}

#[test]
fn financegym_scoring_deterministic_with_bootstrap_ci() {
    let questions = load_questions_jsonl(SAMPLE_QUESTIONS).unwrap();
    assert_eq!(questions.len(), 3);
    let mut answers = BTreeMap::new();
    answers.insert(
        "fg-01".to_string(),
        "The free cash flow was 96.7 billion USD.".to_string(),
    );
    answers.insert("fg-02".to_string(), "TSMC fabricates them.".to_string());
    // fg-03 unanswered → tier 0.
    let r1 = score_run(&questions, &answers);
    let r2 = score_run(&questions, &answers);
    assert_eq!(r1.s, 8, "4 + 4 + 0");
    assert!((r1.score - 8.0 / 12.0).abs() < 1e-12, "s/(4n)");
    assert_eq!(
        r1.bootstrap_ci_95, r2.bootstrap_ci_95,
        "seeded bootstrap is deterministic"
    );
    assert!(r1.bootstrap_ci_95.0 <= r1.score && r1.score <= r1.bootstrap_ci_95.1);
}

fn ops_fixture() -> (Workspace, Store, tempfile::TempDir, tempfile::TempDir) {
    let ws_dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:m5").unwrap(),
        t("2027-01-01T00:00:00Z"),
    )
    .unwrap();
    let mut store = Store::open(&ws.ledger_db()).unwrap();
    store
        .put_mission(&Mission {
            mission_ref: MissionRef::parse("mission:m5").unwrap(),
            objective: "ops".into(),
            closure_conditions: vec![],
            created_at: t("2027-01-01T00:00:00Z"),
        })
        .unwrap();
    store
        .put_epoch(&Epoch {
            epoch_ref: rein_core::ids::EpochRef::parse("epoch:m5").unwrap(),
            mission_ref: MissionRef::parse("mission:m5").unwrap(),
            source_cutoff: t("2027-06-01T00:00:00Z"),
            knowledge_cutoff: t("2027-06-01T00:00:00Z"),
            pit_mode: PitMode::Production,
            provider_pins: Default::default(),
            policy_version: "policy:v1".into(),
            budget_envelope: rein_core::context_pack::Budget {
                max_steps: 8,
                per_step_timeout_ms: 240_000,
                tokens: None,
                tool_calls: None,
            },
            sealed: true,
        })
        .unwrap();
    let _ = (std::fs::create_dir_all(ws.skills()),);
    (ws, store, ws_dir, config)
}

fn plant(ws: &Workspace, store: &mut Store, note: &str, body: serde_json::Value) -> ArtifactRef {
    let cas = Cas::new(ws.objects());
    let bytes = serde_json::to_vec_pretty(&body).unwrap();
    let digest = cas.put(&bytes).unwrap();
    store
        .insert_capture(&CaptureRow {
            digest: digest.clone(),
            tool: "fixture".into(),
            params: note.to_string(),
            provider: "fixture".into(),
            media_type: "application/json".into(),
            as_of: Some(t("2027-01-01T00:00:00Z")),
            as_of_basis: Some("provider".into()),
            retrieved_at: t("2027-01-02T00:00:00Z"),
            url: None,
            host: None,
            note: Some(note.to_string()),
        })
        .unwrap();
    ArtifactRef::parse(&format!("artifact:{digest}")).unwrap()
}

fn ops_contract(artifact: &str, schema: &str) -> rein_core::context_pack::OutputContract {
    rein_core::context_pack::OutputContract {
        required_artifacts: vec![rein_core::context_pack::RequiredArtifact {
            name: artifact.into(),
            media_type: "application/json".into(),
            schema_ref: Some(schema.into()),
            min_bytes: None,
        }],
        validators: vec![
            ValidatorRef::parse("artifact-wellformed@1").unwrap(),
            ValidatorRef::parse("ops-discipline@1").unwrap(),
        ],
    }
}

fn run_ops_task(
    ws: &Workspace,
    store: &mut Store,
    config: &tempfile::TempDir,
    task_name: &str,
    task_type: &str,
    contract: rein_core::context_pack::OutputContract,
    inputs: Vec<ArtifactRef>,
) -> rein_runtime::engine::ExecutionReport {
    let plan_ref = PlanRef::parse(&format!("plan:{task_type}@1")).unwrap();
    store
        .put_plan(&Plan {
            plan_ref: plan_ref.clone(),
            nodes: vec![PlanNode {
                task_ref: TaskRef::parse(task_name).unwrap(),
                depends_on: vec![],
            }],
        })
        .unwrap();
    store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse(task_name).unwrap(),
            plan_ref,
            task_type: task_type.into(),
            output_contract: contract,
            satisfaction_criteria: vec![],
            inputs,
            universe: vec![],
        })
        .unwrap();
    let captures: BTreeMap<String, CaptureRow> = store
        .list_captures()
        .unwrap()
        .into_iter()
        .map(|c| (c.digest.as_str().to_string(), c))
        .collect();
    let clock = FixedClock::new(t("2027-01-03T00:00:00Z"));
    let broker = SecretBroker::open(config.path(), &ws.root).unwrap();
    let mut engine = Engine::new(ws, store, &clock, broker);
    engine
        .hands
        .register(Box::new(rein_finance::hands::FinanceOps));
    register_finance_validators(
        &mut engine.validators,
        FinanceContext {
            captures,
            cas: Cas::new(ws.objects()),
            source_cutoff: t("2027-06-01T00:00:00Z"),
        },
    );
    engine
        .run_task(
            &TaskRef::parse(task_name).unwrap(),
            Some("finance:ops"),
            None,
        )
        .unwrap()
}

#[test]
fn m5__settle_task_end_to_end_and_internal_eval_ranks_hands() {
    let (ws, mut store, _d1, config) = ops_fixture();

    // A due valuation produced by a recorded hand: fabricate the producing
    // attempt via a real run of finance:ops on a monitor-ish noop? Instead:
    // insert the run row through a real valuation-free path is impossible —
    // so settle against a synthetic attempt ref; ranking then reports the
    // hand as unrecorded, which is stated, not invented.
    let due = plant(
        &ws,
        &mut store,
        "due:2027H1",
        serde_json::json!([{
            "subject": "security:nvda",
            "valuation_attempt_ref": "rein:attempt_009999",
            "horizon": "2027-06-30T00:00:00Z",
            "implied_per_share": 300.0,
            "market_at_valuation": 220.0,
            "realized": {"value": 250.0, "as_of": "2027-06-30T00:00:00Z", "basis_ref": "sha256:aa"}
        }, {
            "subject": "security:amd",
            "valuation_attempt_ref": "rein:attempt_009998",
            "horizon": "2027-06-30T00:00:00Z",
            "implied_per_share": 100.0,
            "market_at_valuation": 120.0,
            "realized": null
        }]),
    );
    let report = run_ops_task(
        &ws,
        &mut store,
        &config,
        "task:settle-2027h1@1",
        "settle",
        ops_contract("settlement.json", "schema:rein.settlements/v1"),
        vec![due],
    );
    let (outcome, _) = report.outcome.clone().unwrap();
    assert_eq!(
        outcome,
        TerminalOutcome::Success,
        "settle discipline validated in-pipeline"
    );

    // Internal eval: the settlement joins back through attempt refs; the
    // unrecorded hand is reported as such (absence stated).
    let cas = Cas::new(ws.objects());
    let ranking = rank_hands_on_settled(&cas, &store).unwrap();
    assert_eq!(ranking.len(), 1);
    assert_eq!(ranking[0].hand, "(hand unrecorded)");
    assert_eq!(
        ranking[0].settled, 1,
        "expired_unobserved never counts as settled"
    );
    assert_eq!(ranking[0].confirmed, 1);
    assert_eq!(ranking[0].score, Some(1.0));

    // Zero influence on TerminalOutcome: scoring appended nothing.
    let before = store.receipt_count().unwrap();
    let _ = rank_hands_on_settled(&cas, &store).unwrap();
    let questions = load_questions_jsonl(SAMPLE_QUESTIONS).unwrap();
    let _ = score_run(&questions, &BTreeMap::new());
    assert_eq!(store.receipt_count().unwrap(), before);
}

#[test]
fn m5__verify_and_monitor_tasks_end_to_end() {
    let (ws, mut store, _d1, config) = ops_fixture();

    // verify: claims + meta pinned; the ops hand issues inconclusive
    // verdicts with refutation conditions; ops-discipline validates.
    let claims = plant(
        &ws,
        &mut store,
        "claims:under-test",
        serde_json::json!({
            "schema": "rein.claims/v1",
            "claims": [
                {"id": "c1", "text": "x", "kind": "forecast", "falsifier": "y misses"},
                {"id": "c2", "text": "z", "kind": "fact"}
            ],
            "citations": [],
            "coverage": {"eligible_inputs": 0, "consumed": [], "withheld": [], "hosts": {}}
        }),
    );
    let meta = plant(
        &ws,
        &mut store,
        "meta:verify",
        serde_json::json!({"producer_hand": "agy", "verified_attempt_ref": "rein:attempt_000042"}),
    );
    let report = run_ops_task(
        &ws,
        &mut store,
        &config,
        "task:verify-42@1",
        "verify",
        ops_contract("verdict.json", "schema:rein.verdicts/v1"),
        vec![claims, meta],
    );
    assert_eq!(
        report.outcome.clone().unwrap().0,
        TerminalOutcome::Success,
        "challenger independence + coverage validated in-pipeline"
    );

    // monitor: two pinned series; moved-only diff recomputed by the validator.
    let (ws2, mut store2, _d2, config2) = ops_fixture();
    let prior = plant(
        &ws2,
        &mut store2,
        "series-prior",
        serde_json::json!({"subject":"security:nvda","metric":"dc_revenue","points":[
            {"as_of":"2027-03-31T00:00:00Z","value":100.0,"unit":"ccy"},
            {"as_of":"2027-04-30T00:00:00Z","value":110.0,"unit":"ccy"}]}),
    );
    let newer = plant(
        &ws2,
        &mut store2,
        "series-new",
        serde_json::json!({"subject":"security:nvda","metric":"dc_revenue","points":[
            {"as_of":"2027-03-31T00:00:00Z","value":100.0,"unit":"ccy"},
            {"as_of":"2027-04-30T00:00:00Z","value":115.0,"unit":"ccy"},
            {"as_of":"2027-05-31T00:00:00Z","value":120.0,"unit":"ccy"}]}),
    );
    let report = run_ops_task(
        &ws2,
        &mut store2,
        &config2,
        "task:monitor-nvda@1",
        "monitor",
        ops_contract("drivers-diff.json", "schema:rein.drivers-diff/v1"),
        vec![prior, newer],
    );
    let (outcome, _) = report.outcome.clone().unwrap();
    assert_eq!(outcome, TerminalOutcome::Success);

    // The committed diff: one moved, one inserted — a row inserted is not a
    // value changed.
    let cas = Cas::new(ws2.objects());
    let by_name: BTreeMap<_, _> = report.artifacts.iter().cloned().collect();
    let diff: DriversDiff =
        serde_json::from_slice(&cas.read_verified(&by_name["drivers-diff.json"]).unwrap()).unwrap();
    assert_eq!(diff.diff.moved.len(), 1);
    assert_eq!(diff.diff.inserted.len(), 1);
}
