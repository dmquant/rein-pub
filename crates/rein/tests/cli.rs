//! CLI integration: the §9 output contract and wait-assertion semantics,
//! exercised through the real binary.

use std::path::Path;
use std::process::Command;

struct Cli {
    ws: tempfile::TempDir,
    config: tempfile::TempDir,
}

impl Cli {
    fn new() -> Self {
        Self {
            ws: tempfile::tempdir().unwrap(),
            config: tempfile::tempdir().unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> (i32, serde_json::Value, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_rein"))
            .current_dir(self.ws.path())
            .args(["--config-root"])
            .arg(self.config.path())
            .args(["--output", "json"])
            .args(args)
            .output()
            .expect("binary runs");
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let json = serde_json::from_str(&stdout).unwrap_or(serde_json::Value::Null);
        (out.status.code().unwrap_or(-1), json, stdout)
    }
}

fn write(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
}

fn setup(cli: &Cli) {
    assert_eq!(cli.run(&["init"]).0, 0);
    assert_eq!(
        cli.run(&["mission", "create", "etf", "--objective", "test"])
            .0,
        0
    );
    assert_eq!(
        cli.run(&[
            "epoch",
            "open",
            "e1",
            "--mission",
            "etf",
            "--source-cutoff",
            "2026-08-18T00:00:00Z",
            "--seal",
        ])
        .0,
        0
    );
    write(
        cli.ws.path(),
        "plan.yaml",
        "plan_ref: plan:p@1\nnodes:\n  - task_ref: task:val@1\n    task_type: fixture\n",
    );
    assert_eq!(cli.run(&["plan", "apply", "-f", "plan.yaml"]).0, 0);
}

#[test]
fn envelope_ok_is_exactly_exit_zero_and_schema_is_stable() {
    let cli = Cli::new();
    let (code, json, _) = cli.run(&["init"]);
    assert_eq!(code, 0);
    assert_eq!(json["schema"], "rein.cli-result/v1");
    assert_eq!(json["ok"], true);

    // A refused command: ok false, structured error, nonzero exit.
    let (code2, json2, _) = cli.run(&["init"]);
    assert_ne!(code2, 0);
    assert_eq!(json2["ok"], false);
    assert!(!json2["errors"].as_array().unwrap().is_empty());
}

#[test]
fn run_with_wait_require_task_satisfied_certifies_via_receipts() {
    let cli = Cli::new();
    setup(&cli);
    let (code, json, _) = cli.run(&[
        "run",
        "task:val@1",
        "--hand",
        "fake:deterministic-a",
        "--wait",
        "--require",
        "task-satisfied",
    ]);
    assert_eq!(code, 0, "exit 0 ⇔ a verified TaskSelectionReceipt exists");
    assert_eq!(json["data"]["task_satisfied"], true);
    assert_eq!(json["data"]["outcome"]["terminal"], "success");
}

#[test]
fn exit0_empty_maps_to_artifact_invalid_exit_12() {
    let cli = Cli::new();
    setup(&cli);
    let (code, json, _) = cli.run(&[
        "run",
        "task:val@1",
        "--hand",
        "fake:exit0-empty",
        "--wait",
        "--require",
        "attempt-terminal",
    ]);
    assert_eq!(code, 12, "artifact_invalid → 12 (§9 total mapping)");
    assert_eq!(json["data"]["outcome"]["terminal"], "artifact_invalid");
    // Child exit 0 is evidence in the record, not the verdict (invariant 2).
}

#[test]
fn secret_leak_bare_run_exits_10_and_13_only_under_validation_assertion() {
    let cli = Cli::new();
    // Config root carries the fixture secret so secret-scan has teeth.
    std::fs::write(
        cli.config.path().join("secrets.toml"),
        format!("fixture = \"{}\"\n", rein_core::fakes::FIXTURE_SECRET_VALUE),
    )
    .unwrap();
    setup(&cli);

    // O1's accepted resolution: bare run (attempt-terminal) → failure → 10.
    let (code, json, _) = cli.run(&[
        "run",
        "task:val@1",
        "--hand",
        "fake:secret-leak",
        "--wait",
        "--require",
        "attempt-terminal",
    ]);
    assert_eq!(code, 10);
    assert_eq!(json["data"]["outcome"]["terminal"], "failure");
    assert_eq!(
        json["data"]["outcome"]["reason"],
        "artifact_quarantined_secret"
    );

    // Under --require validation-passed, the wait-assertion code 13 applies.
    let (code13, _, _) = cli.run(&[
        "run",
        "task:val@1",
        "--hand",
        "fake:secret-leak",
        "--wait",
        "--require",
        "validation-passed",
    ]);
    assert_eq!(
        code13, 13,
        "validation-passed unmet is a wait-assertion failure"
    );
}

#[test]
fn without_wait_exit_zero_asserts_nothing_about_outcome() {
    let cli = Cli::new();
    setup(&cli);
    let (code, json, _) = cli.run(&["run", "task:val@1", "--hand", "fake:exit0-empty"]);
    assert_eq!(code, 0, "admitted and ran");
    assert_eq!(json["data"]["outcome"]["terminal"], "artifact_invalid");
    let warnings = json["warnings"].as_array().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().unwrap().contains("asserts nothing")),
        "the non-assertion is stated, not implied"
    );
}

