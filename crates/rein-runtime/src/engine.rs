//! The execution engine: §7's seven phases — preflight → preparation → run →
//! artifact commit → validation → classification → closure — driven over the
//! durable ledger with the M0 pure functions doing every judgment.
//!
//! Pattern: load the ledger into the M0 `ReceiptLog`, operate with the M0
//! functions, persist the new tail after every phase. Classification never
//! sees an exit code; reaching `terminal` requires a terminal receipt; unknown
//! is only ever explicit (invariants 2, 3, 5, 22).

use crate::cas::Cas;
use crate::clock::Clock;
use crate::hands::{HandContext, HandRegistry};
use crate::store::{Store, StoreError};
use crate::validators::{ValidationInput, ValidatorRegistry};
use crate::workspace::{SecretBroker, Workspace};
use rein_core::canon::Sha256Digest;
use rein_core::capture::{CaptureArtifact, StdStream, Utf8StreamDecoder};
use rein_core::classify::{classify, ClassifyError};
use rein_core::context_pack::{ContextPack, PitMode};
use rein_core::entities::Epoch;
use rein_core::fence;
use rein_core::hand::{per_step_breach, EventLedger, HandEvent, HandRequest, IngestOutcome};
use rein_core::idempotency::{
    admit, AdmissionOutcome, AttemptRequest, IdempotencyKey, RequestKind,
};
use rein_core::ids::{AttemptId, GrantId, IdGen, RunId, TaskRef};
use rein_core::outcome::{ReasonCode, TerminalOutcome};
use rein_core::receipts::{
    evaluate_artifact_commit, AbortKind, BudgetScope, BudgetVerdict, ReceiptBody, ReceiptLog,
};
use rein_core::selection;
use rein_core::state::{apply_transition, AnomalyKind, AttemptState, TransitionCauseRecord};
use rein_core::time::LogicalMs;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Workspace(#[from] crate::workspace::WorkspaceError),
    #[error(transparent)]
    Cas(#[from] crate::cas::CasError),
    #[error(transparent)]
    Hand(#[from] crate::hands::HandError),
    #[error(transparent)]
    Admit(#[from] rein_core::idempotency::AdmitError),
    #[error(transparent)]
    Transition(#[from] rein_core::state::TransitionError),
    #[error(transparent)]
    Fence(#[from] rein_core::fence::FenceError),
    #[error(transparent)]
    Recovery(#[from] rein_core::recovery::RecoveryError),
    #[error(transparent)]
    Classify(#[from] ClassifyError),
    #[error(transparent)]
    Pack(#[from] rein_core::context_pack::PackError),
    #[error("epoch `{0}` is not sealed — seal it before running attempts against it")]
    EpochUnsealed(String),
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn io_err(path: PathBuf) -> impl FnOnce(std::io::Error) -> EngineError {
    move |source| EngineError::Io { path, source }
}

#[derive(Debug, Clone)]
pub struct ExecutionReport {
    pub attempt_id: AttemptId,
    pub run_id: Option<RunId>,
    pub final_state: AttemptState,
    pub outcome: Option<(TerminalOutcome, ReasonCode)>,
    pub artifacts: Vec<(String, Sha256Digest)>,
    pub duplicate_events: usize,
    pub event_gaps: Vec<u64>,
    pub task_satisfied: bool,
}

pub struct Engine<'a> {
    pub workspace: &'a Workspace,
    pub store: &'a mut Store,
    pub cas: Cas,
    pub clock: &'a dyn Clock,
    pub hands: HandRegistry,
    pub validators: ValidatorRegistry,
    pub broker: SecretBroker,
}

impl<'a> Engine<'a> {
    pub fn new(
        workspace: &'a Workspace,
        store: &'a mut Store,
        clock: &'a dyn Clock,
        broker: SecretBroker,
    ) -> Self {
        let cas = Cas::new(workspace.objects());
        let validators = ValidatorRegistry::builtin(broker.redactor());
        Self {
            workspace,
            store,
            cas,
            clock,
            hands: HandRegistry::with_fixtures(),
            validators,
            broker,
        }
    }

    fn cancel_flag(&self, attempt: &AttemptId) -> PathBuf {
        self.workspace
            .tmp()
            .join(attempt.as_str())
            .join("cancel-requested")
    }

    /// Bounded cancellation: a flag the pipeline honors between phases.
    pub fn request_cancel(&self, attempt: &AttemptId) -> Result<(), EngineError> {
        let path = self.cancel_flag(attempt);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent.to_path_buf()))?;
        }
        std::fs::write(&path, b"cancel").map_err(io_err(path))
    }

    /// Build a frozen ContextPack for a task under a sealed epoch (§5).
    pub fn build_pack(
        &mut self,
        task_ref: &TaskRef,
        hand_selector: &str,
        ids: &mut IdGen,
    ) -> Result<ContextPack, EngineError> {
        let task = self.store.get_task(task_ref)?;
        let plan = self.store.get_plan(task.plan_ref.as_str())?;
        // The epoch is the latest sealed one; M1 keeps a single active epoch.
        let (epoch, sealed) = self.latest_epoch()?;
        if !sealed {
            return Err(EngineError::EpochUnsealed(
                epoch.epoch_ref.as_str().to_string(),
            ));
        }

        // Instruction artifacts live in the CAS like everything else. A
        // SKILL.md for the task type, when installed, becomes the system
        // instructions and its validator_refs join the contract (M2) — the
        // manifest format is the fabric's, with additive keys.
        let skill_path = self
            .workspace
            .skills()
            .join(format!("{}.md", task.task_type));
        let (skill_validators, system_text) = match std::fs::read_to_string(&skill_path) {
            Ok(content) => {
                let (front, body) = parse_skill_frontmatter(&content);
                (front, body)
            }
            Err(_) => (
                Vec::new(),
                format!(
                    "rein system instructions v1\ntask_type={}\n",
                    task.task_type
                ),
            ),
        };
        let system = self.cas.put(system_text.as_bytes())?;
        let task_instr = self
            .cas
            .put(format!("task instructions for {}\n", task.task_ref).as_bytes())?;
        let aref = |d: &Sha256Digest| {
            rein_core::ids::ArtifactRef::parse(&format!("artifact:{d}")).expect("digest ref")
        };

        let mut pack = ContextPack {
            schema: rein_core::context_pack::SCHEMA.to_string(),
            context_pack_id: ids.context_pack(),
            context_hash: None,
            workspace_ref: self.workspace.manifest.workspace_ref.clone(),
            mission_ref: epoch.mission_ref.clone(),
            epoch_ref: epoch.epoch_ref.clone(),
            plan_ref: plan.plan_ref.clone(),
            task_ref: task.task_ref.clone(),
            pit_mode: epoch.pit_mode,
            source_cutoff: epoch.source_cutoff,
            knowledge_cutoff: epoch.knowledge_cutoff,
            provider_pins: epoch.provider_pins.clone(),
            universe: task.universe.clone(),
            inputs: task_inputs(self.store, &task)?,
            instructions: rein_core::context_pack::Instructions {
                system_ref: aref(&system),
                task_ref: aref(&task_instr),
            },
            hand: rein_core::context_pack::HandSelector {
                selector: hand_selector.to_string(),
                version_ref: rein_core::ids::HandRef::parse("hand:fake@1").expect("static"),
            },
            capabilities: rein_core::context_pack::Capabilities {
                filesystem: rein_core::context_pack::FsCaps {
                    read: vec!["input:///**".into()],
                    write: vec!["output:///**".into()],
                },
                network: rein_core::context_pack::NetworkMode::Deny,
                hand_internal_network: false,
                tools: Vec::new(),
                secrets: self.broker.known_refs(),
            },
            budget: epoch.budget_envelope.clone(),
            output_contract: {
                let mut c = task.output_contract.clone();
                for v in skill_validators {
                    if let Ok(vr) = rein_core::ids::ValidatorRef::parse(&v) {
                        if !c.validators.contains(&vr) {
                            c.validators.push(vr);
                        }
                    }
                }
                c
            },
            created_at: self.clock.now(),
        };
        // Research-capable hands declare their own egress (§6): the sandbox
        // cannot see inside agy, and pretending otherwise would be the
        // CapabilityGrant-doc defect again.
        if hand_selector.starts_with("agy") {
            pack.capabilities.hand_internal_network = true;
        }
        pack.seal()?;
        Ok(pack)
    }

    fn latest_epoch(&self) -> Result<(Epoch, bool), EngineError> {
        let mut epochs = self.store.list_epochs()?;
        epochs
            .pop()
            .ok_or_else(|| EngineError::EpochUnsealed("(none exists)".to_string()))
    }

    /// Admit and fully execute one attempt. Returns the report; every judgment
    /// along the way is already in the ledger by the time this returns.
    pub fn run_task(
        &mut self,
        task_ref: &TaskRef,
        hand_override: Option<&str>,
        retry_of: Option<AttemptId>,
    ) -> Result<ExecutionReport, EngineError> {
        let mut ids = self.store.id_gen()?;
        let selector = hand_override
            .map(str::to_string)
            .or_else(|| self.workspace.manifest.default_hand.clone())
            .unwrap_or_else(|| "fake:deterministic-a".to_string());

        let mut pack = self.build_pack(task_ref, &selector, &mut ids)?;
        // Retry binds the prior pack byte-identically (invariant 6): reuse the
        // prior attempt's stored pack, rebinding only the executor (C2).
        if let Some(prior) = &retry_of {
            let prior_row = self.store.get_attempt(prior)?;
            pack = self.store.get_pack(&prior_row.context_pack_id)?;
            pack.hand.selector = selector.clone();
        }

        let mut log = self.store.load_full_log()?;
        let mut persisted = log.len();
        let at = self.clock.now();

        let request = AttemptRequest {
            task_ref: task_ref.clone(),
            context_pack: pack.clone(),
            kind: match retry_of {
                Some(of) => RequestKind::Retry { of },
                None => RequestKind::Fresh,
            },
        };
        let outcome = admit(&mut log, &mut ids, &request, at)?;
        let attempt = match outcome {
            AdmissionOutcome::New { attempt, .. } => attempt,
            AdmissionOutcome::Duplicate { attempt_id, .. } => {
                // Duplicate delivery: the original receipt stands, no new
                // transition (§6 matrix; invariant 23).
                self.sync(&log, &mut persisted, &ids)?;
                let report = self.report_for(&log, &attempt_id)?;
                return Ok(report);
            }
        };
        let aid = attempt.attempt_id.clone();
        self.store.put_pack(&pack)?;
        self.store.insert_attempt(&attempt)?;

        // Budget reserve receipt (invariant 10's envelope, schema-side at M1).
        log.append(
            &mut ids,
            &aid,
            at,
            ReceiptBody::Budget {
                scope: BudgetScope::Reserve,
                verdict: BudgetVerdict::Reserved,
                detail: format!(
                    "max_steps={} per_step_timeout_ms={}",
                    pack.budget.max_steps, pack.budget.per_step_timeout_ms
                ),
            },
        );
        self.sync(&log, &mut persisted, &ids)?;

        // --- preflight admission decision: created → admitted --------------
        self.advance(&mut log, &mut ids, &aid, AttemptState::Admitted, at)?;

        // --- preparation ----------------------------------------------------
        if self.check_cancel_pre_run(&mut log, &mut ids, &aid, at)? {
            return self.finish_aborted(&mut log, &mut persisted, &mut ids, &aid, &pack);
        }
        self.advance(&mut log, &mut ids, &aid, AttemptState::Preparing, at)?;

        self.pipeline_from_preparing(
            &mut log,
            &mut persisted,
            &mut ids,
            &aid,
            &pack,
            attempt.generation,
        )
    }

    /// The pipeline from `preparing` onward — shared by fresh runs and by
    /// recovery's resume-commit, which re-enters here under a fresh fence
    /// generation as a new HarnessRun on the *same* attempt (§3).
    fn pipeline_from_preparing(
        &mut self,
        log: &mut ReceiptLog,
        persisted: &mut usize,
        ids: &mut IdGen,
        aid: &AttemptId,
        pack: &ContextPack,
        generation: u64,
    ) -> Result<ExecutionReport, EngineError> {
        let at = self.clock.now();
        let attempt_tmp = self.workspace.tmp().join(aid.as_str());
        let run_no = self.store.runs_for_attempt(aid)?.len() + 1;
        let sandbox = attempt_tmp.join(format!("run-{run_no}"));
        let inputs_dir = sandbox.join("inputs");
        let output_dir = sandbox.join("output");
        for d in [&inputs_dir, &output_dir] {
            std::fs::create_dir_all(d).map_err(io_err(d.clone()))?;
        }
        // Mount pinned inputs read-only from the CAS, with a manifest so
        // hands can tell which input is which.
        let mut manifest_entries = Vec::new();
        for (i, input) in pack.inputs.iter().enumerate() {
            let digest =
                Sha256Digest::parse(input.artifact_ref.as_str().trim_start_matches("artifact:"))
                    .map_err(|_| {
                        EngineError::Cas(crate::cas::CasError::Absent(Sha256Digest::of_bytes(
                            b"unparseable input ref",
                        )))
                    })?;
            let bytes = self.cas.read_verified(&digest)?;
            let file = format!("input-{i:02}");
            let path = inputs_dir.join(&file);
            std::fs::write(&path, bytes).map_err(io_err(path.clone()))?;
            let mut perms = std::fs::metadata(&path)
                .map_err(io_err(path.clone()))?
                .permissions();
            perms.set_readonly(true);
            std::fs::set_permissions(&path, perms).map_err(io_err(path))?;
            manifest_entries.push(serde_json::json!({
                "file": file,
                "artifact_ref": input.artifact_ref.as_str(),
                "media_type": input.media_type,
                "note": input.note,
            }));
        }
        let manifest_path = inputs_dir.join("inputs.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest_entries).expect("manifest serializes"),
        )
        .map_err(io_err(manifest_path))?;
        // The pack's system instructions (the task-type skill body) travel
        // into the sandbox too: hands read the method from `system.md`, and
        // the pack hash already binds its exact bytes.
        if let Ok(sys_digest) = Sha256Digest::parse(
            pack.instructions
                .system_ref
                .as_str()
                .trim_start_matches("artifact:"),
        ) {
            if let Ok(sys_bytes) = self.cas.read_verified(&sys_digest) {
                let sys_path = inputs_dir.join("system.md");
                std::fs::write(&sys_path, sys_bytes).map_err(io_err(sys_path))?;
            }
        }
        let mut env_notes = vec![
            "non-coverage: egress exfiltration, reads outside $HOME, wrong-file-inside-root (byte-reading validation runs regardless), anything after exit (§7)".to_string(),
            format!("inputs mounted read-only: {}", pack.inputs.len()),
        ];
        if pack.capabilities.hand_internal_network {
            env_notes.push(
                "network: delegated to hand (hand_internal_network) — the sandbox cannot see inside it; egress unenforced, stated (§6)".to_string(),
            );
            env_notes.push(
                "knowledge-cutoff: advisory — a served model trained after the cutoff cannot be prevented from laundering later knowledge (invariant 15)".to_string(),
            );
        } else {
            env_notes.push("in-process hand: OS sandbox not applicable".to_string());
        }
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Environment {
                binary_paths: vec![],
                notes: env_notes,
            },
        );
        self.sync(log, persisted, ids)?;

        if self.check_cancel_pre_run(log, ids, aid, at)? {
            return self.finish_aborted(log, persisted, ids, aid, pack);
        }

        // --- run ------------------------------------------------------------
        self.advance(log, ids, aid, AttemptState::Running, at)?;
        let fence_generation = fence::current_generation(log, aid)?;
        let run_id = ids.run();
        self.store
            .insert_run(&run_id, aid, fence_generation, &pack.hand.selector, at)?;

        let request = HandRequest {
            attempt_id: aid.clone(),
            run_id: run_id.clone(),
            fence_generation,
            sequence: 0,
            idempotency_key: IdempotencyKey::derive(
                &pack.task_ref,
                pack.context_hash.as_ref().expect("sealed"),
                generation,
            ),
            capability_ref: GrantId::parse("grant_workspace").expect("static"),
            trace: ids.trace(),
            deadline: LogicalMs(pack.budget.per_step_timeout_ms * u64::from(pack.budget.max_steps)),
            internal_retries_disabled: true,
        };
        let env = self.broker.env_for(&pack.capabilities.secrets);
        let hand = self.hands.get(&pack.hand.selector)?;
        let hand_out = hand.run(&HandContext {
            request: &request,
            contract: &pack.output_contract,
            budget: &pack.budget,
            inputs_dir: &inputs_dir,
            output_dir: &output_dir,
            env: &env,
        })?;

        // Ingest events: duplicates idempotent, gaps surfaced, conflicts are
        // a typed anomaly, never absorbed.
        let mut ledger = EventLedger::new(run_id.clone());
        let mut duplicate_events = 0usize;
        let mut conflict = false;
        for ev in &hand_out.events {
            match ledger.ingest(ev.clone()) {
                Ok(IngestOutcome::DuplicateIgnored) => duplicate_events += 1,
                Ok(_) => {}
                Err(_) => {
                    conflict = true;
                }
            }
        }
        let events: Vec<_> = ledger.events().cloned().collect();
        self.store.persist_events(&events)?;
        let event_gaps = ledger.gaps();

        // Capture: decode incrementally (invariant 30), redact (invariant 28).
        let mut decoder = Utf8StreamDecoder::new();
        let mut stdout = String::new();
        let mut completed = false;
        let mut disconnected = false;
        let mut child_exit = None;
        for ev in &events {
            match &ev.event {
                HandEvent::OutputChunk {
                    stream: StdStream::Stdout,
                    bytes,
                } => stdout.push_str(&decoder.feed(bytes)),
                HandEvent::RunCompleted { child_exit: c } => {
                    completed = true;
                    child_exit = *c;
                }
                HandEvent::Disconnected => disconnected = true,
                _ => {}
            }
        }
        stdout.push_str(&decoder.finish());
        let (scrubbed, _report) = self.broker.redactor().scrub(&stdout);
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Capture {
                run_id: run_id.clone(),
                capture: CaptureArtifact {
                    exit_code: child_exit,
                    stdout: scrubbed,
                    stderr: String::new(),
                    side_channels: vec![],
                    captured_via: "in-process".to_string(),
                    tool_versions: BTreeMap::new(),
                },
            },
        );

        // Per-step budget attribution (invariant 10).
        if let Some(breach) = per_step_breach(&events, &pack.budget) {
            log.append(
                ids,
                aid,
                at,
                ReceiptBody::Budget {
                    scope: BudgetScope::Step { step: breach.step },
                    verdict: BudgetVerdict::Exceeded,
                    detail: format!(
                        "step {} ran {}ms against {}ms",
                        breach.step, breach.elapsed_ms, breach.limit_ms
                    ),
                },
            );
        }
        self.sync(log, persisted, ids)?;

        // Cancellation observed during the run: recorded as an abort-cause
        // receipt; the pipeline still completes (O2: from running onward the
        // drawn edges are walked, and evidence is captured).
        if self.cancel_flag(aid).exists() {
            log.append(
                ids,
                aid,
                at,
                ReceiptBody::AbortCause {
                    abort: AbortKind::Cancelled {
                        by: "operator".to_string(),
                    },
                    detail: "cancel requested during run; pipeline completed for evidence"
                        .to_string(),
                },
            );
        }

        // Run lost (disconnect) or event-stream conflict: typed anomaly,
        // recovery, no inferred verdict (invariant 5).
        if (!completed && disconnected) || conflict {
            let anomaly = if conflict {
                AnomalyKind::DuplicateCallback
            } else {
                AnomalyKind::UnknownAfterDisconnect
            };
            rein_core::recovery::enter_recovery(log, ids, aid, anomaly, at)?;
            self.sync(log, persisted, ids)?;
            let report = self.report_for(log, aid)?;
            return Ok(report);
        }

        // --- artifact commit ------------------------------------------------
        self.advance(log, ids, aid, AttemptState::CommitPending, at)?;
        fence::guard_commit(log, aid, fence_generation)?;

        let mut records = Vec::new();
        let mut readback: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for artifact in &pack.output_contract.required_artifacts {
            let staged_path = output_dir.join(&artifact.name);
            let staged_bytes = std::fs::read(&staged_path).ok();
            let record = match staged_bytes {
                None => evaluate_artifact_commit(
                    &artifact.name,
                    hand_out.claimed.get(&artifact.name),
                    None,
                    None,
                ),
                Some(bytes) => {
                    let digest = self.cas.put(&bytes)?;
                    // Read back through a handle the writer did not own.
                    let read = self.cas.read_verified(&digest)?;
                    let rec = evaluate_artifact_commit(
                        &artifact.name,
                        hand_out.claimed.get(&artifact.name),
                        Some(&bytes),
                        Some(&read),
                    );
                    readback.insert(artifact.name.clone(), read);
                    rec
                }
            };
            records.push(record);
        }
        log.append(
            ids,
            aid,
            at,
            ReceiptBody::Commit {
                fence_generation,
                artifacts: records.clone(),
            },
        );
        self.sync(log, persisted, ids)?;

        // --- validation (over read-back bytes only) -------------------------
        self.advance(log, ids, aid, AttemptState::Validating, at)?;
        for (artifact, record) in pack.output_contract.required_artifacts.iter().zip(&records) {
            if record.verdict != rein_core::receipts::CommitVerdict::Verified {
                continue;
            }
            let bytes = &readback[&artifact.name];
            for v in &pack.output_contract.validators {
                let verdict = self.validators.run(
                    v,
                    &ValidationInput {
                        artifact,
                        bytes,
                        all_artifacts: &readback,
                        pack,
                    },
                );
                let quarantined = matches!(
                    verdict,
                    rein_core::receipts::ValidatorVerdict::Quarantined { .. }
                );
                log.append(
                    ids,
                    aid,
                    at,
                    ReceiptBody::Validation {
                        artifact_name: artifact.name.clone(),
                        validator: v.clone(),
                        over_digest: record.readback_digest.clone(),
                        verdict,
                    },
                );
                if quarantined {
                    log.append(
                        ids,
                        aid,
                        at,
                        ReceiptBody::Quarantine {
                            artifact_name: artifact.name.clone(),
                            validator: v.clone(),
                            withheld_from_selection: true,
                        },
                    );
                }
            }
        }
        self.sync(log, persisted, ids)?;

        // --- classification + closure --------------------------------------
        self.advance(log, ids, aid, AttemptState::Classifying, at)?;
        let c = classify(log, aid, &pack.output_contract)?;
        let terminal_receipt = log.append(
            ids,
            aid,
            at,
            ReceiptBody::Terminal {
                outcome: c.outcome,
                reason: c.reason,
                supporting: c.supporting,
            },
        );
        apply_transition(
            log,
            ids,
            aid,
            AttemptState::Terminal,
            TransitionCauseRecord::ClassificationComplete { terminal_receipt },
            at,
        )?;
        apply_transition(
            log,
            ids,
            aid,
            AttemptState::Closed,
            TransitionCauseRecord::Close,
            at,
        )?;
        self.sync(log, persisted, ids)?;

        // --- selection (invariant 4): adjudicate the task -------------------
        let candidates = self.store.attempts_for_task(&pack.task_ref)?;
        selection::select_and_record(log, ids, &pack.task_ref, &candidates, at);
        self.sync(log, persisted, ids)?;

        let mut report = self.report_for(log, aid)?;
        report.run_id = Some(run_id);
        report.duplicate_events = duplicate_events;
        report.event_gaps = event_gaps;
        Ok(report)
    }

    fn advance(
        &mut self,
        log: &mut ReceiptLog,
        ids: &mut IdGen,
        aid: &AttemptId,
        to: AttemptState,
        at: rein_core::time::Timestamp,
    ) -> Result<(), EngineError> {
        apply_transition(log, ids, aid, to, TransitionCauseRecord::Advance, at)?;
        Ok(())
    }

    fn check_cancel_pre_run(
        &mut self,
        log: &mut ReceiptLog,
        ids: &mut IdGen,
        aid: &AttemptId,
        at: rein_core::time::Timestamp,
    ) -> Result<bool, EngineError> {
        if !self.cancel_flag(aid).exists() {
            return Ok(false);
        }
        rein_core::state::abort_to_classifying(
            log,
            ids,
            aid,
            AbortKind::Cancelled {
                by: "operator".to_string(),
            },
            "cancel requested before run",
            at,
        )?;
        Ok(true)
    }

    /// Classify and close an aborted (pre-run) attempt.
    fn finish_aborted(
        &mut self,
        log: &mut ReceiptLog,
        persisted: &mut usize,
        ids: &mut IdGen,
        aid: &AttemptId,
        pack: &ContextPack,
    ) -> Result<ExecutionReport, EngineError> {
        let at = self.clock.now();
        let c = classify(log, aid, &pack.output_contract)?;
        let terminal_receipt = log.append(
            ids,
            aid,
            at,
            ReceiptBody::Terminal {
                outcome: c.outcome,
                reason: c.reason,
                supporting: c.supporting,
            },
        );
        apply_transition(
            log,
            ids,
            aid,
            AttemptState::Terminal,
            TransitionCauseRecord::ClassificationComplete { terminal_receipt },
            at,
        )?;
        apply_transition(
            log,
            ids,
            aid,
            AttemptState::Closed,
            TransitionCauseRecord::Close,
            at,
        )?;
        self.sync(log, persisted, ids)?;
        self.report_for(log, aid)
    }

    fn sync(
        &mut self,
        log: &ReceiptLog,
        persisted: &mut usize,
        ids: &IdGen,
    ) -> Result<(), EngineError> {
        self.store.persist_receipts_from(log, *persisted)?;
        *persisted = log.len();
        self.store.save_id_gen(ids)?;
        Ok(())
    }

    fn report_for(
        &self,
        log: &ReceiptLog,
        aid: &AttemptId,
    ) -> Result<ExecutionReport, EngineError> {
        let final_state = rein_core::state::resolve_state(log, aid)?;
        let mut outcome = None;
        let mut artifacts = Vec::new();
        let mut task_ref = None;
        for e in log.for_attempt(aid) {
            match &e.body {
                ReceiptBody::Terminal {
                    outcome: o, reason, ..
                } => outcome = Some((*o, reason.clone())),
                ReceiptBody::Commit {
                    artifacts: recs, ..
                } => {
                    artifacts = recs
                        .iter()
                        .filter_map(|r| r.readback_digest.clone().map(|d| (r.name.clone(), d)))
                        .collect();
                }
                ReceiptBody::AttemptCreated { task_ref: t, .. } => task_ref = Some(t.clone()),
                _ => {}
            }
        }
        let task_satisfied = task_ref
            .map(|t| selection::task_satisfied(log, &t))
            .unwrap_or(false);
        Ok(ExecutionReport {
            attempt_id: aid.clone(),
            run_id: None,
            final_state,
            outcome,
            artifacts,
            duplicate_events: 0,
            event_gaps: Vec::new(),
            task_satisfied,
        })
    }

    /// Close a recovery-pending attempt as unknown — the explicit path
    /// (invariant 5), surfaced by `rein attempt close`.
    pub fn close_as_unknown(
        &mut self,
        attempt_id: &AttemptId,
        reason: &str,
    ) -> Result<ExecutionReport, EngineError> {
        let mut log = self.store.load_full_log()?;
        let mut persisted = log.len();
        let mut ids = self.store.id_gen()?;
        let at = self.clock.now();
        rein_core::recovery::close_attempt_as_unknown(
            &mut log,
            &mut ids,
            attempt_id,
            ReasonCode(reason.to_string()),
            vec![],
            at,
        )?;
        self.sync(&log, &mut persisted, &ids)?;
        self.report_for(&log, attempt_id)
    }

    /// Recovery action 1 (§8): resume under a new fence generation — a new
    /// HarnessRun on the same attempt; old generations may not commit
    /// (invariant 24). Only legal from `recovery_pending`.
    pub fn resume_attempt(
        &mut self,
        attempt_id: &AttemptId,
        hand_override: Option<&str>,
    ) -> Result<ExecutionReport, EngineError> {
        let row = self.store.get_attempt(attempt_id)?;
        let mut pack = self.store.get_pack(&row.context_pack_id)?;
        if let Some(hand) = hand_override {
            // Execution binding, not semantic content (C2): a dead hand can
            // be replaced without touching the frozen pack's hash.
            pack.hand.selector = hand.to_string();
        }
        let mut log = self.store.load_full_log()?;
        let mut persisted = log.len();
        let mut ids = self.store.id_gen()?;
        let at = self.clock.now();
        rein_core::recovery::resume_commit_new_generation(&mut log, &mut ids, attempt_id, at)?;
        self.sync(&log, &mut persisted, &ids)?;
        self.pipeline_from_preparing(
            &mut log,
            &mut persisted,
            &mut ids,
            attempt_id,
            &pack,
            row.generation,
        )
    }

    /// Retry under the byte-identical pack (recovery action 2), optionally
    /// rebinding the executor (C2 amendment), then execute.
    /// Operational retry (invariant 6). The hand DEFAULTS TO THE ORIGINAL
    /// attempt's hand, never to the workspace default: found 2026-08-21 when
    /// recovering a real research attempt silently re-ran it on a fixture
    /// hand and reported `artifact_invalid` for a reason that had nothing to
    /// do with the original work. A recovery must not quietly change the
    /// executor; `hand_override` remains available for a deliberate change.
    pub fn retry(
        &mut self,
        prior: &AttemptId,
        hand_override: Option<&str>,
    ) -> Result<ExecutionReport, EngineError> {
        let row = self.store.get_attempt(prior)?;
        let original = self.original_hand(&row.context_pack_id);
        let hand = hand_override.or(original.as_deref());
        self.run_task(&row.task_ref, hand, Some(prior.clone()))
    }

    /// The hand an attempt's frozen pack recorded, when the pack is readable.
    fn original_hand(&self, pack_id: &rein_core::ids::ContextPackId) -> Option<String> {
        self.store
            .get_pack(pack_id)
            .ok()
            .map(|p| p.hand.selector.clone())
    }

    /// PIT sanity used by data tools from M2 on; here from M1 so the rule has
    /// one home: a past-cutoff epoch may only read own-CAS captures
    /// (invariant 13). Returns whether live pulls are permitted.
    pub fn live_pulls_permitted(epoch: &Epoch, now: rein_core::time::Timestamp) -> bool {
        match epoch.pit_mode {
            PitMode::Eval => false,
            PitMode::Production => epoch.source_cutoff >= now,
        }
    }
}

/// Resolve a task's pinned inputs into pack `InputPin`s, with media type and
/// note from the capture index when present.
fn task_inputs(
    store: &Store,
    task: &rein_core::entities::TaskVersion,
) -> Result<Vec<rein_core::context_pack::InputPin>, EngineError> {
    let mut out = Vec::new();
    for aref in &task.inputs {
        let digest = aref.as_str().trim_start_matches("artifact:").to_string();
        let row = store.get_capture(&digest)?;
        let (media_type, note) = match row {
            Some(r) => (
                r.media_type,
                r.note.unwrap_or_else(|| format!("{}:{}", r.tool, r.params)),
            ),
            None => (
                "application/octet-stream".to_string(),
                "pinned input".to_string(),
            ),
        };
        out.push(rein_core::context_pack::InputPin {
            artifact_ref: aref.clone(),
            media_type,
            note,
            required: true,
        });
    }
    Ok(out)
}

/// Minimal SKILL.md frontmatter reader: `validator_refs` plus the body.
fn parse_skill_frontmatter(content: &str) -> (Vec<String>, String) {
    let mut parts = content.splitn(3, "---");
    let _ = parts.next();
    match (parts.next(), parts.next()) {
        (Some(front), Some(body)) => {
            #[derive(serde::Deserialize, Default)]
            struct Front {
                #[serde(default)]
                validator_refs: Vec<String>,
            }
            let f: Front = serde_yaml::from_str(front).unwrap_or_default();
            (f.validator_refs, body.trim_start().to_string())
        }
        _ => (Vec::new(), content.to_string()),
    }
}
