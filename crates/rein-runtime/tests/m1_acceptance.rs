//! M1 acceptance (§13): the local deterministic proof, over the real SQLite
//! ledger and filesystem CAS.
//!
//! - Same ContextPack through fake-a and fake-b (a retry, so a different
//!   attempt generation) yields identical required-artifact digests.
//! - `exit0-empty` and `hash-mismatch` fail closed per the §6 matrix.
//! - The ledger is append-only by trigger (invariant 22, M1 re-point).
//! - Strict replay catches CAS tampering.
//! - Invariant 27: credentials never resolve from the workspace tree
//!   (symbol: `workspace::SecretBroker::open`).
//!
//! Test names carry `mN__<symbol>__<claim>`; the separators are load-bearing.
#![allow(non_snake_case)]

use rein_core::context_pack::{OutputContract, PitMode, RequiredArtifact};
use rein_core::entities::{Epoch, Mission, Plan, PlanNode, TaskVersion};
use rein_core::ids::{MissionRef, PlanRef, TaskRef, ValidatorRef, WorkspaceRef};
use rein_core::outcome::TerminalOutcome;
use rein_core::state::AttemptState;
use rein_core::time::Timestamp;
use rein_runtime::clock::FixedClock;
use rein_runtime::engine::Engine;
use rein_runtime::store::Store;
use rein_runtime::workspace::{SecretBroker, Workspace};

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

fn contract() -> OutputContract {
    OutputContract {
        required_artifacts: vec![
            RequiredArtifact {
                name: "valuation.json".into(),
                media_type: "application/json".into(),
                schema_ref: None,
                min_bytes: None,
            },
            RequiredArtifact {
                name: "memo.md".into(),
                media_type: "text/markdown".into(),
                schema_ref: None,
                min_bytes: Some(8),
            },
        ],
        validators: vec![
            ValidatorRef::parse("artifact-wellformed@1").unwrap(),
            ValidatorRef::parse("secret-scan@1").unwrap(),
        ],
    }
}

struct Fixture {
    ws: Workspace,
    store: Store,
    config_root: tempfile::TempDir,
    _ws_dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let ws_dir = tempfile::tempdir().unwrap();
    let config_root = tempfile::tempdir().unwrap();
    // The fixture secret, resolvable as `secret-ref:fixture` (invariant 28).
    std::fs::write(
        config_root.path().join("secrets.toml"),
        format!("fixture = \"{}\"\n", rein_core::fakes::FIXTURE_SECRET_VALUE),
    )
    .unwrap();

    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:test").unwrap(),
        t("2026-08-19T00:00:00Z"),
    )
    .unwrap();
    let mut store = Store::open(&ws.ledger_db()).unwrap();

    let mission = Mission {
        mission_ref: MissionRef::parse("mission:test").unwrap(),
        objective: "M1 acceptance".into(),
        closure_conditions: vec![],
        created_at: t("2026-08-19T00:00:00Z"),
    };
    store.put_mission(&mission).unwrap();
    store
        .put_epoch(&Epoch {
            epoch_ref: rein_core::ids::EpochRef::parse("epoch:m1").unwrap(),
            mission_ref: mission.mission_ref.clone(),
            source_cutoff: t("2026-08-18T00:00:00Z"),
            knowledge_cutoff: t("2026-08-18T00:00:00Z"),
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
    let plan = Plan {
        plan_ref: PlanRef::parse("plan:m1@1").unwrap(),
        nodes: vec![PlanNode {
            task_ref: TaskRef::parse("task:m1@1").unwrap(),
            depends_on: vec![],
        }],
    };
    store.put_plan(&plan).unwrap();
    store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:m1@1").unwrap(),
            plan_ref: plan.plan_ref.clone(),
            task_type: "valuation".into(),
            output_contract: contract(),
            satisfaction_criteria: vec!["first-valid-deterministic@1".into()],
        })
        .unwrap();

    Fixture {
        ws,
        store,
        config_root,
        _ws_dir: ws_dir,
    }
}

fn broker(f: &Fixture) -> SecretBroker {
    SecretBroker::open(f.config_root.path(), &f.ws.root).unwrap()
}

fn task() -> TaskRef {
    TaskRef::parse("task:m1@1").unwrap()
}

