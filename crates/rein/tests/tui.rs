//! M4 acceptance (§13): headless render tests on buffers — a pattern
//! earned in a sibling TUI. The Live-Attempt panel shows *disagreeing axes* for exit0-empty
//! (process: exit 0 / artifact: absent / outcome: artifact_invalid), and
//! invariants 31–32 land: absence is stated, never blank; every disabled
//! action explains itself and every status names its receipt.
#![allow(non_snake_case)]
#![allow(clippy::field_reassign_with_default)]

use ratatui::backend::TestBackend;
use ratatui::Terminal;
use rein_core::context_pack::{OutputContract, PitMode, RequiredArtifact};
use rein_core::entities::{Epoch, Mission, Plan, PlanNode, TaskVersion};
use rein_core::ids::{AttemptId, MissionRef, PlanRef, TaskRef, ValidatorRef, WorkspaceRef};
use rein_core::time::Timestamp;
use rein_runtime::clock::FixedClock;
use rein_runtime::engine::Engine;
use rein_runtime::store::Store;
use rein_runtime::workspace::{SecretBroker, Workspace};

#[path = "../src/tui/mod.rs"]
#[allow(dead_code, unused_imports)]
mod tui;

use tui::data::{
    attempt_detail, compare_attempts, load_snapshot, publish_action_state, ActionState, DiffClass,
};
use tui::{render_app, App, Screen, KEYMAP};

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

struct Fx {
    ws: Workspace,
    store: Store,
    _config: tempfile::TempDir,
    _ws_dir: tempfile::TempDir,
}

fn fixture_with_runs(hands: &[&str]) -> (Fx, Vec<AttemptId>) {
    let ws_dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:tui").unwrap(),
        t("2026-08-19T00:00:00Z"),
    )
    .unwrap();
    let mut store = Store::open(&ws.ledger_db()).unwrap();
    store
        .put_mission(&Mission {
            mission_ref: MissionRef::parse("mission:tui").unwrap(),
            objective: "render".into(),
            closure_conditions: vec![],
            created_at: t("2026-08-19T00:00:00Z"),
        })
        .unwrap();
    store
        .put_epoch(&Epoch {
            epoch_ref: rein_core::ids::EpochRef::parse("epoch:tui").unwrap(),
            mission_ref: MissionRef::parse("mission:tui").unwrap(),
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
        plan_ref: PlanRef::parse("plan:tui@1").unwrap(),
        nodes: vec![PlanNode {
            task_ref: TaskRef::parse("task:tui@1").unwrap(),
            depends_on: vec![],
        }],
    };
    store.put_plan(&plan).unwrap();
    store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:tui@1").unwrap(),
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

    let clock = FixedClock::new(t("2026-08-19T08:00:00Z"));
    let broker = SecretBroker::open(config.path(), &ws.root).unwrap();
    let mut ids = Vec::new();
    {
        let mut engine = Engine::new(&ws, &mut store, &clock, broker);
        for (i, hand) in hands.iter().enumerate() {
            let report = if i == 0 {
                engine
                    .run_task(&TaskRef::parse("task:tui@1").unwrap(), Some(hand), None)
                    .unwrap()
            } else {
                engine.retry(&ids[0], Some(hand)).unwrap()
            };
            ids.push(report.attempt_id);
        }
    }
    (
        Fx {
            ws,
            store,
            _config: config,
            _ws_dir: ws_dir,
        },
        ids,
    )
}

fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer().clone();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer.get(x, y).symbol());
        }
        out.push('\n');
    }
    out
}

