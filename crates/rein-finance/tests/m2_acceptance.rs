//! M2 acceptance (§13) and the M2-owed invariant manifest rows:
//! 10, 11, 12, 13, 14, 15, 16, 17/18, 19, 20, 21, 26, 29.
//!
//! The acceptance core: a valuation run on one ticker, through the full
//! pipeline over pinned captures, whose DCF recomputes from assumptions.json
//! alone and whose every assumption resolves to a basis.
#![allow(non_snake_case)]

use rein_core::context_pack::PitMode;
use rein_core::entities::{Epoch, Mission, Plan, PlanNode, TaskVersion};
use rein_core::ids::{ArtifactRef, MissionRef, PlanRef, TaskRef, ValidatorRef, WorkspaceRef};
use rein_core::outcome::TerminalOutcome;
use rein_core::time::Timestamp;
use rein_finance::capture::{capture_admissible, ensure_live_permitted, CaptureStore};
use rein_finance::datum::{AsOfBasis, Stamped};
use rein_finance::fmp::{EquityEndpoint, FmpClient};
use rein_finance::schemas::{claim_admissible, Claim, ClaimKind, Claims, CLAIMS_SCHEMA};
use rein_finance::validators::{register_finance_validators, FinanceContext};
use rein_runtime::cas::Cas;
use rein_runtime::clock::FixedClock;
use rein_runtime::engine::Engine;
use rein_runtime::store::{CaptureRow, Store};
use rein_runtime::workspace::{SecretBroker, Workspace};
use std::collections::BTreeMap;
use std::io::{Read, Write};

fn t(s: &str) -> Timestamp {
    Timestamp::parse(s).unwrap()
}

const CUTOFF: &str = "2026-08-18T00:00:00Z";
const RETRIEVED: &str = "2026-08-17T12:00:00Z";

struct Fx {
    ws: Workspace,
    store: Store,
    config: tempfile::TempDir,
    _ws_dir: tempfile::TempDir,
}

fn valuation_contract() -> rein_core::context_pack::OutputContract {
    use rein_core::context_pack::{OutputContract, RequiredArtifact};
    let v = |n: &str| ValidatorRef::parse(n).unwrap();
    OutputContract {
        required_artifacts: vec![
            RequiredArtifact {
                name: "assumptions.json".into(),
                media_type: "application/json".into(),
                schema_ref: Some("schema:rein.assumptions/v1".into()),
                min_bytes: None,
            },
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
            v("artifact-wellformed@1"),
            v("secret-scan@1"),
            v("input-closure@1"),
            v("numeric-consistency@1"),
            v("bridge-completeness@1"),
            v("falsifier-present@1"),
            v("source-cutoff@1"),
            v("coverage-denominator@1"),
        ],
    }
}

/// Synthetic FMP-shaped captures for NVDA, retrieved inside the cutoff.
fn plant_captures(ws: &Workspace, store: &mut Store) -> Vec<ArtifactRef> {
    let cas = Cas::new(ws.objects());
    let mut refs = Vec::new();
    let mut plant = |tool: &str, endpoint: &str, body: serde_json::Value| {
        let bytes = serde_json::to_vec_pretty(&body).unwrap();
        let digest = cas.put(&bytes).unwrap();
        store
            .insert_capture(&CaptureRow {
                digest: digest.clone(),
                tool: tool.to_string(),
                params: format!("symbol=NVDA&endpoint={endpoint}"),
                provider: "Financial Modeling Prep".into(),
                media_type: "application/json".into(),
                as_of: Some(t("2026-06-30T00:00:00Z")),
                as_of_basis: Some("provider".into()),
                retrieved_at: t(RETRIEVED),
                url: None,
                host: Some("financialmodelingprep.com".into()),
                note: Some(format!("fmp:{endpoint}:NVDA")),
            })
            .unwrap();
        refs.push(ArtifactRef::parse(&format!("artifact:{digest}")).unwrap());
    };
    plant(
        "data.equity.quote",
        "quote",
        serde_json::json!([{"symbol":"NVDA","price":182.5,"sharesOutstanding":24400000000.0,"timestamp":1787086800}]),
    );
    plant(
        "data.equity.fundamentals",
        "cash-flow-statement",
        serde_json::json!([{"date":"2026-01-26","freeCashFlow":60853000000.0}]),
    );
    plant(
        "data.equity.fundamentals",
        "balance-sheet-statement",
        serde_json::json!([{"date":"2026-01-26","totalDebt":8460000000.0,"cashAndCashEquivalents":43210000000.0,"minorityInterest":0.0}]),
    );
    refs
}

