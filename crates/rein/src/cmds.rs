//! Command implementations. Every command returns a [`CmdOutput`] whose exit
//! code follows §9's closed vocabulary; judgments come from receipts, never
//! from this file.

use crate::out::{j, kv, s, CmdOutput};
use rein_core::canon::Sha256Digest;
use rein_core::context_pack::{Budget, OutputContract, PitMode, RequiredArtifact};
use rein_core::entities::{Epoch, Mission, Plan, TaskVersion};
use rein_core::ids::{AttemptId, EpochRef, MissionRef, PlanRef, RunId, TaskRef, ValidatorRef};
use rein_core::outcome::ExitCode;
use rein_core::pins::ProviderPin;
use rein_core::receipts::{CommitVerdict, ReceiptBody, ValidatorVerdict};
use rein_core::selection;
use rein_core::state::resolve_state;
use rein_core::time::Timestamp;
use rein_runtime::clock::{Clock, SystemClock};
use rein_runtime::engine::{Engine, EngineError, ExecutionReport};
use rein_runtime::providers::ProvidersLock;
use rein_runtime::store::{Store, StoreError};
use rein_runtime::workspace::{SecretBroker, Workspace, WorkspaceError};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct CliError {
    pub exit: ExitCode,
    pub message: String,
}

impl CliError {
    fn new(exit: ExitCode, message: impl Into<String>) -> Self {
        Self {
            exit,
            message: message.into(),
        }
    }
}

impl From<EngineError> for CliError {
    fn from(e: EngineError) -> Self {
        let exit = match &e {
            EngineError::Store(StoreError::NotFound { .. }) => ExitCode::NotFound,
            EngineError::Store(StoreError::Immutable { .. }) => ExitCode::ConflictStaleFence,
            EngineError::Admit(_) => ExitCode::ConflictStaleFence,
            EngineError::Fence(_) => ExitCode::ConflictStaleFence,
            EngineError::Hand(rein_runtime::hands::HandError::Unknown(_)) => ExitCode::NotFound,
            EngineError::EpochUnsealed(_) => ExitCode::Usage,
            EngineError::Classify(_) => ExitCode::Unknown,
            EngineError::Pack(_) => ExitCode::Usage,
            _ => ExitCode::Internal,
        };
        Self::new(exit, e.to_string())
    }
}

impl From<StoreError> for CliError {
    fn from(e: StoreError) -> Self {
        let exit = match &e {
            StoreError::NotFound { .. } => ExitCode::NotFound,
            StoreError::Immutable { .. } => ExitCode::ConflictStaleFence,
            _ => ExitCode::Internal,
        };
        Self::new(exit, e.to_string())
    }
}

impl From<WorkspaceError> for CliError {
    fn from(e: WorkspaceError) -> Self {
        let exit = match &e {
            WorkspaceError::NotFound(_) => ExitCode::NotFound,
            WorkspaceError::AlreadyExists(_) => ExitCode::ConflictStaleFence,
            WorkspaceError::ConfigInsideWorkspace { .. } => ExitCode::PolicyDenied,
            _ => ExitCode::Internal,
        };
        Self::new(exit, e.to_string())
    }
}

pub type CmdResult = Result<CmdOutput, CliError>;

pub struct Ctx {
    pub start_dir: PathBuf,
    pub config_root: PathBuf,
}

impl Ctx {
    fn open(&self) -> Result<(Workspace, Store), CliError> {
        let ws = Workspace::discover(&self.start_dir)?;
        let store = Store::open(&ws.ledger_db())?;
        Ok((ws, store))
    }

    fn broker(&self, ws: &Workspace) -> Result<SecretBroker, CliError> {
        Ok(SecretBroker::open(&self.config_root, &ws.root)?)
    }