/// The M4 acceptance row: exit0-empty renders with its axes DISAGREEING on
/// one screen — exit 0, artifacts absent, outcome artifact_invalid.
#[test]
fn m4_acceptance__live_attempt_shows_disagreeing_axes_for_exit0_empty() {
    let (f, ids) = fixture_with_runs(&["fake:exit0-empty"]);
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let detail = attempt_detail(&f.store, &ids[0]).unwrap();
    let action = publish_action_state(&detail);

    let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
    let mut app = App::default();
    app.screen = Screen::LiveAttempt;
    terminal
        .draw(|fr| render_app(fr, &app, &snap, Some(&detail), Some(&action), None))
        .unwrap();
    let text = buffer_text(&terminal);

    assert!(
        text.contains("last child exit: 0"),
        "process axis shows exit 0"
    );
    assert!(text.contains("missing: 2"), "artifact axis shows absence");
    assert!(
        text.contains("ArtifactInvalid"),
        "outcome axis disagrees with the exit"
    );
    assert!(
        text.contains("Process exit is evidence only"),
        "the organizing sentence stays on-screen"
    );
    // Six vocabularies visible as separate fields.
    for label in [
        "child process",
        "harness run",
        "artifact",
        "attempt outcome",
        "task satisfaction",
        "research acceptance",
        "system admission",
    ] {
        assert!(text.contains(label), "panel field `{label}`");
    }
}

/// Invariant 31 — absence is stated, never blank; an empty panel and a failed
/// one mean opposite things. Symbol: the screens' stated-absence rows.
#[test]
fn inv31__screens__absence_is_stated_never_blank() {
    let (f, ids) = fixture_with_runs(&["fake:deterministic-a"]);
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let detail = attempt_detail(&f.store, &ids[0]).unwrap();

    // External axes: recorded state or "not adjudicated here" — words.
    let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
    let mut app = App::default();
    app.screen = Screen::LiveAttempt;
    terminal
        .draw(|fr| render_app(fr, &app, &snap, Some(&detail), None, None))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("not adjudicated here"));

    // Recovery with an empty queue: a statement, not a blank.
    app.screen = Screen::Recovery;
    terminal
        .draw(|fr| render_app(fr, &app, &snap, None, None, None))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("queue empty"));
    assert!(text.contains("opposite things"));

    // Compare with no pair: stated.
    app.screen = Screen::Compare;
    terminal
        .draw(|fr| render_app(fr, &app, &snap, None, None, None))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("no pair selected"));
}

/// Invariant 32 — every disabled action explains itself; every status names
/// the receipt it derives from. Symbol: `tui::data::publish_action_state`.
#[test]
fn inv32__action_gating__disabled_actions_explain_and_statuses_name_receipts() {
    let (f, ids) = fixture_with_runs(&["fake:exit0-empty"]);
    let detail = attempt_detail(&f.store, &ids[0]).unwrap();
    match publish_action_state(&detail) {
        ActionState::Disabled { explain } => {
            assert!(explain.contains("ArtifactInvalid"), "{explain}");
            assert!(
                explain.contains("required_artifact_absent"),
                "the reason is named"
            );
        }
        ActionState::Enabled => panic!("a failed attempt must not be publishable"),
    }

    // Statuses name receipts: mission control's outcome column carries
    // `per rcpt_…`.
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(140, 34)).unwrap();
    let app = App::default();
    terminal
        .draw(|fr| render_app(fr, &app, &snap, None, None, None))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("per rcpt_"), "status names its receipt");

    // A successful attempt: publish enabled.
    let (f2, ids2) = fixture_with_runs(&["fake:deterministic-a"]);
    let detail2 = attempt_detail(&f2.store, &ids2[0]).unwrap();
    assert!(matches!(
        publish_action_state(&detail2),
        ActionState::Enabled
    ));
}