fn fixture() -> (Fx, Vec<ArtifactRef>) {
    let ws_dir = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ws = Workspace::init(
        ws_dir.path(),
        WorkspaceRef::parse("ws:m2").unwrap(),
        t("2026-08-01T00:00:00Z"),
    )
    .unwrap();
    // Skills installed like `rein init` does — validator_refs ride frontmatter.
    rein_finance::skills::install(&ws.skills()).unwrap();
    let mut store = Store::open(&ws.ledger_db()).unwrap();

    store
        .put_mission(&Mission {
            mission_ref: MissionRef::parse("mission:etf-book").unwrap(),
            objective: "maintain valuations".into(),
            closure_conditions: vec![],
            created_at: t("2026-08-01T00:00:00Z"),
        })
        .unwrap();
    store
        .put_epoch(&Epoch {
            epoch_ref: rein_core::ids::EpochRef::parse("epoch:2026-08-18").unwrap(),
            mission_ref: MissionRef::parse("mission:etf-book").unwrap(),
            source_cutoff: t(CUTOFF),
            knowledge_cutoff: t(CUTOFF),
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

    let inputs = plant_captures(&ws, &mut store);
    let plan = Plan {
        plan_ref: PlanRef::parse("plan:m2@1").unwrap(),
        nodes: vec![PlanNode {
            task_ref: TaskRef::parse("task:dcf-nvda@1").unwrap(),
            depends_on: vec![],
        }],
    };
    store.put_plan(&plan).unwrap();
    store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:dcf-nvda@1").unwrap(),
            plan_ref: plan.plan_ref.clone(),
            task_type: "valuation".into(),
            output_contract: valuation_contract(),
            satisfaction_criteria: vec!["first-valid-deterministic@1".into()],
            inputs: inputs.clone(),
            universe: vec!["security:nvda".into()],
        })
        .unwrap();

    (
        Fx {
            ws,
            store,
            config,
            _ws_dir: ws_dir,
        },
        inputs,
    )
}

fn finance_engine<'a>(
    ws: &'a Workspace,
    store: &'a mut Store,
    clock: &'a FixedClock,
    config: &tempfile::TempDir,
) -> Engine<'a> {
    let captures: BTreeMap<String, CaptureRow> = store
        .list_captures()
        .unwrap()
        .into_iter()
        .map(|c| (c.digest.as_str().to_string(), c))
        .collect();
    let broker = SecretBroker::open(config.path(), &ws.root).unwrap();
    let mut engine = Engine::new(ws, store, clock, broker);
    engine
        .hands
        .register(Box::new(rein_finance::hands::FinanceDeterministic));
    register_finance_validators(
        &mut engine.validators,
        FinanceContext {
            captures,
            cas: Cas::new(ws.objects()),
            source_cutoff: t(CUTOFF),
        },
    );
    engine
}

/// §13 M2 acceptance: a valuation run on one ticker; every assumption
/// resolves to a basis; the DCF recomputes from assumptions.json alone.
#[test]
fn m2_acceptance__valuation_run_on_one_ticker_is_green_and_recomputable() {
    let (mut f, _inputs) = fixture();
    let clock = FixedClock::new(t("2026-08-17T18:00:00Z"));
    let report = {
        let mut engine = finance_engine(&f.ws, &mut f.store, &clock, &f.config);
        engine
            .run_task(
                &TaskRef::parse("task:dcf-nvda@1").unwrap(),
                Some("finance:deterministic"),
                None,
            )
            .unwrap()
    };
    let (outcome, reason) = report.outcome.clone().expect("terminal");
    assert_eq!(
        outcome,
        TerminalOutcome::Success,
        "reason: {} — validators: run `validation list`",
        reason.0
    );
    assert!(report.task_satisfied);
    assert_eq!(report.artifacts.len(), 3);

    // The committed assumptions resolve every slot to a basis, and the
    // valuation recomputes from them alone (validated in-pipeline; verified
    // again here from the read-back bytes).
    let cas = Cas::new(f.ws.objects());
    let by_name: BTreeMap<_, _> = report.artifacts.iter().cloned().collect();
    let a_bytes = cas.read_verified(&by_name["assumptions.json"]).unwrap();
    let assumptions: rein_finance::schemas::Assumptions = serde_json::from_slice(&a_bytes).unwrap();
    assert!(assumptions.slots.len() >= 12);
    let (filled, defaulted) = assumptions.coverage();
    assert!(filled >= 4, "captures actually fed slots");
    assert!(
        defaulted >= 1,
        "the defaulted slots are counted, not hidden"
    );

    let v_bytes = cas.read_verified(&by_name["valuation.json"]).unwrap();
    let valuation: rein_finance::schemas::Valuation = serde_json::from_slice(&v_bytes).unwrap();
    let (dcf_in, mut bridge_in, _market) =
        rein_finance::schemas::assemble_dcf_from_slots(&assumptions, assumptions.as_of).unwrap();
    let d = rein_finance::compute::dcf::dcf(&dcf_in).unwrap();
    bridge_in.enterprise_value = d.enterprise_value;
    let b = rein_finance::compute::bridge::bridge(&bridge_in).unwrap();
    assert!((valuation.per_share - b.per_share).abs() < 1e-9 * b.per_share.abs());
    assert!(!valuation.falsifiers.is_empty());
    assert!(valuation.sensitivity.len() >= 3);
}

