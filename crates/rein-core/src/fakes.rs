//! Fake Hands (§6): first-class conformance fixtures — the failure matrix is
//! executable before any real model is wired. Nine adopted from the PDF's ten
//! (`fake:lease-expiry` deferred with the lease service, §12) plus
//! `fake:cjk-splitter`, a Rein addition for invariant 30.
//!
//! Fixtures are *pure*: same request, same output. `deterministic-a` and
//! `deterministic-b` differ in event chatter but produce byte-identical
//! artifacts from the same ContextPack — M1's acceptance rides on exactly
//! this.

use crate::canon::Sha256Digest;
use crate::capture::StdStream;
use crate::context_pack::{Budget, OutputContract};
use crate::hand::{HandEvent, HandRequest, ModelIdentity, SelfClaim, SequencedEvent};
use crate::time::LogicalMs;
use std::collections::BTreeMap;

/// The secret *value* planted by `fake:secret-leak`. Tests register it with a
/// [`crate::secretref::Redactor`] under `secret-ref:fixture`.
pub const FIXTURE_SECRET_VALUE: &str = "sk_fixture_0deadbeef_do_not_ship";

/// CJK text emitted by `fake:cjk-splitter`.
pub const CJK_TEXT: &str = "研究收益率曲线：数据完整性优先，宁缺毋滥。";

#[derive(Debug, Clone, PartialEq)]
pub struct FakeRunOutput {
    pub events: Vec<SequencedEvent>,
    /// Artifacts as staged by the writer.
    pub staged: BTreeMap<String, Vec<u8>>,
    /// What the hand *claims* each artifact hashes to.
    pub claimed: BTreeMap<String, Sha256Digest>,
}

pub trait FakeHand {
    fn name(&self) -> &'static str;
    fn run(&self, req: &HandRequest, contract: &OutputContract, budget: &Budget) -> FakeRunOutput;
}

/// Deterministic artifact bytes: a pure function of (task, context-hash,
/// artifact) shared by both deterministic fixtures. The attempt *generation*
/// is deliberately stripped — an operational retry under the same ContextPack
/// must reproduce identical digests (M1 acceptance; invariants 6 and 23).
pub fn deterministic_artifact_bytes(req: &HandRequest, artifact_name: &str) -> Vec<u8> {
    let key = req.idempotency_key.as_str();
    let semantic = key.rsplit_once("/gen:").map_or(key, |(head, _)| head);
    // Media-type honest: a `.json` artifact stages valid JSON, so the
    // well-formedness validator judges content, not the fixture's laziness.
    if artifact_name.ends_with(".json") {
        format!(
            "{{\"schema\":\"rein.fixture/v1\",\"key\":\"{semantic}\",\"artifact\":\"{artifact_name}\"}}\n"
        )
        .into_bytes()
    } else {
        format!("REIN-M0-DETERMINISTIC\nkey={semantic}\nartifact={artifact_name}\n").into_bytes()
    }
}

fn identity(name: &str) -> ModelIdentity {
    ModelIdentity {
        requested: format!("fake:{name}"),
        served: format!("fake:{name}"),
    }
}

struct EventBuilder {
    run_id: crate::ids::RunId,
    seq: u64,
    events: Vec<SequencedEvent>,
}

impl EventBuilder {
    fn new(req: &HandRequest) -> Self {
        Self {
            run_id: req.run_id.clone(),
            seq: 0,
            events: Vec::new(),
        }
    }

    fn push(&mut self, at: u64, event: HandEvent) -> &mut Self {
        self.events.push(SequencedEvent {
            run_id: self.run_id.clone(),
            seq: self.seq,
            at: LogicalMs(at),
            event,
        });
        self.seq += 1;
        self
    }

    /// Re-emit the previous event verbatim (same sequence number) — the
    /// duplicate-callback shape.
    fn duplicate_last(&mut self) -> &mut Self {
        let last = self.events.last().expect("duplicate of nothing").clone();
        self.events.push(last);
        self
    }
}

fn stage_all(
    req: &HandRequest,
    contract: &OutputContract,
) -> (BTreeMap<String, Vec<u8>>, BTreeMap<String, Sha256Digest>) {
    let mut staged = BTreeMap::new();
    let mut claimed = BTreeMap::new();
    for a in &contract.required_artifacts {
        let bytes = deterministic_artifact_bytes(req, &a.name);
        claimed.insert(a.name.clone(), Sha256Digest::of_bytes(&bytes));
        staged.insert(a.name.clone(), bytes);
    }
    (staged, claimed)
}

