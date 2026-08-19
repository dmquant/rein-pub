//! Shared test infrastructure: a §5-shaped sample pack and a pure model of
//! §7's pipeline (preflight → run → commit → validation → classification),
//! driven entirely by receipts — no IO, no clock.
#![allow(dead_code)]

use rein_core::capture::{CaptureArtifact, StdStream, Utf8StreamDecoder};
use rein_core::classify::classify;
use rein_core::context_pack::*;
use rein_core::fakes::{FakeHand, FIXTURE_SECRET_VALUE};
use rein_core::fence;
use rein_core::hand::{per_step_breach, EventLedger, HandEvent, HandRequest, IngestOutcome};
use rein_core::idempotency::{
    admit, AdmissionOutcome, AttemptRequest, IdempotencyKey, RequestKind,
};
use rein_core::ids::*;
use rein_core::outcome::{ReasonCode, TerminalOutcome};
use rein_core::pins::ProviderPin;
use rein_core::receipts::*;
use rein_core::recovery;
use rein_core::secretref::Redactor;
use rein_core::state::{apply_transition, AnomalyKind, AttemptState, TransitionCauseRecord};
use rein_core::time::{LogicalMs, Timestamp};
use std::collections::BTreeMap;

pub fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).expect("test timestamp")
}

pub fn sample_contract() -> OutputContract {
    OutputContract {
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
        validators: vec![
            ValidatorRef::parse("json-schema@1").unwrap(),
            ValidatorRef::parse("secret-scan@1").unwrap(),
        ],
    }
}

/// A §5-shaped ContextPack. Unsealed — callers seal.
pub fn sample_pack() -> ContextPack {
    let mut pins = BTreeMap::new();
    pins.insert(
        "hand".to_string(),
        ProviderPin::Digest {
            coordinate: "hand:fake@1".into(),
            digest: rein_core::canon::Sha256Digest::of_bytes(b"fake hand build"),
        },
    );
    pins.insert(
        "data".to_string(),
        ProviderPin::Service {
            coordinate: "fmp-api@v4".into(),
            pin_method: "served version header, recorded per call".into(),
        },
    );
    ContextPack {
        schema: SCHEMA.to_string(),
        context_pack_id: ContextPackId::parse("ctx_000001").unwrap(),
        context_hash: None,
        workspace_ref: WorkspaceRef::parse("ws:local").unwrap(),
        mission_ref: MissionRef::parse("mission:etf-book-valuations").unwrap(),
        epoch_ref: EpochRef::parse("epoch:2026-08-18").unwrap(),
        plan_ref: PlanRef::parse("plan:top20@3").unwrap(),
        task_ref: TaskRef::parse("task:dcf-nvda@2").unwrap(),
        pit_mode: PitMode::Production,
        source_cutoff: t("2026-08-18T00:00:00Z"),
        knowledge_cutoff: t("2026-08-18T00:00:00Z"),
        provider_pins: pins,
        universe: vec!["security:nvda".into()],
        inputs: vec![InputPin {
            artifact_ref: ArtifactRef::parse(
                "artifact:sha256:1111111111111111111111111111111111111111111111111111111111111111",
            )
            .unwrap(),
            media_type: "application/json".into(),
            note: "institute stance rows for NVDA, captured 2026-08-18".into(),
            required: true,
        }],
        instructions: Instructions {
            system_ref: ArtifactRef::parse(
                "artifact:sha256:2222222222222222222222222222222222222222222222222222222222222222",
            )
            .unwrap(),
            task_ref: ArtifactRef::parse(
                "artifact:sha256:3333333333333333333333333333333333333333333333333333333333333333",
            )
            .unwrap(),
        },
        hand: HandSelector {
            selector: "fake:deterministic-a".into(),
            version_ref: HandRef::parse("hand:fake@1").unwrap(),
        },
        capabilities: Capabilities {
            filesystem: FsCaps {
                read: vec!["input:///**".into()],
                write: vec!["output:///**".into()],
            },
            network: NetworkMode::Deny,
            hand_internal_network: false,
            tools: vec!["compute.valuation.*".into()],
            secrets: vec![SecretRefId::parse("secret-ref:fixture").unwrap()],
        },
        budget: Budget {
            max_steps: 24,
            per_step_timeout_ms: 240_000,
            tokens: Some(200_000),
            tool_calls: Some(60),
        },
        output_contract: sample_contract(),
        created_at: t("2026-08-19T07:00:00Z"),
    }
}

pub fn sealed_sample_pack() -> ContextPack {
    let mut p = sample_pack();
    p.seal().expect("sample pack seals");
    p
}