/// A hallucinated basis fails closed: point one slot at a capture digest the
/// workspace has never seen and input-closure reddens the attempt.
#[test]
fn m2__input_closure_fails_a_hallucinated_basis() {
    use rein_core::receipts::{ReceiptBody, ValidatorVerdict};
    let (mut f, inputs) = fixture();
    // Remove one pinned capture from the index by re-planting the task with a
    // dangling ref appended — the hand will cite what the manifest lists.
    let mut bad_inputs = inputs.clone();
    bad_inputs.push(
        ArtifactRef::parse(
            "artifact:sha256:00000000000000000000000000000000000000000000000000000000000000aa",
        )
        .unwrap(),
    );
    // The dangling input cannot be mounted (absent from CAS): preparation
    // refuses before any hand runs — absence fails closed at the boundary.
    let plan_ref = PlanRef::parse("plan:m2@1").unwrap();
    f.store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:dcf-nvda@1").unwrap(),
            plan_ref,
            task_type: "valuation".into(),
            output_contract: valuation_contract(),
            satisfaction_criteria: vec![],
            inputs: bad_inputs,
            universe: vec!["security:nvda".into()],
        })
        .unwrap();
    let clock = FixedClock::new(t("2026-08-17T18:00:00Z"));
    let err = {
        let mut engine = finance_engine(&f.ws, &mut f.store, &clock, &f.config);
        engine
            .run_task(
                &TaskRef::parse("task:dcf-nvda@1").unwrap(),
                Some("finance:deterministic"),
                None,
            )
            .err()
    };
    assert!(
        err.is_some(),
        "a dangling pinned input must refuse to mount"
    );

    // And the validator itself reddens a basis pointing at an unknown
    // capture (direct check, invariant's own words).
    let (fx2, _) = fixture();
    let mut reg = rein_runtime::validators::ValidatorRegistry::empty();
    register_finance_validators(
        &mut reg,
        FinanceContext {
            captures: BTreeMap::new(),
            cas: Cas::new(fx2.ws.objects()),
            source_cutoff: t(CUTOFF),
        },
    );
    let assumptions = serde_json::json!({
        "schema": "rein.assumptions/v1",
        "instrument": "security:nvda",
        "as_of": CUTOFF,
        "slots": [{
            "name": "discount_rate", "value": 0.09, "unit": "rate",
            "basis": {"kind": "capture", "digest": "sha256:ffff", "field": "beta"},
            "status": "filled"
        }]
    });
    let bytes = serde_json::to_vec(&assumptions).unwrap();
    let mut all = BTreeMap::new();
    all.insert("assumptions.json".to_string(), bytes.clone());
    let artifact = rein_core::context_pack::RequiredArtifact {
        name: "assumptions.json".into(),
        media_type: "application/json".into(),
        schema_ref: None,
        min_bytes: None,
    };
    let pack_stub = pack_stub(&fx2);
    let verdict = reg.run(
        &ValidatorRef::parse("input-closure@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact,
            bytes: &bytes,
            all_artifacts: &all,
            pack: &pack_stub,
        },
    );
    assert!(
        matches!(verdict, ValidatorVerdict::Failed { ref reason } if reason.contains("hallucinated")),
        "{verdict:?}"
    );
    // Silence unused warnings for the receipts import used above.
    let _ = ReceiptBody::Transition {
        from: rein_core::state::AttemptState::Created,
        to: rein_core::state::AttemptState::Admitted,
        cause: rein_core::state::TransitionCauseRecord::Advance,
    };
}