    fn with_engine<F>(&self, f: F) -> CmdResult
    where
        F: FnOnce(&mut Engine<'_>) -> CmdResult,
    {
        let (ws, mut store) = self.open()?;
        let broker = self.broker(&ws)?;
        let clock = SystemClock;
        let mut engine = Engine::new(&ws, &mut store, &clock, broker);
        f(&mut engine)
    }
}

// ---- ref normalization -----------------------------------------------------

fn norm(prefix: &str, v: &str) -> String {
    if v.starts_with(prefix) {
        v.to_string()
    } else {
        format!("{prefix}{v}")
    }
}

pub fn mission_ref(v: &str) -> Result<MissionRef, CliError> {
    MissionRef::parse(&norm("mission:", v))
        .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

pub fn epoch_ref(v: &str) -> Result<EpochRef, CliError> {
    EpochRef::parse(&norm("epoch:", v)).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

pub fn plan_ref(v: &str) -> Result<PlanRef, CliError> {
    PlanRef::parse(&norm("plan:", v)).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

pub fn task_ref(v: &str) -> Result<TaskRef, CliError> {
    TaskRef::parse(&norm("task:", v)).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

pub fn attempt_id(v: &str) -> Result<AttemptId, CliError> {
    AttemptId::parse(v).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

fn ts(v: &str) -> Result<Timestamp, CliError> {
    Timestamp::parse(v).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))
}

// ---- init / status / doctor ------------------------------------------------

pub fn init(ctx: &Ctx, workspace_ref: &str) -> CmdResult {
    let wref = rein_core::ids::WorkspaceRef::parse(&norm("ws:", workspace_ref))
        .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?;
    let ws = Workspace::init(&ctx.start_dir, wref, SystemClock.now())?;
    let store = Store::open(&ws.ledger_db())?;
    drop(store);
    ProvidersLock::new()
        .save(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    Ok(CmdOutput::ok(kv(&[
        ("workspace", s(ws.root.display().to_string())),
        ("rein_dir", s(ws.rein_dir.display().to_string())),
        ("workspace_ref", s(ws.manifest.workspace_ref.as_str())),
    ]))
    .next("rein mission create <name> --objective \"…\""))
}

pub fn status(ctx: &Ctx) -> CmdResult {
    let (ws, store) = ctx.open()?;
    let log = store.load_full_log()?;
    let attempts = store.list_attempts()?;
    let mut by_state: std::collections::BTreeMap<String, u64> = Default::default();
    for a in &attempts {
        let st = resolve_state(&log, &a.attempt_id)
            .map(|s| format!("{s:?}"))
            .unwrap_or_else(|_| "unresolvable".to_string());
        *by_state.entry(st).or_insert(0) += 1;
    }
    let missions = store.list_missions()?;
    let epochs = store.list_epochs()?;
    let tasks = store.list_tasks()?;
    Ok(CmdOutput::ok(kv(&[
        ("workspace", s(ws.root.display().to_string())),
        ("missions", json!(missions.len())),
        (
            "epochs",
            json!({"total": epochs.len(), "sealed": epochs.iter().filter(|(_, sealed)| *sealed).count()}),
        ),
        ("tasks", json!(tasks.len())),
        ("attempts", json!(attempts.len())),
        ("attempts_by_state", j(&by_state)),
        ("receipts", json!(store.receipt_count()?)),
    ])))
}

pub fn doctor(ctx: &Ctx) -> CmdResult {
    let (ws, store) = ctx.open()?;
    let mut notes = store.doctor()?;
    let broker = SecretBroker::open(&ctx.config_root, &ws.root);
    notes.push(match &broker {
        Ok(_) => format!(
            "configRoot {} is disjoint from the workspace (invariant 27)",
            ctx.config_root.display()
        ),
        Err(e) => format!("FAIL: {e}"),
    });
    let cas = rein_runtime::cas::Cas::new(ws.objects());
    let probe = cas
        .put(b"rein doctor probe")
        .and_then(|d| cas.read_verified(&d).map(|_| d));
    notes.push(match probe {
        Ok(d) => format!("cas write/read-back probe ok ({d})"),
        Err(e) => format!("FAIL: cas probe: {e}"),
    });
    for dir in [ws.tmp(), ws.logs(), ws.plans(), ws.skills(), ws.policies()] {
        if !dir.exists() {
            notes.push(format!("FAIL: missing {}", dir.display()));
        }
    }
    let failed = notes.iter().any(|n| n.starts_with("FAIL"));
    let out = CmdOutput::ok(j(&notes));
    Ok(if failed {
        out.with_exit(ExitCode::Internal)
    } else {
        out
    })
}

// ---- provider ---------------------------------------------------------------

pub fn provider_add(
    ctx: &Ctx,
    name: &str,
    coordinate: &str,
    digest: Option<&str>,
    pin_method: Option<&str>,
) -> CmdResult {
    let (ws, _) = ctx.open()?;
    let mut lock = ProvidersLock::load(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    let pin = match (digest, pin_method) {
        (Some(d), None) => ProviderPin::Digest {
            coordinate: coordinate.to_string(),
            digest: Sha256Digest::parse(d)
                .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?,
        },
        (None, Some(m)) => ProviderPin::Service {
            coordinate: coordinate.to_string(),
            pin_method: m.to_string(),
        },
        _ => {
            return Err(CliError::new(
                ExitCode::Usage,
                "a pin is exact or declares its method: pass exactly one of --digest / --pin-method (invariant 8)",
            ))
        }
    };
    lock.pins.insert(name.to_string(), pin);
    lock.generated_at = Some(SystemClock.now());
    lock.save(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    Ok(CmdOutput::ok(j(&lock)))
}

pub fn provider_list(ctx: &Ctx) -> CmdResult {
    let (ws, _) = ctx.open()?;
    let lock = ProvidersLock::load(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    Ok(CmdOutput::ok(j(&lock)))
}

pub fn provider_verify(ctx: &Ctx) -> CmdResult {
    let (ws, _) = ctx.open()?;
    let lock = ProvidersLock::load(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    match lock.verify() {
        Ok(notes) => Ok(CmdOutput::ok(j(&notes))),
        Err(e) => Ok(CmdOutput::error(
            ExitCode::ProviderUnresolved,
            e.to_string(),
        )),
    }
}

pub fn provider_lock(ctx: &Ctx) -> CmdResult {
    let (ws, _) = ctx.open()?;
    let mut lock = ProvidersLock::load(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    lock.generated_at = Some(SystemClock.now());
    lock.notes.insert(
        "determinism".to_string(),
        "lock generation is deterministic except this one labeled timestamp (invariant 8)"
            .to_string(),
    );
    lock.save(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    Ok(CmdOutput::ok(j(&lock)))
}

// ---- hand -------------------------------------------------------------------

pub fn hand_list(ctx: &Ctx) -> CmdResult {
    ctx.with_engine(|engine| {
        let hands: Vec<Value> = engine
            .hands
            .selectors()
            .into_iter()
            .map(|sel| kv(&[("selector", s(sel)), ("kind", s("fixture"))]))
            .collect();
        Ok(CmdOutput::ok(Value::Array(hands)))
    })
}

pub fn hand_show(ctx: &Ctx, selector: &str) -> CmdResult {
    ctx.with_engine(|engine| {
        engine
            .hands
            .get(selector)
            .map_err(|e| CliError::new(ExitCode::NotFound, e.to_string()))?;
        Ok(CmdOutput::ok(kv(&[
            ("selector", s(selector)),
            ("kind", s("fixture")),
            (
                "note",
                s("conformance fixture (§6); real model hands land at M2 behind the same trait"),
            ),
        ])))
    })
}

/// Conformance probe: run the fixture against the M0 sample contract in a
/// scratch directory, no ledger involved.
pub fn hand_test(ctx: &Ctx, selector: &str) -> CmdResult {
    ctx.with_engine(|engine| {
        let hand = engine
            .hands
            .get(selector)
            .map_err(|e| CliError::new(ExitCode::NotFound, e.to_string()))?;
        let scratch = engine.workspace.tmp().join("hand-test");
        let inputs = scratch.join("inputs");
        let output = scratch.join("output");
        for d in [&inputs, &output] {
            std::fs::create_dir_all(d)
                .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
        }
        let contract = default_contract("valuation");
        let budget = Budget {
            max_steps: 8,
            per_step_timeout_ms: 1000,
            tokens: None,
            tool_calls: None,
        };
        let mut ids = rein_core::ids::IdGen::new();
        let request = rein_core::hand::HandRequest {
            attempt_id: AttemptId::parse("attempt_000000").expect("static"),
            run_id: ids.run(),
            fence_generation: 1,
            sequence: 0,
            idempotency_key: rein_core::idempotency::IdempotencyKey::derive(
                &TaskRef::parse("task:hand-test@1").expect("static"),
                &Sha256Digest::of_bytes(b"hand-test"),
                1,
            ),
            capability_ref: rein_core::ids::GrantId::parse("grant_hand_test").expect("static"),
            trace: ids.trace(),
            deadline: rein_core::time::LogicalMs(8_000),
            internal_retries_disabled: true,
        };
        let env = Default::default();
        let out = hand
            .run(&rein_runtime::hands::HandContext {
                request: &request,
                contract: &contract,
                budget: &budget,
                inputs_dir: &inputs,
                output_dir: &output,
                env: &env,
            })
            .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
        let staged: Vec<String> = contract
            .required_artifacts
            .iter()
            .filter(|a| output.join(&a.name).exists())
            .map(|a| a.name.clone())
            .collect();
        let _ = std::fs::remove_dir_all(&scratch);
        Ok(CmdOutput::ok(kv(&[
            ("selector", s(selector)),
            ("events", json!(out.events.len())),
            ("staged_required", j(&staged)),
            ("claimed", json!(out.claimed.len())),
        ])))
    })
}

// ---- mission ----------------------------------------------------------------

pub fn mission_create(ctx: &Ctx, name: &str, objective: &str) -> CmdResult {
    let (_, mut store) = ctx.open()?;
    let m = Mission {
        mission_ref: mission_ref(name)?,
        objective: objective.to_string(),
        closure_conditions: Vec::new(),
        created_at: SystemClock.now(),
    };
    store.put_mission(&m)?;
    Ok(CmdOutput::ok(j(&m)).next("rein epoch open --mission <ref> --source-cutoff <ts> --seal"))
}

pub fn mission_list(ctx: &Ctx) -> CmdResult {
    let (_, store) = ctx.open()?;
    let rows: Vec<Value> = store
        .list_missions()?
        .into_iter()
        .map(|(m, status)| {
            kv(&[
                ("mission_ref", s(m.mission_ref.as_str())),
                ("status", s(status)),
                ("objective", s(m.objective)),
            ])
        })
        .collect();
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn mission_show(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, store) = ctx.open()?;
    let r = mission_ref(name)?;
    let found = store
        .list_missions()?
        .into_iter()
        .find(|(m, _)| m.mission_ref == r);
    match found {
        Some((m, status)) => Ok(CmdOutput::ok(kv(&[
            ("mission", j(&m)),
            ("status", s(status)),
        ]))),
        None => Err(CliError::new(
            ExitCode::NotFound,
            format!("mission `{r}` not found"),
        )),
    }
}

pub fn mission_close(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, mut store) = ctx.open()?;
    let r = mission_ref(name)?;
    store.set_mission_status(r.as_str(), "closed")?;
    Ok(CmdOutput::ok(kv(&[
        ("mission_ref", s(r.as_str())),
        ("status", s("closed")),
    ])))
}

// ---- epoch ------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn epoch_open(
    ctx: &Ctx,
    name: &str,
    mission: &str,
    source_cutoff: &str,
    knowledge_cutoff: Option<&str>,
    pit_mode: &str,
    seal: bool,
    max_steps: u32,
    per_step_timeout_ms: u64,
) -> CmdResult {
    let (ws, mut store) = ctx.open()?;
    let source_cutoff = ts(source_cutoff)?;
    let knowledge_cutoff = knowledge_cutoff
        .map(ts)
        .transpose()?
        .unwrap_or(source_cutoff);
    let pit = match pit_mode {
        "eval" => PitMode::Eval,
        "production" => PitMode::Production,
        other => {
            return Err(CliError::new(
                ExitCode::Usage,
                format!("pit-mode is `eval` or `production`, got `{other}`"),
            ))
        }
    };
    let lock = ProvidersLock::load(&ws.providers_lock())
        .map_err(|e| CliError::new(ExitCode::Internal, e.to_string()))?;
    let e = Epoch {
        epoch_ref: epoch_ref(name)?,
        mission_ref: mission_ref(mission)?,
        source_cutoff,
        knowledge_cutoff,
        pit_mode: pit,
        provider_pins: lock.pins,
        policy_version: "policy:v1".to_string(),
        budget_envelope: Budget {
            max_steps,
            per_step_timeout_ms,
            tokens: None,
            tool_calls: None,
        },
        sealed: seal,
    };
    store.put_epoch(&e)?;
    Ok(CmdOutput::ok(j(&e)))
}

pub fn epoch_seal(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, mut store) = ctx.open()?;
    let r = epoch_ref(name)?;
    let (mut e, sealed) = store.get_epoch(r.as_str())?;
    if sealed {
        return Ok(CmdOutput::ok(j(&e)).warn("epoch already sealed"));
    }
    e.sealed = true;
    store.put_epoch(&e)?;
    Ok(CmdOutput::ok(j(&e)))
}

pub fn epoch_list(ctx: &Ctx) -> CmdResult {
    let (_, store) = ctx.open()?;
    let rows: Vec<Value> = store
        .list_epochs()?
        .into_iter()
        .map(|(e, sealed)| {
            kv(&[
                ("epoch_ref", s(e.epoch_ref.as_str())),
                ("mission", s(e.mission_ref.as_str())),
                ("source_cutoff", s(e.source_cutoff.canonical())),
                ("pit_mode", j(&e.pit_mode)),
                ("sealed", json!(sealed)),
            ])
        })
        .collect();
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn epoch_show(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, store) = ctx.open()?;
    let (e, sealed) = store.get_epoch(epoch_ref(name)?.as_str())?;
    Ok(CmdOutput::ok(kv(&[
        ("epoch", j(&e)),
        ("sealed", json!(sealed)),
    ])))
}

// ---- plan / task ------------------------------------------------------------

#[derive(serde::Deserialize)]
struct PlanFile {
    plan_ref: String,
    nodes: Vec<PlanNodeFile>,
}

#[derive(serde::Deserialize)]
struct PlanNodeFile {
    task_ref: String,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    task_type: Option<String>,
}

fn parse_plan_file(path: &str) -> Result<(Plan, Vec<(TaskRef, String)>), CliError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CliError::new(ExitCode::NotFound, format!("{path}: {e}")))?;
    let pf: PlanFile = if path.ends_with(".json") {
        serde_json::from_str(&text).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?
    } else {
        serde_yaml::from_str(&text).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?
    };
    let mut nodes = Vec::new();
    let mut types = Vec::new();
    for n in pf.nodes {
        let t = task_ref(&n.task_ref)?;
        let deps = n
            .depends_on
            .iter()
            .map(|d| task_ref(d))
            .collect::<Result<Vec<_>, _>>()?;
        types.push((
            t.clone(),
            n.task_type.unwrap_or_else(|| "research".to_string()),
        ));
        nodes.push(rein_core::entities::PlanNode {
            task_ref: t,
            depends_on: deps,
        });
    }
    Ok((
        Plan {
            plan_ref: plan_ref(&pf.plan_ref)?,
            nodes,
        },
        types,
    ))
}

pub fn default_contract(task_type: &str) -> OutputContract {
    let validators = vec![
        ValidatorRef::parse("artifact-wellformed@1").expect("static"),
        ValidatorRef::parse("secret-scan@1").expect("static"),
    ];
    match task_type {
        "valuation" => OutputContract {
            required_artifacts: vec![
                RequiredArtifact {
                    name: "valuation.json".into(),
                    media_type: "application/json".into(),
                    schema_ref: Some("schema:rein.valuation/v1".into()),
                    min_bytes: None,
                },
                RequiredArtifact {
                    name: "memo.md".into(),
                    media_type: "text/markdown".into(),
                    schema_ref: None,
                    min_bytes: Some(8),
                },
            ],
            validators,
        },
        _ => OutputContract {
            required_artifacts: vec![
                RequiredArtifact {
                    name: "dossier.md".into(),
                    media_type: "text/markdown".into(),
                    schema_ref: None,
                    min_bytes: Some(8),
                },
                RequiredArtifact {
                    name: "claims.json".into(),
                    media_type: "application/json".into(),
                    schema_ref: Some("schema:rein.claims/v1".into()),
                    min_bytes: None,
                },
            ],
            validators,
        },
    }
}

pub fn plan_apply(ctx: &Ctx, file: &str) -> CmdResult {
    let (_, mut store) = ctx.open()?;
    let (plan, types) = parse_plan_file(file)?;
    plan.validate()
        .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?;
    store.put_plan(&plan)?;
    for (t, task_type) in &types {
        let task = TaskVersion {
            task_ref: t.clone(),
            plan_ref: plan.plan_ref.clone(),
            task_type: task_type.clone(),
            output_contract: default_contract(task_type),
            satisfaction_criteria: vec!["first-valid-deterministic@1".to_string()],
        };
        store.put_task(&task)?;
    }
    Ok(CmdOutput::ok(kv(&[
        ("plan_ref", s(plan.plan_ref.as_str())),
        ("tasks", json!(types.len())),
    ])))
}

pub fn plan_validate(_ctx: &Ctx, file: &str) -> CmdResult {
    let (plan, _) = parse_plan_file(file)?;
    match plan.validate() {
        Ok(()) => Ok(CmdOutput::ok(kv(&[
            ("plan_ref", s(plan.plan_ref.as_str())),
            ("acyclic", json!(true)),
            ("nodes", json!(plan.nodes.len())),
        ]))),
        Err(e) => Ok(CmdOutput::error(ExitCode::Usage, e.to_string())),
    }
}

pub fn plan_show(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, store) = ctx.open()?;
    let plan = store.get_plan(plan_ref(name)?.as_str())?;
    Ok(CmdOutput::ok(j(&plan)))
}

pub fn task_add(
    ctx: &Ctx,
    name: &str,
    plan: &str,
    task_type: &str,
    contract_file: Option<&str>,
) -> CmdResult {
    let (_, mut store) = ctx.open()?;
    let contract = match contract_file {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| CliError::new(ExitCode::NotFound, format!("{path}: {e}")))?;
            serde_json::from_str(&text)
                .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?
        }
        None => default_contract(task_type),
    };
    let task = TaskVersion {
        task_ref: task_ref(name)?,
        plan_ref: plan_ref(plan)?,
        task_type: task_type.to_string(),
        output_contract: contract,
        satisfaction_criteria: vec!["first-valid-deterministic@1".to_string()],
    };
    store.put_task(&task)?;
    Ok(CmdOutput::ok(j(&task)))
}

pub fn task_list(ctx: &Ctx) -> CmdResult {
    let (_, store) = ctx.open()?;
    let log = store.load_full_log()?;
    let rows: Vec<Value> = store
        .list_tasks()?
        .into_iter()
        .map(|t| {
            kv(&[
                ("task_ref", s(t.task_ref.as_str())),
                ("type", s(t.task_type)),
                ("plan", s(t.plan_ref.as_str())),
                (
                    "satisfied",
                    json!(selection::task_satisfied(&log, &t.task_ref)),
                ),
            ])
        })
        .collect();
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn task_show(ctx: &Ctx, name: &str) -> CmdResult {
    let (_, store) = ctx.open()?;
    let t = store.get_task(&task_ref(name)?)?;
    Ok(CmdOutput::ok(j(&t)))
}

/// Tasks whose plan dependencies are satisfied and which are not yet
/// satisfied themselves (invariant 4: satisfaction = selection receipts).
pub fn task_ready(ctx: &Ctx) -> CmdResult {
    let (_, store) = ctx.open()?;
    let log = store.load_full_log()?;
    let mut ready = Vec::new();
    for t in store.list_tasks()? {
        if selection::task_satisfied(&log, &t.task_ref) {
            continue;
        }
        let plan = store.get_plan(t.plan_ref.as_str())?;
        let deps_ok = plan
            .nodes
            .iter()
            .find(|n| n.task_ref == t.task_ref)
            .map(|n| {
                n.depends_on
                    .iter()
                    .all(|d| selection::task_satisfied(&log, d))
            })
            .unwrap_or(true);
        if deps_ok {
            ready.push(s(t.task_ref.as_str()));
        }
    }
    Ok(CmdOutput::ok(Value::Array(ready)))
}

// ---- attempts / run ---------------------------------------------------------

fn report_json(r: &ExecutionReport) -> Value {
    kv(&[
        ("attempt_id", s(r.attempt_id.as_str())),
        (
            "run_id",
            r.run_id
                .as_ref()
                .map(|x| s(x.as_str()))
                .unwrap_or(Value::Null),
        ),
        ("state", json!(format!("{:?}", r.final_state))),
        (
            "outcome",
            r.outcome
                .as_ref()
                .map(|(o, reason)| {
                    kv(&[
                        ("terminal", j(o)),
                        ("reason", s(reason.0.clone())),
                        ("exit_mapping", json!(o.exit_code().code())),
                    ])
                })
                .unwrap_or(Value::Null),
        ),
        (
            "artifacts",
            Value::Array(
                r.artifacts
                    .iter()
                    .map(|(n, d)| kv(&[("name", s(n.clone())), ("digest", s(d.to_string()))]))
                    .collect(),
            ),
        ),
        ("duplicate_events", json!(r.duplicate_events)),
        ("event_gaps", j(&r.event_gaps)),
        ("task_satisfied", json!(r.task_satisfied)),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitAssertion {
    AttemptTerminal,
    ArtifactCommitted,
    ValidationPassed,
    TaskSatisfied,
    PlanCompleted,
}

impl std::str::FromStr for WaitAssertion {
    type Err = String;

    fn from_str(v: &str) -> Result<Self, Self::Err> {
        match v {
            "attempt-terminal" => Ok(Self::AttemptTerminal),
            "artifact-committed" => Ok(Self::ArtifactCommitted),
            "validation-passed" => Ok(Self::ValidationPassed),
            "task-satisfied" => Ok(Self::TaskSatisfied),
            "plan-completed" => Ok(Self::PlanCompleted),
            other => Err(format!("unknown --require assertion `{other}`")),
        }
    }
}

/// §9 wait semantics: exit 0 certifies the named assertion via a verified
/// receipt, nothing else. Outcome-specific codes win; 13 is the
/// validation-passed wait-assertion failure (objection O1's resolution).
fn wait_exit(
    store: &Store,
    report: &ExecutionReport,
    require: WaitAssertion,
) -> Result<(ExitCode, Vec<String>), CliError> {
    let log = store.load_full_log()?;
    let aid = &report.attempt_id;
    let mut warnings = Vec::new();
    let outcome_exit = report
        .outcome
        .as_ref()
        .map(|(o, _)| o.exit_code())
        .unwrap_or(ExitCode::Unknown);

    let exit = match require {
        WaitAssertion::AttemptTerminal => {
            if report.outcome.is_some() {
                outcome_exit
            } else {
                warnings.push("attempt is not terminal (recovery pending)".to_string());
                ExitCode::Unknown
            }
        }
        WaitAssertion::ArtifactCommitted => {
            let all_verified = log.for_attempt(aid).any(|e| {
                matches!(&e.body, ReceiptBody::Commit { artifacts, .. }
                    if !artifacts.is_empty()
                        && artifacts.iter().all(|a| a.verdict == CommitVerdict::Verified))
            });
            if all_verified {
                ExitCode::AssertedTrue
            } else {
                ExitCode::ArtifactCommitOrReadbackFailed
            }
        }
        WaitAssertion::ValidationPassed => {
            let mut any = false;
            let mut all_passed = true;
            for e in log.for_attempt(aid) {
                if let ReceiptBody::Validation { verdict, .. } = &e.body {
                    any = true;
                    if !matches!(verdict, ValidatorVerdict::Passed) {
                        all_passed = false;
                    }
                }
            }
            if any
                && all_passed
                && report.outcome.as_ref().map(|(o, _)| *o)
                    == Some(rein_core::outcome::TerminalOutcome::Success)
            {
                ExitCode::AssertedTrue
            } else {
                ExitCode::ValidationFailed
            }
        }
        WaitAssertion::TaskSatisfied => {
            if report.task_satisfied {
                ExitCode::AssertedTrue
            } else {
                warnings.push("no satisfying TaskSelectionReceipt (invariant 4)".to_string());
                outcome_exit
            }
        }
        WaitAssertion::PlanCompleted => {
            let row = store.get_attempt(aid)?;
            let task = store.get_task(&row.task_ref)?;
            let plan = store.get_plan(task.plan_ref.as_str())?;
            let all = plan
                .nodes
                .iter()
                .all(|n| selection::task_satisfied(&log, &n.task_ref));
            if all {
                ExitCode::AssertedTrue
            } else {
                warnings.push("plan has unsatisfied tasks".to_string());
                ExitCode::AttemptTerminalNonSuccess
            }
        }
    };
    Ok((exit, warnings))
}

pub fn attempt_start(
    ctx: &Ctx,
    task: &str,
    hand: Option<&str>,
    wait: bool,
    require: Option<WaitAssertion>,
) -> CmdResult {
    let t = task_ref(task)?;
    let (ws, mut store) = ctx.open()?;
    let broker = ctx.broker(&ws)?;
    let clock = SystemClock;
    let report = {
        let mut engine = Engine::new(&ws, &mut store, &clock, broker);
        engine.run_task(&t, hand, None)?
    };
    let mut out = CmdOutput::ok(report_json(&report));
    if wait {
        let require = require.unwrap_or(WaitAssertion::AttemptTerminal);
        let (exit, warnings) = wait_exit(&store, &report, require)?;
        out = out.with_exit(exit);
        for w in warnings {
            out = out.warn(w);
        }
    } else {
        out = out.warn("without --wait, exit 0 means the attempt was admitted and ran; it asserts nothing about the outcome (§9)");
    }
    Ok(out)
}

pub fn attempt_retry(ctx: &Ctx, id: &str, hand: Option<&str>) -> CmdResult {
    let aid = attempt_id(id)?;
    ctx.with_engine(|engine| {
        let report = engine.retry(&aid, hand)?;
        Ok(CmdOutput::ok(report_json(&report)))
    })
}

pub fn attempt_cancel(ctx: &Ctx, id: &str) -> CmdResult {
    let aid = attempt_id(id)?;
    ctx.with_engine(|engine| {
        engine.request_cancel(&aid)?;
        Ok(CmdOutput::ok(kv(&[
            ("attempt_id", s(aid.as_str())),
            (
                "cancel",
                s("requested — honored at the next phase boundary (bounded cancellation)"),
            ),
        ])))
    })
}

pub fn attempt_close(ctx: &Ctx, id: &str, reason: &str) -> CmdResult {
    let aid = attempt_id(id)?;
    ctx.with_engine(|engine| {
        let report = engine.close_as_unknown(&aid, reason)?;
        Ok(CmdOutput::ok(report_json(&report)))
    })
}

pub fn attempt_list(ctx: &Ctx) -> CmdResult {
    let (_, store) = ctx.open()?;
    let log = store.load_full_log()?;
    let rows: Vec<Value> = store
        .list_attempts()?
        .into_iter()
        .map(|a| {
            let state = resolve_state(&log, &a.attempt_id)
                .map(|st| format!("{st:?}"))
                .unwrap_or_else(|_| "unresolvable".into());
            let outcome = log
                .for_attempt(&a.attempt_id)
                .filter_map(|e| match &e.body {
                    ReceiptBody::Terminal { outcome, .. } => Some(format!("{outcome:?}")),
                    _ => None,
                })
                .last()
                .unwrap_or_else(|| "—".into());
            kv(&[
                ("attempt_id", s(a.attempt_id.as_str())),
                ("task", s(a.task_ref.as_str())),
                ("gen", json!(a.generation)),
                ("state", s(state)),
                ("outcome", s(outcome)),
            ])
        })
        .collect();
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn attempt_show(ctx: &Ctx, id: &str) -> CmdResult {
    let aid = attempt_id(id)?;
    let (_, store) = ctx.open()?;
    let row = store.get_attempt(&aid)?;
    let log = store.load_attempt_log(&aid)?;
    let state = resolve_state(&log, &aid)
        .map(|st| format!("{st:?}"))
        .unwrap_or_else(|_| "unresolvable".into());
    let receipts: Vec<Value> = log
        .for_attempt(&aid)
        .map(|e| {
            let body = serde_json::to_value(&e.body).unwrap_or(Value::Null);
            kv(&[
                ("receipt_id", s(e.receipt_id.as_str())),
                ("kind", body.get("kind").cloned().unwrap_or(Value::Null)),
                ("at", s(e.at.canonical())),
            ])
        })
        .collect();
    Ok(CmdOutput::ok(kv(&[
        ("attempt_id", s(row.attempt_id.as_str())),
        ("task", s(row.task_ref.as_str())),
        ("context_hash", s(row.context_hash.to_string())),
        ("generation", json!(row.generation)),
        ("state", s(state)),
        ("receipts", Value::Array(receipts)),
    ])))
}

pub fn attempt_watch(ctx: &Ctx, id: &str) -> CmdResult {
    let aid = attempt_id(id)?;
    let (_, store) = ctx.open()?;
    loop {
        let log = store.load_attempt_log(&aid)?;
        let state = resolve_state(&log, &aid)
            .map_err(|e| CliError::new(ExitCode::NotFound, e.to_string()))?;
        eprintln!("attempt {} — {state:?}", aid.as_str());
        if matches!(
            state,
            rein_core::state::AttemptState::Terminal
                | rein_core::state::AttemptState::Closed
                | rein_core::state::AttemptState::RecoveryPending
        ) {
            return attempt_show(ctx, id);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

// ---- plan run ---------------------------------------------------------------

pub fn plan_run(ctx: &Ctx, name: &str, hand: Option<&str>) -> CmdResult {
    let pr = plan_ref(name)?;
    let (ws, mut store) = ctx.open()?;
    let plan = store.get_plan(pr.as_str())?;
    let broker = ctx.broker(&ws)?;
    let clock = SystemClock;
    let mut results = Vec::new();
    let mut engine = Engine::new(&ws, &mut store, &clock, broker);
    // Ready-order execution: keep sweeping until nothing new becomes ready.
    let mut progressed = true;
    while progressed {
        progressed = false;
        let log = engine.store.load_full_log()?;
        let pending: Vec<TaskRef> = plan
            .nodes
            .iter()
            .filter(|n| !selection::task_satisfied(&log, &n.task_ref))
            .filter(|n| {
                n.depends_on
                    .iter()
                    .all(|d| selection::task_satisfied(&log, d))
            })
            .map(|n| n.task_ref.clone())
            .collect();
        for t in pending {
            let report = engine.run_task(&t, hand, None)?;
            let satisfied = report.task_satisfied;
            results.push(report_json(&report));
            if satisfied {
                progressed = true;
            }
        }
    }
    Ok(CmdOutput::ok(Value::Array(results)))
}

// ---- artifacts / validation / events / replay -------------------------------

pub fn artifact_list(ctx: &Ctx, attempt: Option<&str>) -> CmdResult {
    let (_, store) = ctx.open()?;
    let log = store.load_full_log()?;
    let mut rows = Vec::new();
    for e in log.iter() {
        if let ReceiptBody::Commit { artifacts, .. } = &e.body {
            if let Some(a) = attempt {
                if e.attempt_id.as_str() != a {
                    continue;
                }
            }
            for r in artifacts {
                rows.push(kv(&[
                    ("attempt", s(e.attempt_id.as_str())),
                    ("name", s(r.name.clone())),
                    ("verdict", j(&r.verdict)),
                    (
                        "digest",
                        r.readback_digest
                            .as_ref()
                            .map(|d| s(d.to_string()))
                            .unwrap_or(Value::Null),
                    ),
                ]));
            }
        }
    }
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn artifact_cat(ctx: &Ctx, digest: &str) -> CmdResult {
    let (ws, _) = ctx.open()?;
    let d =
        Sha256Digest::parse(digest).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?;
    let cas = rein_runtime::cas::Cas::new(ws.objects());
    let bytes = cas
        .read_verified(&d)
        .map_err(|e| CliError::new(ExitCode::NotFound, e.to_string()))?;
    Ok(CmdOutput::ok(json!(String::from_utf8_lossy(&bytes))))
}

pub fn artifact_verify(ctx: &Ctx, digest: Option<&str>) -> CmdResult {
    let (ws, store) = ctx.open()?;
    let cas = rein_runtime::cas::Cas::new(ws.objects());
    let mut checked = 0usize;
    let mut failures = Vec::new();
    let digests: Vec<Sha256Digest> =
        match digest {
            Some(d) => vec![Sha256Digest::parse(d)
                .map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?],
            None => {
                let log = store.load_full_log()?;
                let mut v = Vec::new();
                for e in log.iter() {
                    if let ReceiptBody::Commit { artifacts, .. } = &e.body {
                        for r in artifacts {
                            if let Some(d) = &r.readback_digest {
                                v.push(d.clone());
                            }
                        }
                    }
                }
                v
            }
        };
    for d in &digests {
        checked += 1;
        if let Err(e) = cas.verify(d) {
            failures.push(format!("{e}"));
        }
    }
    let out = kv(&[("checked", json!(checked)), ("failures", j(&failures))]);
    Ok(if failures.is_empty() {
        CmdOutput::ok(out)
    } else {
        CmdOutput::ok(out).with_exit(ExitCode::EvidenceReplayMismatch)
    })
}

pub fn validation_list(ctx: &Ctx, attempt: &str) -> CmdResult {
    let aid = attempt_id(attempt)?;
    let (_, store) = ctx.open()?;
    let log = store.load_attempt_log(&aid)?;
    let rows: Vec<Value> = log
        .for_attempt(&aid)
        .filter_map(|e| match &e.body {
            ReceiptBody::Validation {
                artifact_name,
                validator,
                verdict,
                ..
            } => Some(kv(&[
                ("artifact", s(artifact_name.clone())),
                ("validator", s(validator.to_string())),
                ("verdict", j(verdict)),
            ])),
            _ => None,
        })
        .collect();
    Ok(CmdOutput::ok(Value::Array(rows)))
}

pub fn events_list(ctx: &Ctx, run: &str, tail: Option<usize>) -> CmdResult {
    let run_id = RunId::parse(run).map_err(|e| CliError::new(ExitCode::Usage, e.to_string()))?;
    let (_, store) = ctx.open()?;
    let mut events = store.load_events(&run_id)?;
    if let Some(n) = tail {
        let len = events.len();
        events = events.split_off(len.saturating_sub(n));
    }
    Ok(CmdOutput::ok(j(&events)))
}

pub fn replay_attempt(ctx: &Ctx, id: &str, strict: bool) -> CmdResult {
    let aid = attempt_id(id)?;
    let (ws, store) = ctx.open()?;
    let hands = rein_runtime::hands::HandRegistry::with_fixtures();
    let report = rein_runtime::replay::replay_attempt(&ws, &store, &hands, &aid)?;
    let matches = report.matches();
    let out = CmdOutput::ok(j(&report));
    Ok(if strict && !matches {
        out.with_exit(ExitCode::EvidenceReplayMismatch)
    } else {
        out
    })
}