#[test]
fn m1_acceptance__same_pack_through_fake_a_and_fake_b_yields_identical_digests() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);
    let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);

    let first = engine
        .run_task(&task(), Some("fake:deterministic-a"), None)
        .unwrap();
    assert_eq!(
        first.outcome.as_ref().map(|(o, _)| *o),
        Some(TerminalOutcome::Success)
    );
    assert!(first.task_satisfied);

    // Retry: byte-identical pack, next generation, different hand (the C2
    // amendment makes the rebinding legal — same semantic hash).
    let second = engine
        .retry(&first.attempt_id, Some("fake:deterministic-b"))
        .unwrap();
    assert_eq!(
        second.outcome.as_ref().map(|(o, _)| *o),
        Some(TerminalOutcome::Success)
    );
    assert_ne!(first.attempt_id, second.attempt_id);

    let mut a = first.artifacts.clone();
    let mut b2 = second.artifacts.clone();
    a.sort();
    b2.sort();
    assert_eq!(
        a, b2,
        "M1 kill criterion: digest equality must be deterministic"
    );
    assert!(!a.is_empty());

    // Same context hash on both attempts (invariant 6).
    let ra = f.store.get_attempt(&first.attempt_id).unwrap();
    let rb = f.store.get_attempt(&second.attempt_id).unwrap();
    assert_eq!(ra.context_hash, rb.context_hash);
    assert_eq!(rb.generation, ra.generation + 1);
}

#[test]
fn m1_acceptance__exit0_empty_and_hash_mismatch_fail_closed() {
    for (hand, expected_reason) in [
        ("fake:exit0-empty", "required_artifact_absent"),
        ("fake:hash-mismatch", "readback_digest_mismatch"),
    ] {
        let mut f = fixture();
        let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
        let b = broker(&f);
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        let r = engine.run_task(&task(), Some(hand), None).unwrap();
        let (outcome, reason) = r.outcome.expect("terminal");
        assert_eq!(outcome, TerminalOutcome::ArtifactInvalid, "{hand}");
        assert_eq!(reason.0, expected_reason, "{hand}");
        assert_eq!(outcome.exit_code().code(), 12, "{hand}");
        assert!(!r.task_satisfied, "{hand}: never selectable");
    }
}

#[test]
fn m1_matrix__remaining_rows_over_the_real_store() {
    let rows: &[(&str, TerminalOutcome, &str)] = &[
        (
            "fake:duplicate-callback",
            TerminalOutcome::Success,
            "required_outputs_valid",
        ),
        (
            "fake:timeout",
            TerminalOutcome::TimedOut,
            "per_step_budget_exceeded",
        ),
        (
            "fake:secret-leak",
            TerminalOutcome::Failure,
            "artifact_quarantined_secret",
        ),
        (
            "fake:partial-output",
            TerminalOutcome::PartialSuccess,
            "some_required_valid",
        ),
        (
            "fake:cjk-splitter",
            TerminalOutcome::Success,
            "required_outputs_valid",
        ),
    ];
    for (hand, expected, reason) in rows {
        let mut f = fixture();
        let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
        let b = broker(&f);
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        let r = engine.run_task(&task(), Some(hand), None).unwrap();
        let (outcome, got_reason) = r.outcome.expect("terminal");
        assert_eq!(&outcome, expected, "{hand}");
        assert_eq!(&got_reason.0, reason, "{hand}");
    }
}

#[test]
fn m1_matrix__disconnect_enters_recovery_and_only_explicit_close_yields_unknown() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);
    let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
    let r = engine
        .run_task(&task(), Some("fake:unknown-after-disconnect"), None)
        .unwrap();
    assert_eq!(r.final_state, AttemptState::RecoveryPending);
    assert!(r.outcome.is_none(), "no inferred verdict (invariant 5)");

    let closed = engine
        .close_as_unknown(&r.attempt_id, "run_lost_no_evidence")
        .unwrap();
    let (outcome, reason) = closed.outcome.expect("terminal");
    assert_eq!(outcome, TerminalOutcome::Unknown);
    assert_eq!(reason.0, "run_lost_no_evidence");
    assert_eq!(outcome.exit_code().code(), 11);
}