#[test]
fn replay_strict_is_clean_after_a_deterministic_run() {
    let cli = Cli::new();
    setup(&cli);
    let (_, json, _) = cli.run(&["run", "task:val@1", "--hand", "fake:deterministic-a"]);
    let attempt = json["data"]["attempt_id"].as_str().unwrap().to_string();
    let (code, rjson, _) = cli.run(&["replay", "attempt", &attempt, "--strict"]);
    assert_eq!(code, 0);
    assert_eq!(rjson["data"]["differences"].as_array().unwrap().len(), 0);
}

#[test]
fn doctor_and_status_are_green_on_a_fresh_workspace() {
    let cli = Cli::new();
    setup(&cli);
    assert_eq!(cli.run(&["doctor"]).0, 0);
    let (code, json, _) = cli.run(&["status"]);
    assert_eq!(code, 0);
    assert_eq!(json["data"]["missions"], 1);
}

#[test]
fn eval_answers_runs_real_attempts_and_resumes() {
    let cli = Cli::new();
    setup(&cli);
    // Two public-FinanceGym-shaped questions (task_id, no expectations).
    write(
        cli.ws.path(),
        "qs.jsonl",
        r#"{"task_id":"q-aa","question":"What moved rates?","cutoff":"2025-06-05"}
{"task_id":"q-bb","question":"Who supplies the wafers?","cutoff":"2025-08-05"}
"#,
    );
    let (code, json, _) = cli.run(&[
        "eval",
        "answers",
        "-f",
        "qs.jsonl",
        "--hand",
        "finance:ops",
        "--out",
        "a.json",
    ]);
    assert_eq!(code, 0);
    assert_eq!(json["data"]["answered"], 2);
    assert_eq!(json["data"]["resumed"], 0);
    let answers: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cli.ws.path().join("a.json")).unwrap())
            .unwrap();
    assert!(answers["q-aa"]
        .as_str()
        .unwrap()
        .contains("What moved rates?"));

    // Rerun: satisfaction rides selection receipts — everything resumes, no
    // new attempts.
    let (_, before, _) = cli.run(&["attempt", "list"]);
    let n_before = before["data"].as_array().unwrap().len();
    let (code2, json2, _) = cli.run(&[
        "eval",
        "answers",
        "-f",
        "qs.jsonl",
        "--hand",
        "finance:ops",
        "--out",
        "a.json",
    ]);
    assert_eq!(code2, 0);
    assert_eq!(json2["data"]["resumed"], 2);
    assert_eq!(json2["data"]["answered"], 0);
    let (_, after, _) = cli.run(&["attempt", "list"]);
    assert_eq!(after["data"].as_array().unwrap().len(), n_before);
}

/// `eval grade`: an external judge emits tiers 0–4 into a resumable grades
/// file; grading is file → file and appends nothing to any ledger.
#[test]
fn eval_grade_judges_answers_and_resumes() {
    let cli = Cli::new();
    write(
        cli.ws.path(),
        "qs.jsonl",
        concat!(
            r#"{"task_id":"g1","question":"What is 2+2 in USD millions?","cutoff":"2026-01-01"}"#,
            "\n",
            r#"{"task_id":"g2","question":"Second question","cutoff":"2026-01-01"}"#,
            "\n",
        ),
    );
    write(cli.ws.path(), "a.json", r#"{"g1":"It is 4 USD millions."}"#);

    // A stub judge speaking the agy envelope.
    let judge = cli.ws.path().join("judge.sh");
    std::fs::write(
        &judge,
        "#!/bin/sh\necho '{\"status\":\"SUCCESS\",\"response\":\"{\\\"tier\\\": 3, \\\"reason\\\": \\\"close enough\\\"}\"}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&judge, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let judge_arg = judge.display().to_string();

    let args = [
        "eval",
        "grade",
        "-f",
        "qs.jsonl",
        "--answers",
        "a.json",
        "--out",
        "grades.json",
        "--judge",
        judge_arg.as_str(),
        "--judge-model",
        "stub",
    ];
    let (code, json, _) = cli.run(&args);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["data"]["graded_now"], 1);
    assert_eq!(json["data"]["missing_answers"], 1);
    let grades: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(cli.ws.path().join("grades.json")).unwrap())
            .unwrap();
    assert_eq!(grades["g1"], 3);

    // Rerun: prior grades are prior work — nothing is re-judged.
    let (code2, json2, _) = cli.run(&args);
    assert_eq!(code2, 0);
    assert_eq!(json2["data"]["graded_now"], 0);
    assert_eq!(json2["data"]["already_graded"], 1);
}