fn pack_stub(f: &Fx) -> rein_core::context_pack::ContextPack {
    use rein_core::context_pack::*;
    ContextPack {
        schema: SCHEMA.to_string(),
        context_pack_id: rein_core::ids::ContextPackId::parse("ctx_000099").unwrap(),
        context_hash: None,
        workspace_ref: f.ws.manifest.workspace_ref.clone(),
        mission_ref: MissionRef::parse("mission:etf-book").unwrap(),
        epoch_ref: rein_core::ids::EpochRef::parse("epoch:2026-08-18").unwrap(),
        plan_ref: PlanRef::parse("plan:m2@1").unwrap(),
        task_ref: TaskRef::parse("task:dcf-nvda@1").unwrap(),
        pit_mode: PitMode::Production,
        source_cutoff: t(CUTOFF),
        knowledge_cutoff: t(CUTOFF),
        provider_pins: Default::default(),
        universe: vec![],
        inputs: vec![],
        instructions: Instructions {
            system_ref: ArtifactRef::parse(&format!(
                "artifact:{}",
                rein_core::canon::Sha256Digest::of_bytes(b"sys")
            ))
            .unwrap(),
            task_ref: ArtifactRef::parse(&format!(
                "artifact:{}",
                rein_core::canon::Sha256Digest::of_bytes(b"task")
            ))
            .unwrap(),
        },
        hand: HandSelector {
            selector: "finance:deterministic".into(),
            version_ref: rein_core::ids::HandRef::parse("hand:fake@1").unwrap(),
        },
        capabilities: Capabilities {
            filesystem: FsCaps {
                read: vec![],
                write: vec![],
            },
            network: NetworkMode::Deny,
            hand_internal_network: false,
            tools: vec![],
            secrets: vec![],
        },
        budget: Budget {
            max_steps: 8,
            per_step_timeout_ms: 240_000,
            tokens: None,
            tool_calls: None,
        },
        output_contract: valuation_contract(),
        created_at: t("2026-08-17T18:00:00Z"),
    }
}

/// Invariant 13 — PIT enforcement where it is real: eval refuses live; a
/// past-cutoff production epoch refuses live; own-CAS captures are
/// admissible only within the cutoff.
/// Symbols: `capture::ensure_live_permitted`, `capture::capture_admissible`.
#[test]
fn inv13__capture_ensure_live_permitted__past_cutoff_reads_own_cas_only() {
    let (f, _) = fixture();
    let (epoch, _) = f.store.get_epoch("epoch:2026-08-18").unwrap();

    // Cutoff 2026-08-18; "now" after it → live refused with the rule stated.
    let refused = ensure_live_permitted(&epoch, t("2026-08-19T00:00:00Z"));
    let msg = format!("{}", refused.unwrap_err());
    assert!(msg.contains("own CAS captures"), "{msg}");
    assert!(msg.contains("current-vintage"), "{msg}");

    // "now" inside the cutoff → permitted (production).
    ensure_live_permitted(&epoch, t("2026-08-17T00:00:00Z")).unwrap();

    // Eval mode always refuses live.
    let mut eval = epoch.clone();
    eval.pit_mode = PitMode::Eval;
    assert!(ensure_live_permitted(&eval, t("2020-01-01T00:00:00Z")).is_err());

    // Own-CAS admissibility rides retrieved_at ≤ cutoff.
    let rows = f.store.list_captures().unwrap();
    assert!(rows.iter().all(|r| capture_admissible(r, &epoch)));
    let mut late = rows[0].clone();
    late.retrieved_at = t("2026-08-19T00:00:00Z");
    assert!(!capture_admissible(&late, &epoch));
}

/// Invariant 16 — a tool that cannot stamp as-of refuses.
/// Symbol: `datum::Stamped::new`.
#[test]
fn inv16__datum_stamped_new__refuses_unstampable_figures() {
    let refused = Stamped::new(
        "data.equity.profile",
        "NVDA.beta",
        1.1,
        "ratio",
        None,
        "FMP",
        t(RETRIEVED),
        None,
    );
    let msg = format!("{}", refused.unwrap_err());
    assert!(msg.contains("refusing rather than returning a bare number"));

    let ok = Stamped::new(
        "data.equity.quote",
        "NVDA.price",
        182.5,
        "ccy/share",
        Some((t("2026-08-17T00:00:00Z"), AsOfBasis::Provider)),
        "FMP",
        t(RETRIEVED),
        None,
    )
    .unwrap();
    assert_eq!(ok.as_of_basis, AsOfBasis::Provider);
}