macro_rules! fixture {
    ($ty:ident, $name:literal, |$req:ident, $contract:ident, $budget:ident| $body:block) => {
        pub struct $ty;
        impl FakeHand for $ty {
            fn name(&self) -> &'static str {
                $name
            }
            #[allow(unused_variables)]
            fn run(
                &self,
                $req: &HandRequest,
                $contract: &OutputContract,
                $budget: &Budget,
            ) -> FakeRunOutput {
                $body
            }
        }
    };
}

fixture!(
    DeterministicA,
    "fake:deterministic-a",
    |req, contract, budget| {
        let (staged, claimed) = stage_all(req, contract);
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("deterministic-a"),
                attempts: 1,
            },
        );
        b.push(1, HandEvent::StepStarted { step: 1 });
        for (name, digest) in &claimed {
            b.push(
                2,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        b.push(3, HandEvent::StepCompleted { step: 1 });
        b.push(
            4,
            HandEvent::SelfReport {
                claim: SelfClaim::Success,
            },
        );
        b.push(
            5,
            HandEvent::RunCompleted {
                child_exit: Some(0),
            },
        );
        FakeRunOutput {
            events: b.events,
            staged,
            claimed,
        }
    }
);

fixture!(
    DeterministicB,
    "fake:deterministic-b",
    |req, contract, budget| {
        // Same artifact bytes; different process shape (more chatter, other
        // timings) — digests must not care.
        let (staged, claimed) = stage_all(req, contract);
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("deterministic-b"),
                attempts: 1,
            },
        );
        b.push(7, HandEvent::StepStarted { step: 1 });
        b.push(
            9,
            HandEvent::OutputChunk {
                stream: StdStream::Stdout,
                bytes: b"thinking differently\n".to_vec(),
            },
        );
        b.push(11, HandEvent::StepCompleted { step: 1 });
        b.push(12, HandEvent::StepStarted { step: 2 });
        for (name, digest) in &claimed {
            b.push(
                13,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        b.push(14, HandEvent::StepCompleted { step: 2 });
        b.push(
            15,
            HandEvent::RunCompleted {
                child_exit: Some(0),
            },
        );
        FakeRunOutput {
            events: b.events,
            staged,
            claimed,
        }
    }
);

fixture!(Exit0Empty, "fake:exit0-empty", |req, contract, budget| {
    // The founding failure shape: green exit, confident self-report, nothing
    // produced (§6 matrix row 3; the 65.6% case).
    let mut b = EventBuilder::new(req);
    b.push(
        0,
        HandEvent::RunStarted {
            identity: identity("exit0-empty"),
            attempts: 1,
        },
    );
    b.push(
        1,
        HandEvent::SelfReport {
            claim: SelfClaim::Success,
        },
    );
    b.push(
        2,
        HandEvent::RunCompleted {
            child_exit: Some(0),
        },
    );
    FakeRunOutput {
        events: b.events,
        staged: BTreeMap::new(),
        claimed: BTreeMap::new(),
    }
});

fixture!(
    HashMismatch,
    "fake:hash-mismatch",
    |req, contract, budget| {
        let (staged, mut claimed) = stage_all(req, contract);
        // Claim a digest that the staged bytes do not have.
        for digest in claimed.values_mut() {
            *digest = Sha256Digest::of_bytes(b"bytes the writer wishes it had staged");
        }
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("hash-mismatch"),
                attempts: 1,
            },
        );
        for (name, digest) in &claimed {
            b.push(
                1,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        b.push(
            2,
            HandEvent::RunCompleted {
                child_exit: Some(0),
            },
        );
        FakeRunOutput {
            events: b.events,
            staged,
            claimed,
        }
    }
);

fixture!(
    DuplicateCallback,
    "fake:duplicate-callback",
    |req, contract, budget| {
        let (staged, claimed) = stage_all(req, contract);
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("duplicate-callback"),
                attempts: 1,
            },
        );
        for (name, digest) in &claimed {
            b.push(
                1,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        b.push(
            2,
            HandEvent::RunCompleted {
                child_exit: Some(0),
            },
        );
        b.duplicate_last(); // same seq, same payload: must be idempotent
        FakeRunOutput {
            events: b.events,
            staged,
            claimed,
        }
    }
);

fixture!(TimeoutFake, "fake:timeout", |req, contract, budget| {
    // Step 1 completes one millisecond past its per-step budget.
    let over = budget.per_step_timeout_ms + 1;
    let mut b = EventBuilder::new(req);
    b.push(
        0,
        HandEvent::RunStarted {
            identity: identity("timeout"),
            attempts: 1,
        },
    );
    b.push(1, HandEvent::StepStarted { step: 1 });
    b.push(1 + over, HandEvent::StepCompleted { step: 1 });
    b.push(2 + over, HandEvent::RunCompleted { child_exit: None });
    FakeRunOutput {
        events: b.events,
        staged: BTreeMap::new(),
        claimed: BTreeMap::new(),
    }
});

