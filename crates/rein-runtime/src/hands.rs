//! Runtime hands: the executors the engine can bind (C2 amendment: an
//! execution binding, recorded in receipts, outside the semantic hash).
//!
//! M1 ships the ten M0 conformance fixtures as runtime hands — they write
//! their staged artifacts into the sandbox output directory like any real
//! hand would, so the commit path exercises real files. Real model hands
//! (agy) arrive at M2 behind the same trait, constructed with internal
//! retries disabled (invariant 11).

use rein_core::canon::Sha256Digest;
use rein_core::context_pack::{Budget, OutputContract};
use rein_core::fakes::{self, FakeHand};
use rein_core::hand::{HandRequest, SequencedEvent};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum HandError {
    #[error("no hand named `{0}` is installed")]
    Unknown(String),
    #[error("hand io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("hand `{hand}` failed: {detail}")]
    Failed { hand: String, detail: String },
}

/// What a hand run leaves behind for the pipeline.
pub struct HandRunOutcome {
    pub events: Vec<SequencedEvent>,
    /// Digests the hand *claims* per artifact (evidence, not truth).
    pub claimed: BTreeMap<String, Sha256Digest>,
}

pub struct HandContext<'a> {
    pub request: &'a HandRequest,
    pub contract: &'a OutputContract,
    pub budget: &'a Budget,
    pub inputs_dir: &'a Path,
    pub output_dir: &'a Path,
    /// Environment for real subprocess hands (secret injection at the
    /// narrowest boundary). Fakes ignore it.
    pub env: &'a BTreeMap<String, String>,
}

pub trait RuntimeHand: Send + Sync {
    fn selector(&self) -> &str;
    /// Run once. Internal retries are disabled by construction; the run
    /// record carries `attempts` in the RunStarted event (invariant 11).
    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError>;
}

/// Adapter: an M0 fixture as a runtime hand that writes real files.
struct FixtureHand<F: FakeHand> {
    name: &'static str,
    inner: F,
}

impl<F: FakeHand + Send + Sync> RuntimeHand for FixtureHand<F> {
    fn selector(&self) -> &str {
        self.name
    }

    fn run(&self, ctx: &HandContext<'_>) -> Result<HandRunOutcome, HandError> {
        let out = self.inner.run(ctx.request, ctx.contract, ctx.budget);
        for (name, bytes) in &out.staged {
            let path = ctx.output_dir.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|source| HandError::Io {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            std::fs::write(&path, bytes).map_err(|source| HandError::Io { path, source })?;
        }
        Ok(HandRunOutcome {
            events: out.events,
            claimed: out.claimed,
        })
    }
}

pub struct HandRegistry {
    hands: Vec<Box<dyn RuntimeHand>>,
}

impl Default for HandRegistry {
    fn default() -> Self {
        Self::with_fixtures()
    }
}

impl HandRegistry {
    pub fn empty() -> Self {
        Self { hands: Vec::new() }
    }

    /// All ten M0 fixtures, under their `fake:` selectors.
    pub fn with_fixtures() -> Self {
        let mut r = Self::empty();
        r.register(Box::new(FixtureHand {
            name: "fake:deterministic-a",
            inner: fakes::DeterministicA,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:deterministic-b",
            inner: fakes::DeterministicB,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:exit0-empty",
            inner: fakes::Exit0Empty,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:hash-mismatch",
            inner: fakes::HashMismatch,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:duplicate-callback",
            inner: fakes::DuplicateCallback,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:timeout",
            inner: fakes::TimeoutFake,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:secret-leak",
            inner: fakes::SecretLeak,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:partial-output",
            inner: fakes::PartialOutput,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:unknown-after-disconnect",
            inner: fakes::UnknownAfterDisconnect,
        }));
        r.register(Box::new(FixtureHand {
            name: "fake:cjk-splitter",
            inner: fakes::CjkSplitter,
        }));
        r
    }

    pub fn register(&mut self, hand: Box<dyn RuntimeHand>) {
        self.hands.push(hand);
    }

    pub fn get(&self, selector: &str) -> Result<&dyn RuntimeHand, HandError> {
        self.hands
            .iter()
            .map(|h| h.as_ref())
            .find(|h| h.selector() == selector)
            .ok_or_else(|| HandError::Unknown(selector.to_string()))
    }

    pub fn selectors(&self) -> Vec<&str> {
        self.hands.iter().map(|h| h.selector()).collect()
    }
}
