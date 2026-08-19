//! M3 (§13): recovery + evidence. The bundle verifies deterministically and
//! catches tampering; the recovery queue types its anomalies with tolerance
//! (invariant 25); resume-commit re-enters the same attempt under a fresh
//! fence generation as a new HarnessRun — with the dead hand replaceable
//! (C2: execution binding).
#![allow(non_snake_case)]

use rein_core::context_pack::{OutputContract, PitMode, RequiredArtifact};
use rein_core::entities::{Epoch, Mission, Plan, PlanNode, TaskVersion};
use rein_core::ids::{MissionRef, PlanRef, TaskRef, ValidatorRef, WorkspaceRef};
use rein_core::outcome::TerminalOutcome;
use rein_core::state::AttemptState;
use rein_core::time::Timestamp;
use rein_runtime::clock::FixedClock;
use rein_runtime::engine::Engine;
use rein_runtime::evidence::{bundle_attempt, verify_bundle};
use rein_runtime::recovery_queue::{recovery_queue, DEFAULT_STALE_AFTER_MS};
use rein_runtime::store::Store;
use rein_runtime::workspace::{SecretBroker, Workspace};

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

struct Fx {
    ws: Workspace,
    store: Store,
    config: tempfile::TempDir,
    _ws_dir: tempfile::TempDir,
}

fn fixture() -> Fx {
    let ws_dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:m3").unwrap(),
        t("2026-08-19T00:00:00Z"),
    )
    .unwrap();
    let mut store = Store::open(&ws.ledger_db()).unwrap();
    store
        .put_mission(&Mission {
            mission_ref: MissionRef::parse("mission:m3").unwrap(),
            objective: "recovery + evidence".into(),
            closure_conditions: vec![],
            created_at: t("2026-08-19T00:00:00Z"),
        })
        .unwrap();
    store
        .put_epoch(&Epoch {
            epoch_ref: rein_core::ids::EpochRef::parse("epoch:m3").unwrap(),
            mission_ref: MissionRef::parse("mission:m3").unwrap(),
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
        plan_ref: PlanRef::parse("plan:m3@1").unwrap(),
        nodes: vec![PlanNode {
            task_ref: TaskRef::parse("task:m3@1").unwrap(),
            depends_on: vec![],
        }],
    };
    store.put_plan(&plan).unwrap();
    store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:m3@1").unwrap(),
            plan_ref: plan.plan_ref.clone(),
            task_type: "fixture".into(),
            output_contract: OutputContract {
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
            },
            satisfaction_criteria: vec![],
            inputs: vec![],
            universe: vec![],
        })
        .unwrap();
    Fx {
        ws,
        store,
        config,
        _ws_dir: ws_dir,
    }
}

fn broker(f: &Fx) -> SecretBroker {
    SecretBroker::open(f.config.path(), &f.ws.root).unwrap()
}

fn task() -> TaskRef {
    TaskRef::parse("task:m3@1").unwrap()
}

#[test]
fn m3__bundle_verifies_deterministically_and_catches_tampering() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let report = {
        let b = broker(&f);
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        engine
            .run_task(&task(), Some("fake:deterministic-a"), None)
            .unwrap()
    };
    let out = f.ws.tmp().join("m3.evidence.tar.zst");
    let bundle = bundle_attempt(&f.ws, &f.store, &report.attempt_id, &out).unwrap();

    let clean = verify_bundle(&bundle).unwrap();
    assert!(clean.ok(), "problems: {:?}", clean.problems);
    assert!(clean.files_checked >= 6);
    assert!(clean.receipts_replayed >= 10);
    assert!(clean.events_checked >= 5);

    // Tamper inside the tarball's staging path: repack with one artifact byte
    // flipped — cheapest honest tamper: unpack, flip, verify the DIRECTORY.
    let unpack = f.ws.tmp().join("m3-tamper");
    std::fs::create_dir_all(&unpack).unwrap();
    let file = std::fs::File::open(&bundle).unwrap();
    let zr = zstd::stream::read::Decoder::new(file).unwrap();
    tar::Archive::new(zr).unpack(&unpack).unwrap();
    // Flip bytes in one committed artifact file.
    let artifacts_dir = unpack.join("evidence").join("artifacts");
    let victim = std::fs::read_dir(&artifacts_dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(&victim, b"tampered").unwrap();
    let tampered = verify_bundle(&unpack).unwrap();
    assert!(
        !tampered.ok(),
        "a flipped artifact byte must surface as a digest mismatch"
    );
    assert!(tampered
        .problems
        .iter()
        .any(|p| p.contains("digest") || p.contains("absent")));
}

#[test]
fn inv25__recovery_queue__stale_check_tolerates_the_boundarys_own_latency() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    // A run that disconnects sits in recovery_pending (typed, always queued).
    let report = {
        let b = broker(&f);
        let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);
        engine
            .run_task(&task(), Some("fake:unknown-after-disconnect"), None)
            .unwrap()
    };
    assert_eq!(report.final_state, AttemptState::RecoveryPending);

    // Immediately after: within tolerance nothing screams "stale" — the one
    // queued entry is the *typed* recovery_pending anomaly, not a stale-run
    // false positive (a warning that fires on every run burns credibility).
    let now = t("2026-08-19T08:00:01Z");
    let queue = recovery_queue(&f.store, now, DEFAULT_STALE_AFTER_MS).unwrap();
    assert_eq!(queue.len(), 1);
    assert!(matches!(
        queue[0].anomaly,
        rein_core::state::AnomalyKind::UnknownAfterDisconnect
    ));
    assert_eq!(queue[0].actions.len(), 3, "exactly three safe actions");

    // Far past the tolerance the same entry is still typed the same way —
    // and a fresh healthy attempt never enters the queue at all.
    let later = t("2026-08-19T09:00:00Z");
    let queue2 = recovery_queue(&f.store, later, DEFAULT_STALE_AFTER_MS).unwrap();
    assert_eq!(queue2.len(), 1);
}

#[test]
fn m3__resume_commit_reenters_same_attempt_new_generation_new_run() {
    let mut f = fixture();
    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let b = broker(&f);
    let mut engine = Engine::new(&f.ws, &mut f.store, &clock, b);

    let dead = engine
        .run_task(&task(), Some("fake:unknown-after-disconnect"), None)
        .unwrap();
    assert_eq!(dead.final_state, AttemptState::RecoveryPending);
    assert!(dead.outcome.is_none(), "no inferred verdict (invariant 5)");

    // Resume: same attempt, fence generation 2, the dead hand replaced (C2 —
    // execution binding), pipeline completes to success.
    let resumed = engine
        .resume_attempt(&dead.attempt_id, Some("fake:deterministic-a"))
        .unwrap();
    assert_eq!(resumed.attempt_id, dead.attempt_id, "SAME attempt");
    assert_eq!(
        resumed.outcome.as_ref().map(|(o, _)| *o),
        Some(TerminalOutcome::Success)
    );
    assert_eq!(resumed.final_state, AttemptState::Closed);

    // Two HarnessRuns on one attempt (§3: multiple runs only for recovery),
    // and the current fence generation is 2 — the old generation may not
    // commit (invariant 24).
    let runs = f.store.runs_for_attempt(&dead.attempt_id).unwrap();
    assert_eq!(runs.len(), 2);
    let log = f.store.load_attempt_log(&dead.attempt_id).unwrap();
    assert_eq!(
        rein_core::fence::current_generation(&log, &dead.attempt_id).unwrap(),
        2
    );
    assert!(rein_core::fence::guard_commit(&log, &dead.attempt_id, 1).is_err());
}