#[test]
fn m1__cancellation_before_run_aborts_to_cancelled() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);

    // The next attempt id is deterministic: peek by running the id generator
    // the same way admission will.
    // The id stream is deterministic: build_pack mints the pack id, then
    // admission mints the attempt id (CAS puts mint nothing).
    let mut peek = f.store.id_gen().unwrap();
    let _pack_id = peek.context_pack();
    let expected_attempt = peek.attempt();

    let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
    engine.request_cancel(&expected_attempt).unwrap();
    let r = engine
        .run_task(&task(), Some("fake:deterministic-a"), None)
        .unwrap();
    assert_eq!(r.attempt_id, expected_attempt, "deterministic id stream");
    let (outcome, _) = r.outcome.expect("terminal");
    assert_eq!(outcome, TerminalOutcome::Cancelled);
    assert_eq!(outcome.exit_code().code(), 14);
}

#[test]
fn inv22__store_persist__ledger_is_append_only_by_trigger_and_survives_reopen() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);
    let attempt_id = {
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        engine
            .run_task(&task(), Some("fake:deterministic-a"), None)
            .unwrap()
            .attempt_id
    };

    // Append-only by trigger: UPDATE and DELETE raise inside SQLite itself.
    let update = f
        .store
        .raw()
        .execute("UPDATE receipts SET kind='forged' WHERE 1=1", []);
    assert!(update.unwrap_err().to_string().contains("append-only"));
    let delete = f.store.raw().execute("DELETE FROM receipts", []);
    assert!(delete.unwrap_err().to_string().contains("append-only"));
    let ev_update = f
        .store
        .raw()
        .execute("UPDATE events SET body='{}' WHERE 1=1", []);
    assert!(ev_update.unwrap_err().to_string().contains("append-only"));

    // Reopen: state resolves from the durable ledger, ids never collide.
    let count_before = f.store.receipt_count().unwrap();
    let issued_before = f.store.id_gen().unwrap().issued();
    drop(f.store);
    let store2 = Store::open(&f.ws.ledger_db()).unwrap();
    assert_eq!(store2.receipt_count().unwrap(), count_before);
    let log = store2.load_full_log().unwrap();
    assert_eq!(
        rein_core::state::resolve_state(&log, &attempt_id).unwrap(),
        AttemptState::Closed
    );
    assert_eq!(store2.id_gen().unwrap().issued(), issued_before);
}

#[test]
fn m1__strict_replay_catches_cas_tampering() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);
    let report = {
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        engine
            .run_task(&task(), Some("fake:deterministic-a"), None)
            .unwrap()
    };
    let hands = rein_runtime::hands::HandRegistry::with_fixtures();

    // Clean replay first.
    let clean =
        rein_runtime::replay::replay_attempt(&f.ws, &f.store, &hands, &report.attempt_id).unwrap();
    assert!(clean.matches(), "{:?}", clean.differences);
    assert_eq!(clean.artifacts_reverified, 2);

    // Tamper with the committed object bytes on disk.
    let (_name, digest) = &report.artifacts[0];
    let cas = rein_runtime::cas::Cas::new(f.ws.objects());
    let path = cas.path_of(digest);
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o644);
        std::fs::set_permissions(&path, perms).unwrap();
    }
    std::fs::write(&path, b"tampered bytes").unwrap();

    let tampered =
        rein_runtime::replay::replay_attempt(&f.ws, &f.store, &hands, &report.attempt_id).unwrap();
    assert!(
        !tampered.matches(),
        "tampering must surface as a replay difference"
    );
}

#[test]
fn inv27__workspace_secretbroker_open__refuses_config_root_inside_workspace() {
    let ws_dir = tempfile::tempdir().unwrap();
    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:inv27").unwrap(),
        t("2026-08-19T00:00:00Z"),
    )
    .unwrap();

    // A config root inside the workspace tree is refused outright: credentials
    // must never resolve from a directory written by model output.
    let inside = ws.root.join("model-writable-config");
    std::fs::create_dir_all(&inside).unwrap();
    let refused = SecretBroker::open(&inside, &ws.root);
    assert!(
        matches!(
            refused,
            Err(rein_runtime::workspace::WorkspaceError::ConfigInsideWorkspace { .. })
        ),
        "invariant 27"
    );

    // A disjoint config root is fine — and resolves refs.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secrets.toml"), "k = \"v\"\n").unwrap();
    let broker = SecretBroker::open(outside.path(), &ws.root).unwrap();
    assert_eq!(
        broker.resolve(&rein_core::ids::SecretRefId::parse("secret-ref:k").unwrap()),
        Some("v")
    );
    let _keep = ws_dir; // keep tempdirs alive to the end
    let _keep2 = outside;
}