fixture!(SecretLeak, "fake:secret-leak", |req, contract, budget| {
    let (mut staged, mut claimed) = stage_all(req, contract);
    if let Some(a) = contract.required_artifacts.first() {
        let bytes = format!("api key is {FIXTURE_SECRET_VALUE}\n").into_bytes();
        claimed.insert(a.name.clone(), Sha256Digest::of_bytes(&bytes));
        staged.insert(a.name.clone(), bytes);
    }
    let mut b = EventBuilder::new(req);
    b.push(
        0,
        HandEvent::RunStarted {
            identity: identity("secret-leak"),
            attempts: 1,
        },
    );
    b.push(
        1,
        HandEvent::OutputChunk {
            stream: StdStream::Stdout,
            bytes: format!("using {FIXTURE_SECRET_VALUE} for auth\n").into_bytes(),
        },
    );
    b.push(
        2,
        HandEvent::RunCompleted {
            child_exit: Some(0),
        },
    );
    FakeRunOutput {
        events: b.events,
        staged,
        claimed,
    }
});

fixture!(
    PartialOutput,
    "fake:partial-output",
    |req, contract, budget| {
        let (mut staged, mut claimed) = stage_all(req, contract);
        // Drop everything but the first required artifact.
        if let Some(keep) = contract.required_artifacts.first().map(|a| a.name.clone()) {
            staged.retain(|k, _| *k == keep);
            claimed.retain(|k, _| *k == keep);
        }
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("partial-output"),
                attempts: 1,
            },
        );
        for (name, digest) in &claimed {
            b.push(
                1,
                HandEvent::ArtifactDeclared {
                    name: name.clone(),
                    claimed_digest: digest.clone(),
                },
            );
        }
        b.push(
            2,
            HandEvent::RunCompleted {
                child_exit: Some(0),
            },
        );
        FakeRunOutput {
            events: b.events,
            staged,
            claimed,
        }
    }
);

fixture!(
    UnknownAfterDisconnect,
    "fake:unknown-after-disconnect",
    |req, contract, budget| {
        let mut b = EventBuilder::new(req);
        b.push(
            0,
            HandEvent::RunStarted {
                identity: identity("unknown-after-disconnect"),
                attempts: 1,
            },
        );
        b.push(1, HandEvent::StepStarted { step: 1 });
        b.push(2, HandEvent::Disconnected);
        // No RunCompleted, no artifacts: nothing to infer a verdict from.
        FakeRunOutput {
            events: b.events,
            staged: BTreeMap::new(),
            claimed: BTreeMap::new(),
        }
    }
);

fixture!(CjkSplitter, "fake:cjk-splitter", |req, contract, budget| {
    // Emit CJK output split at pathological byte boundaries (mid-character),
    // then complete normally with deterministic artifacts (invariant 30; §6
    // matrix row: captured stdout byte-identical to emitted bytes).
    let (staged, claimed) = stage_all(req, contract);
    let bytes = CJK_TEXT.as_bytes();
    let mut b = EventBuilder::new(req);
    b.push(
        0,
        HandEvent::RunStarted {
            identity: identity("cjk-splitter"),
            attempts: 1,
        },
    );
    // Chunk sizes chosen to land inside multi-byte sequences: 1, 2, 4, 5, …
    let mut off = 0usize;
    let mut size = 1usize;
    let mut at = 1u64;
    while off < bytes.len() {
        let end = (off + size).min(bytes.len());
        b.push(
            at,
            HandEvent::OutputChunk {
                stream: StdStream::Stdout,
                bytes: bytes[off..end].to_vec(),
            },
        );
        off = end;
        size = if size >= 5 { 1 } else { size + 1 };
        at += 1;
    }
    for (name, digest) in &claimed {
        b.push(
            at,
            HandEvent::ArtifactDeclared {
                name: name.clone(),
                claimed_digest: digest.clone(),
            },
        );
    }
    b.push(
        at + 1,
        HandEvent::RunCompleted {
            child_exit: Some(0),
        },
    );
    FakeRunOutput {
        events: b.events,
        staged,
        claimed,
    }
});

/// All ten fixtures (§6's nine + the Rein addition).
pub fn all_fixtures() -> Vec<Box<dyn FakeHand>> {
    vec![
        Box::new(DeterministicA),
        Box::new(DeterministicB),
        Box::new(Exit0Empty),
        Box::new(HashMismatch),
        Box::new(DuplicateCallback),
        Box::new(TimeoutFake),
        Box::new(SecretLeak),
        Box::new(PartialOutput),
        Box::new(UnknownAfterDisconnect),
        Box::new(CjkSplitter),
    ]
}