/// The recovery console has no force-success by construction: the complete
/// keymap contains nothing that could spell it, and authority-changing keys
/// go through a confirm popup, never a single keystroke.
#[test]
fn m4__recovery_console__no_force_success_keybinding_and_confirm_required() {
    for (key, action) in KEYMAP {
        let lower = action.to_lowercase();
        assert!(
            !(lower.contains("force") && lower.contains("success")),
            "keymap entry {key} spells force-success: {action}"
        );
    }

    // Pressing a recovery action key opens a confirm popup; the action fires
    // only on `y`.
    let (f, _ids) = fixture_with_runs(&["fake:unknown-after-disconnect"]);
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    assert_eq!(snap.queue.len(), 1);
    let mut app = App::default();
    app.screen = Screen::Recovery;
    let fired = app.handle_key(crossterm::event::KeyCode::Char('u'), &snap);
    assert!(fired.is_none(), "no action on the first keystroke");
    assert!(matches!(app.popup, Some(tui::Popup::Confirm { .. })));
    let fired = app.handle_key(crossterm::event::KeyCode::Char('y'), &snap);
    assert!(fired.is_some(), "confirmed action fires");
    assert_eq!(fired.unwrap().action, "close-unknown");

    // Esc unwinds the popup instead of firing anything.
    let mut app2 = App::default();
    app2.screen = Screen::Recovery;
    app2.handle_key(crossterm::event::KeyCode::Char('m'), &snap);
    assert!(app2.popup.is_some());
    app2.handle_key(crossterm::event::KeyCode::Esc, &snap);
    assert!(app2.popup.is_none(), "Esc unwinds popup first");
}

/// Compare renders both attempts with all six difference classes available
/// and digests classified as output differences.
#[test]
fn m4__compare_screen__six_classes_and_digest_rows() {
    assert_eq!(DiffClass::ALL.len(), 6, "the six classes, complete");

    let (f, ids) = fixture_with_runs(&["fake:deterministic-a", "fake:deterministic-b"]);
    let report = compare_attempts(&f.store, &ids[0], &ids[1]).unwrap();
    // Same pack → same context hash row; artifact rows classed `output`.
    let ctx_row = report
        .rows
        .iter()
        .find(|r| r.subject == "context_hash")
        .unwrap();
    assert_eq!(ctx_row.a, ctx_row.b, "byte-identical pack across the pair");
    assert!(report
        .rows
        .iter()
        .any(|r| r.subject.starts_with("artifact ") && r.class == DiffClass::Output));

    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(160, 30)).unwrap();
    let mut app = App::default();
    app.screen = Screen::Compare;
    terminal
        .draw(|fr| render_app(fr, &app, &snap, None, None, Some(&report)))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("6 classes, complete"));
    assert!(text.contains("nonsemantic-receipt"));
    assert!(text.contains("expected-environmental"));
}

/// Enter-to-results: from an attempt row straight to the committed content,
/// read back through the CAS; Esc unwinds the viewer before selection.
#[test]
fn results_viewer_shows_committed_artifacts_inline() {
    let (f, ids) = fixture_with_runs(&["fake:deterministic-a"]);
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let rv = tui::data::attempt_results(&f.ws, &f.store, &ids[0]).unwrap();
    assert!(!rv.artifacts.is_empty(), "the run committed artifacts");
    let first = rv.artifacts[0].name.clone();
    assert!(
        !rv.artifacts[0].preview.is_empty(),
        "content preview is loaded"
    );

    let mut app = App::default();
    app.results = Some(rv);
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|fr| render_app(fr, &app, &snap, None, None, None))
        .unwrap();
    let text = buffer_text(&terminal);
    assert!(text.contains("results —"), "viewer title present");
    assert!(text.contains(&first), "artifact name listed");
    assert!(text.contains("read back through the CAS"));
    assert!(text.contains("sha256:"), "the digest is shown");

    // Esc unwinds: popup → results → selection → quit.
    app.selected = 3;
    app.unwind();
    assert!(app.results.is_none(), "results closed first");
    assert_eq!(app.selected, 3, "selection untouched by that unwind");
    assert!(!app.quit);
}

/// j/k stay inside the list — the cursor can never point past the data.
#[test]
fn selection_clamps_to_visible_rows() {
    let (f, _ids) = fixture_with_runs(&["fake:deterministic-a"]);
    let snap = load_snapshot(&f.ws, &f.store).unwrap();
    let mut app = App::default();
    for _ in 0..50 {
        app.handle_key(crossterm::event::KeyCode::Char('j'), &snap);
    }
    assert_eq!(
        app.selected,
        snap.attempts.len() - 1,
        "clamped to the last attempt"
    );
}