pub fn fixture_redactor() -> Redactor {
    Redactor::new(vec![(
        SecretRefId::parse("secret-ref:fixture").unwrap(),
        FIXTURE_SECRET_VALUE.to_string(),
    )])
}

pub struct PipelineResult {
    pub log: ReceiptLog,
    pub ids: IdGen,
    pub attempt_id: AttemptId,
    pub pack: ContextPack,
    /// Outcome + reason from the ledger's latest Terminal receipt.
    pub outcome: Option<(TerminalOutcome, ReasonCode)>,
    pub final_state: AttemptState,
    pub decoded_stdout: String,
    pub duplicate_events: usize,
}

pub fn advance(
    log: &mut ReceiptLog,
    ids: &mut IdGen,
    attempt: &AttemptId,
    to: AttemptState,
    at: Timestamp,
) {
    apply_transition(log, ids, attempt, to, TransitionCauseRecord::Advance, at)
        .unwrap_or_else(|e| panic!("advance to {to:?}: {e}"));
}

/// Drive one fake hand through the pure M0 pipeline.
pub fn run_fixture_pipeline(fixture: &dyn FakeHand) -> PipelineResult {
    let mut ids = IdGen::new();
    let mut log = ReceiptLog::new();
    let at = t("2026-08-19T08:00:00Z");
    let pack = sealed_sample_pack();

    let request = AttemptRequest {
        task_ref: pack.task_ref.clone(),
        context_pack: pack.clone(),
        kind: RequestKind::Fresh,
    };
    let AdmissionOutcome::New { attempt, .. } = admit(&mut log, &mut ids, &request, at).unwrap()
    else {
        panic!("fresh admission");
    };
    let aid = attempt.attempt_id.clone();

    advance(&mut log, &mut ids, &aid, AttemptState::Admitted, at);
    advance(&mut log, &mut ids, &aid, AttemptState::Preparing, at);
    advance(&mut log, &mut ids, &aid, AttemptState::Running, at);

    let hash = pack.context_hash.clone().unwrap();
    let req = HandRequest {
        attempt_id: aid.clone(),
        run_id: ids.run(),
        fence_generation: fence::current_generation(&log, &aid).unwrap(),
        sequence: 0,
        idempotency_key: IdempotencyKey::derive(&pack.task_ref, &hash, attempt.generation),
        capability_ref: GrantId::parse("grant_fixture").unwrap(),
        trace: ids.trace(),
        deadline: LogicalMs(10_000_000),
        internal_retries_disabled: true,
    };
    let out = fixture.run(&req, &pack.output_contract, &pack.budget);

    // Ingest the event stream: duplicates idempotent, gaps surfaced.
    let mut ledger = EventLedger::new(req.run_id.clone());
    let mut duplicate_events = 0usize;
    for ev in &out.events {
        if ledger
            .ingest(ev.clone())
            .expect("no conflicting duplicates in fixtures")
            == IngestOutcome::DuplicateIgnored
        {
            duplicate_events += 1;
        }
    }

    // Capture (invariant 30 decoder for stdout).
    let mut decoder = Utf8StreamDecoder::new();
    let mut stdout = String::new();
    let mut completed = false;
    let mut disconnected = false;
    let mut child_exit = None;
    for ev in ledger.events() {
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

    // Secrets are redacted from capture before it becomes durable (invariant 28).
    let redactor = fixture_redactor();
    let (scrubbed_stdout, _redaction) = redactor.scrub(&stdout);
    log.append(
        &mut ids,
        &aid,
        at,
        ReceiptBody::Capture {
            run_id: req.run_id.clone(),
            capture: CaptureArtifact {
                exit_code: child_exit,
                stdout: scrubbed_stdout,
                stderr: String::new(),
                side_channels: vec![],
                captured_via: "in-process".into(),
                tool_versions: BTreeMap::new(),
            },
        },
    );

    // Per-step budget attribution (invariant 10).
    let events: Vec<_> = ledger.events().cloned().collect();
    if let Some(breach) = per_step_breach(&events, &pack.budget) {
        log.append(
            &mut ids,
            &aid,
            at,
            ReceiptBody::Budget {
                scope: BudgetScope::Step { step: breach.step },
                verdict: BudgetVerdict::Exceeded,
                detail: format!(
                    "step {} took {}ms against {}ms",
                    breach.step, breach.elapsed_ms, breach.limit_ms
                ),
            },
        );
    }

    if !completed && disconnected {
        // Run lost: recovery, close-as-unknown — never inferred (invariant 5).
        recovery::enter_recovery(
            &mut log,
            &mut ids,
            &aid,
            AnomalyKind::UnknownAfterDisconnect,
            at,
        )
        .unwrap();
        recovery::close_attempt_as_unknown(
            &mut log,
            &mut ids,
            &aid,
            ReasonCode::run_lost_no_evidence(),
            vec![],
            at,
        )
        .unwrap();
    } else {
        advance(&mut log, &mut ids, &aid, AttemptState::CommitPending, at);

        // Commit with independent read-back (M0 model: a fresh copy).
        fence::guard_commit(&log, &aid, req.fence_generation).unwrap();
        let records: Vec<_> = pack
            .output_contract
            .required_artifacts
            .iter()
            .map(|a| {
                let staged = out.staged.get(&a.name).map(|v| v.as_slice());
                let readback = staged.map(|v| v.to_vec());
                evaluate_artifact_commit(
                    &a.name,
                    out.claimed.get(&a.name),
                    staged,
                    readback.as_deref(),
                )
            })
            .collect();
        log.append(
            &mut ids,
            &aid,
            at,
            ReceiptBody::Commit {
                fence_generation: req.fence_generation,
                artifacts: records.clone(),
            },
        );
        advance(&mut log, &mut ids, &aid, AttemptState::Validating, at);

        // Validators over read-back bytes: secret-scan is real, the rest are
        // M0 pass-stubs (real validators land M2).
        for (a, rec) in pack.output_contract.required_artifacts.iter().zip(&records) {
            if rec.verdict != CommitVerdict::Verified {
                continue;
            }
            let bytes = out.staged.get(&a.name).unwrap();
            for v in &pack.output_contract.validators {
                let quarantined = v.as_str().starts_with("secret-scan@")
                    && redactor.scan(&String::from_utf8_lossy(bytes)).is_some();
                if quarantined {
                    log.append(
                        &mut ids,
                        &aid,
                        at,
                        ReceiptBody::Validation {
                            artifact_name: a.name.clone(),
                            validator: v.clone(),
                            over_digest: rec.readback_digest.clone(),
                            verdict: ValidatorVerdict::Quarantined {
                                reason: "artifact carries a secret value".into(),
                            },
                        },
                    );
                    log.append(
                        &mut ids,
                        &aid,
                        at,
                        ReceiptBody::Quarantine {
                            artifact_name: a.name.clone(),
                            validator: v.clone(),
                            withheld_from_selection: true,
                        },
                    );
                } else {
                    log.append(
                        &mut ids,
                        &aid,
                        at,
                        ReceiptBody::Validation {
                            artifact_name: a.name.clone(),
                            validator: v.clone(),
                            over_digest: rec.readback_digest.clone(),
                            verdict: ValidatorVerdict::Passed,
                        },
                    );
                }
            }
        }
        advance(&mut log, &mut ids, &aid, AttemptState::Classifying, at);

        let c = classify(&log, &aid, &pack.output_contract).expect("pipeline evidence classifies");
        let terminal_receipt = log.append(
            &mut ids,
            &aid,
            at,
            ReceiptBody::Terminal {
                outcome: c.outcome,
                reason: c.reason,
                supporting: c.supporting,
            },
        );
        apply_transition(
            &mut log,
            &mut ids,
            &aid,
            AttemptState::Terminal,
            TransitionCauseRecord::ClassificationComplete { terminal_receipt },
            at,
        )
        .unwrap();
        apply_transition(
            &mut log,
            &mut ids,
            &aid,
            AttemptState::Closed,
            TransitionCauseRecord::Close,
            at,
        )
        .unwrap();
    }

    let outcome = log.for_attempt(&aid).rev_last_terminal();
    let final_state = rein_core::state::resolve_state(&log, &aid).unwrap();
    PipelineResult {
        outcome,
        final_state,
        decoded_stdout: stdout,
        duplicate_events,
        attempt_id: aid,
        pack,
        log,
        ids,
    }
}

trait TerminalScan {
    fn rev_last_terminal(self) -> Option<(TerminalOutcome, ReasonCode)>;
}

impl<'a, I: Iterator<Item = &'a ReceiptEnvelope>> TerminalScan for I {
    fn rev_last_terminal(self) -> Option<(TerminalOutcome, ReasonCode)> {
        let mut found = None;
        for e in self {
            if let ReceiptBody::Terminal {
                outcome, reason, ..
            } = &e.body
            {
                found = Some((*outcome, reason.clone()));
            }
        }
        found
    }
}