/// Invariant 14 — the 2027-claim class: a post-cutoff time stated as fact
/// fails validation. Symbol: finance validator `fact-vs-forecast@1`.
#[test]
fn inv14__fact_vs_forecast__post_cutoff_fact_fails() {
    use rein_core::receipts::ValidatorVerdict;
    let (f, _) = fixture();
    let mut reg = rein_runtime::validators::ValidatorRegistry::empty();
    register_finance_validators(
        &mut reg,
        FinanceContext {
            captures: BTreeMap::new(),
            cas: Cas::new(f.ws.objects()),
            source_cutoff: t(CUTOFF),
        },
    );
    let claims = Claims {
        schema: CLAIMS_SCHEMA.into(),
        claims: vec![Claim {
            id: "c1".into(),
            text: "Data-center revenue reaches $400B in 2027.".into(),
            kind: ClaimKind::Fact,
            about_time: Some(t("2027-06-30T00:00:00Z")),
            evidence: vec![1],
            falsifier: Some("2027 revenue below $400B".into()),
        }],
        citations: vec![],
        coverage: Default::default(),
    };
    let bytes = serde_json::to_vec(&claims).unwrap();
    let artifact = rein_core::context_pack::RequiredArtifact {
        name: "claims.json".into(),
        media_type: "application/json".into(),
        schema_ref: None,
        min_bytes: None,
    };
    let all = BTreeMap::new();
    let pack = pack_stub(&f);
    let verdict = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact,
            bytes: &bytes,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(verdict, ValidatorVerdict::Failed { ref reason } if reason.contains("2027-claim")),
        "{verdict:?}"
    );

    // Prose face: a bare post-cutoff year in memo prose fails; marked
    // forecast lines pass.
    let memo_bad = b"Revenue will be huge in 2027.".to_vec();
    let artifact_md = rein_core::context_pack::RequiredArtifact {
        name: "memo.md".into(),
        media_type: "text/markdown".into(),
        schema_ref: None,
        min_bytes: None,
    };
    let v_bad = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_bad,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(matches!(v_bad, ValidatorVerdict::Failed { .. }));
    let memo_ok = b"We forecast strong growth into 2027.".to_vec();
    let v_ok = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_ok,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(matches!(v_ok, ValidatorVerdict::Passed));
    // Historicity face: fiscal labels run ahead of the calendar — a
    // fiscal-2027 quarter already reported before the cutoff is history,
    // not the 2027-claim class, when the sentence says so.
    let memo_reported = b"Q1 FY2027 revenue was $44.1B, reported April 2026 [1].".to_vec();
    let v_reported = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_reported,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(v_reported, ValidatorVerdict::Passed),
        "{v_reported:?}"
    );
    // Fiscal-label face: a cited fiscal quarter one year ahead of the
    // calendar is a filed quarter, not the 2027-claim class …
    let memo_fiscal = b"Gross margin rebounded to 74.93% in Q1 FY2027 [3].".to_vec();
    let v_fiscal = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_fiscal,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(matches!(v_fiscal, ValidatorVerdict::Passed), "{v_fiscal:?}");
    // … but only that exact shape: an uncited fiscal line still marks,
    let memo_uncited = b"Margins recover in Q2 FY2027.".to_vec();
    let v_uncited = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_uncited,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(v_uncited, ValidatorVerdict::Failed { .. }),
        "{v_uncited:?}"
    );
    // and a further-out fiscal year is future no matter the dressing.
    let memo_far = b"We see 80% margins in Q1 FY2029 [3].".to_vec();
    let v_far = reg.run(
        &ValidatorRef::parse("fact-vs-forecast@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact_md,
            bytes: &memo_far,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(v_far, ValidatorVerdict::Failed { .. }),
        "{v_far:?}"
    );
}

/// Invariants 17/18 — a citation resolves to captured bytes or fails; a word
/// in brackets is not a citation. Symbol: finance validator
/// `citation-closure@1`.
#[test]
fn inv17_18__citation_closure__uncaptured_sources_fail() {
    use rein_core::receipts::ValidatorVerdict;
    let (f, inputs) = fixture();
    let captures: BTreeMap<String, CaptureRow> = f
        .store
        .list_captures()
        .unwrap()
        .into_iter()
        .map(|c| (c.digest.as_str().to_string(), c))
        .collect();
    let real_digest = inputs[0]
        .as_str()
        .trim_start_matches("artifact:")
        .to_string();
    let mut reg = rein_runtime::validators::ValidatorRegistry::empty();
    register_finance_validators(
        &mut reg,
        FinanceContext {
            captures,
            cas: Cas::new(f.ws.objects()),
            source_cutoff: t(CUTOFF),
        },
    );

    let claims_ok = Claims {
        schema: CLAIMS_SCHEMA.into(),
        claims: vec![],
        citations: vec![rein_finance::schemas::Citation {
            n: 1,
            source_digest: real_digest,
            locator: "quote row".into(),
        }],
        coverage: Default::default(),
    };
    let dossier =
        b"NVDA trades at 182.5 [1]. A bracketed [search] word is not a citation.".to_vec();
    let artifact = rein_core::context_pack::RequiredArtifact {
        name: "dossier.md".into(),
        media_type: "text/markdown".into(),
        schema_ref: None,
        min_bytes: None,
    };
    let mut all = BTreeMap::new();
    all.insert(
        "claims.json".to_string(),
        serde_json::to_vec(&claims_ok).unwrap(),
    );
    let pack = pack_stub(&f);
    let ok = reg.run(
        &ValidatorRef::parse("citation-closure@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact,
            bytes: &dossier,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(matches!(ok, ValidatorVerdict::Passed), "{ok:?}");

    // An uncaptured citation fails: the claims cite a digest nobody captured.
    let mut claims_bad = claims_ok.clone();
    claims_bad.citations[0].source_digest =
        "sha256:00000000000000000000000000000000000000000000000000000000000000bb".into();
    all.insert(
        "claims.json".to_string(),
        serde_json::to_vec(&claims_bad).unwrap(),
    );
    let bad = reg.run(
        &ValidatorRef::parse("citation-closure@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact,
            bytes: &dossier,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(bad, ValidatorVerdict::Failed { ref reason } if reason.contains("not evidence until its bytes are captured")),
        "{bad:?}"
    );
}

/// Invariant 19 — captures per host are capped; the cap refuses with words.
/// Symbol: `capture::CaptureStore::capture_page` (MAX_CAPTURES_PER_HOST).
#[test]
fn inv19__capture_page__host_cap_refuses_syndication_as_corroboration() {
    let (mut f, _) = fixture();
    let (epoch, _) = f.store.get_epoch("epoch:2026-08-18").unwrap();
    let cas = Cas::new(f.ws.objects());
    let mut cs = CaptureStore::new(&mut f.store, cas);
    let now = t("2026-08-17T00:00:00Z");
    for i in 0..rein_finance::capture::MAX_CAPTURES_PER_HOST {
        cs.capture_page(
            &format!("https://example.com/story-{i}"),
            format!("body {i}").as_bytes(),
            "text/html",
            &epoch,
            now,
        )
        .unwrap();
    }
    let over = cs.capture_page(
        "https://example.com/story-extra",
        b"one more",
        "text/html",
        &epoch,
        now,
    );
    let msg = format!("{}", over.unwrap_err());
    assert!(msg.contains("syndication is not corroboration"), "{msg}");
}

/// Invariant 20 — declared denominators must add up; drops carry reasons.
/// Symbol: finance validator `coverage-denominator@1`.
#[test]
fn inv20__coverage_denominator__silent_truncation_fails() {
    use rein_core::receipts::ValidatorVerdict;
    let (f, _) = fixture();
    let mut reg = rein_runtime::validators::ValidatorRegistry::empty();
    register_finance_validators(
        &mut reg,
        FinanceContext {
            captures: BTreeMap::new(),
            cas: Cas::new(f.ws.objects()),
            source_cutoff: t(CUTOFF),
        },
    );
    let claims = Claims {
        schema: CLAIMS_SCHEMA.into(),
        claims: vec![],
        citations: vec![],
        coverage: rein_finance::schemas::ResearchCoverage {
            eligible_inputs: 3,
            consumed: vec!["input-00".into()],
            withheld: vec![],
            hosts: Default::default(),
        },
    };
    let bytes = serde_json::to_vec(&claims).unwrap();
    let artifact = rein_core::context_pack::RequiredArtifact {
        name: "claims.json".into(),
        media_type: "application/json".into(),
        schema_ref: None,
        min_bytes: None,
    };
    let all = BTreeMap::new();
    let pack = pack_stub(&f);
    let v = reg.run(
        &ValidatorRef::parse("coverage-denominator@1").unwrap(),
        &rein_runtime::validators::ValidationInput {
            artifact: &artifact,
            bytes: &bytes,
            all_artifacts: &all,
            pack: &pack,
        },
    );
    assert!(
        matches!(v, ValidatorVerdict::Failed { ref reason } if reason.contains("silent truncation")),
        "{v:?}"
    );
}

/// Invariant 21 — no falsifier → non_settleable_missing_falsifier, barred
/// from decision-ready. Symbol: `schemas::claim_admissible`.
#[test]
fn inv21__schemas_claim_admissible__missing_falsifier_is_not_decision_ready() {
    let mut c = Claim {
        id: "c1".into(),
        text: "x".into(),
        kind: ClaimKind::Forecast,
        about_time: None,
        evidence: vec![],
        falsifier: None,
    };
    assert_eq!(
        claim_admissible(&c).unwrap_err(),
        "non_settleable_missing_falsifier"
    );
    c.falsifier = Some("breaks if X by D".into());
    claim_admissible(&c).unwrap();
}

/// Invariants 11 + 26 — the agy adapter: absolute path resolution, a single
/// attempt recorded, and survival under `env -i`-grade environments.
/// Symbols: `hands::AgyHand::resolve`, `hands::AgyHand` (attempts: 1).
#[test]
fn inv11_26__agy_hand__absolute_path_single_attempt_env_i() {
    use rein_core::hand::HandEvent;
    // A stub agy: prints a well-formed envelope and exits 0. No network, no
    // real model — the adapter contract is what's under test.
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("agy-stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\nprintf '{\"status\":\"SUCCESS\",\"response\":\"stub says hello\"}'\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // Relative, non-PATH name refuses; absolute path resolves.
    assert!(rein_finance::hands::AgyHand::resolve(
        "definitely-not-on-path-xyz",
        "m",
        dir.path().join("ws")
    )
    .is_err());
    let hand = rein_finance::hands::AgyHand::resolve(
        stub.to_str().unwrap(),
        "gemini-3.6-flash",
        dir.path().join("ws"),
    )
    .unwrap();
    assert!(hand.binary.is_absolute(), "invariant 26: absolute path");

    // Run it under a scrubbed environment (env -i shape): only PATH.
    let out_dir = dir.path().join("out");
    let in_dir = dir.path().join("in");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::create_dir_all(&in_dir).unwrap();
    let mut ids = rein_core::ids::IdGen::new();
    let request = rein_core::hand::HandRequest {
        attempt_id: rein_core::ids::AttemptId::parse("attempt_000001").unwrap(),
        run_id: ids.run(),
        fence_generation: 1,
        sequence: 0,
        idempotency_key: rein_core::idempotency::IdempotencyKey::derive(
            &TaskRef::parse("task:t@1").unwrap(),
            &rein_core::canon::Sha256Digest::of_bytes(b"x"),
            1,
        ),
        capability_ref: rein_core::ids::GrantId::parse("grant_t").unwrap(),
        trace: ids.trace(),
        deadline: rein_core::time::LogicalMs(60_000),
        internal_retries_disabled: true,
    };
    let mut env = BTreeMap::new();
    env.insert(
        "PATH".to_string(),
        "/usr/bin:/bin:/usr/sbin:/sbin".to_string(),
    );
    let contract = valuation_contract();
    let budget = rein_core::context_pack::Budget {
        max_steps: 4,
        per_step_timeout_ms: 60_000,
        tokens: None,
        tool_calls: None,
    };
    use rein_runtime::hands::RuntimeHand;
    let out = hand
        .run(&rein_runtime::hands::HandContext {
            request: &request,
            contract: &contract,
            budget: &budget,
            inputs_dir: &in_dir,
            output_dir: &out_dir,
            env: &env,
        })
        .unwrap();

    // Exactly one attempt, recorded (invariant 11).
    let attempts = out.events.iter().find_map(|e| match &e.event {
        HandEvent::RunStarted { attempts, .. } => Some(*attempts),
        _ => None,
    });
    assert_eq!(
        attempts,
        Some(1),
        "internal retries disabled by construction"
    );
    // The envelope was strict-decoded; the run completed with the stub's exit.
    assert!(out.events.iter().any(|e| matches!(
        e.event,
        HandEvent::RunCompleted {
            child_exit: Some(0)
        }
    )));
}

/// Invariant 12 — a satisfied task is never re-run by the plan sweep: the
/// resume semantics ride selection receipts, not memory.
/// Symbol: `selection::task_satisfied` driving the sweep.
#[test]
fn inv12__plan_sweep_resume__satisfied_tasks_are_skipped() {
    let (mut f, _) = fixture();
    let clock = FixedClock::new(t("2026-08-17T18:00:00Z"));
    let task = TaskRef::parse("task:dcf-nvda@1").unwrap();
    {
        let mut engine = finance_engine(&f.ws, &mut f.store, &clock, &f.config);
        engine
            .run_task(&task, Some("finance:deterministic"), None)
            .unwrap();
    }
    let attempts_before = f.store.list_attempts().unwrap().len();
    let log = f.store.load_full_log().unwrap();
    assert!(rein_core::selection::task_satisfied(&log, &task));
    // The sweep predicate: a satisfied task is not pending. Nothing to run.
    let plan = f.store.get_plan("plan:m2@1").unwrap();
    let pending: Vec<_> = plan
        .nodes
        .iter()
        .filter(|n| !rein_core::selection::task_satisfied(&log, &n.task_ref))
        .collect();
    assert!(pending.is_empty());
    assert_eq!(f.store.list_attempts().unwrap().len(), attempts_before);
}

/// Invariant 29 — absence is never permission: only the grant's named secret
/// refs are injected. Symbol: `workspace::SecretBroker::env_for`.
#[test]
fn inv29__secretbroker_env_for__absence_is_never_permission() {
    let config = tempfile::tempdir().unwrap();
    std::fs::write(
        config.path().join("secrets.toml"),
        "granted = \"g-value\"\nungranted = \"u-value\"\n",
    )
    .unwrap();
    let ws_dir = tempfile::tempdir().unwrap();
    let broker = SecretBroker::open(config.path(), ws_dir.path()).unwrap();
    let env = broker.env_for(&[rein_core::ids::SecretRefId::parse("secret-ref:granted").unwrap()]);
    assert_eq!(env.get("GRANTED").map(String::as_str), Some("g-value"));
    assert!(
        !env.values().any(|v| v == "u-value"),
        "an ungranted secret must never be injected"
    );
    // And the grant schema cannot express transfer or non-expiry: compile
    // shape (no delegation field; expires_at mandatory) pinned in core.
    let _grant = rein_core::entities::CapabilityGrant {
        grant_id: rein_core::ids::GrantId::parse("grant_x").unwrap(),
        subject: rein_core::ids::HandRef::parse("hand:agy@1").unwrap(),
        capabilities: pack_stub(&fixture().0).capabilities,
        issued_at: t("2026-08-17T00:00:00Z"),
        expires_at: t("2026-08-18T00:00:00Z"),
    };
}

/// Invariant 10 — the per-step budget names the guilty step through the real
/// engine (budget receipt with the step number).
/// Symbols: `hand::per_step_breach` + engine Budget receipt.
#[test]
fn inv10__engine_budget__per_step_breach_names_the_step() {
    use rein_core::receipts::{BudgetScope, BudgetVerdict, ReceiptBody};
    let (mut f, _) = fixture();
    // The timeout fixture needs the M1-shape contract: repoint the task.
    f.store
        .put_task(&TaskVersion {
            task_ref: TaskRef::parse("task:dcf-nvda@1").unwrap(),
            plan_ref: PlanRef::parse("plan:m2@1").unwrap(),
            task_type: "fixture".into(),
            output_contract: rein_core::context_pack::OutputContract {
                required_artifacts: vec![rein_core::context_pack::RequiredArtifact {
                    name: "valuation.json".into(),
                    media_type: "application/json".into(),
                    schema_ref: None,
                    min_bytes: None,
                }],
                validators: vec![ValidatorRef::parse("artifact-wellformed@1").unwrap()],
            },
            satisfaction_criteria: vec![],
            inputs: vec![],
            universe: vec![],
        })
        .unwrap();
    let clock = FixedClock::new(t("2026-08-17T18:00:00Z"));
    let report = {
        let mut engine = finance_engine(&f.ws, &mut f.store, &clock, &f.config);
        engine
            .run_task(
                &TaskRef::parse("task:dcf-nvda@1").unwrap(),
                Some("fake:timeout"),
                None,
            )
            .unwrap()
    };
    let (outcome, reason) = report.outcome.unwrap();
    assert_eq!(outcome, TerminalOutcome::TimedOut);
    assert_eq!(reason.0, "per_step_budget_exceeded");
    let log = f.store.load_attempt_log(&report.attempt_id).unwrap();
    let named = log.for_attempt(&report.attempt_id).any(|e| {
        matches!(
            &e.body,
            ReceiptBody::Budget {
                scope: BudgetScope::Step { step: 1 },
                verdict: BudgetVerdict::Exceeded,
                ..
            }
        )
    });
    assert!(named, "the budget buys attribution: step 1 is named");
}

/// The FMP client against a local fixture server: provider-time stamping and
/// the capture path, no network.
#[test]
fn fmp_client__stamps_provider_time_and_captures_raw_bytes() {
    // Minimal single-shot HTTP fixture server.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = r#"[{"symbol":"NVDA","price":182.5,"sharesOutstanding":24400000000.0,"timestamp":1787086800}]"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-api-version: v4-stable\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let handle = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = sock.read(&mut buf);
            let _ = sock.write_all(response.as_bytes());
        }
    });

    let (mut f, _) = fixture();
    let (epoch, _) = f.store.get_epoch("epoch:2026-08-18").unwrap();
    let client = FmpClient::with_key_and_root("test-key", format!("http://{addr}")).unwrap();
    let cas = Cas::new(f.ws.objects());
    let mut cs = CaptureStore::new(&mut f.store, cas);
    let result = cs
        .pull_equity(
            &client,
            EquityEndpoint::Quote,
            "NVDA",
            &epoch,
            t("2026-08-17T00:00:00Z"),
        )
        .unwrap();
    handle.join().unwrap();

    assert_eq!(result.served_version.as_deref(), Some("v4-stable"));
    assert_eq!(
        result.rows.len(),
        2,
        "price + shares (no marketCap in fixture)"
    );
    let price = result
        .rows
        .iter()
        .find(|r| r.name.ends_with("price"))
        .unwrap();
    assert_eq!(price.as_of_basis, AsOfBasis::Provider);
    // 1787086800s = 2026-08-18T21:00:00Z — the provider's time, not ours.
    assert_eq!(price.as_of.canonical(), "2026-08-18T21:00:00Z");
    // The raw bytes are in the CAS and the capture index.
    let row = f
        .store
        .get_capture(result.digest.as_str())
        .unwrap()
        .expect("captured");
    assert_eq!(row.tool, "data.equity.quote");
    Cas::new(f.ws.objects()).verify(&result.digest).unwrap();
}
