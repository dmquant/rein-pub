//! EvidenceBundle (§8): a content-addressed, self-describing tarball —
//! manifest, ContextPack, receipts, events, artifact and input bytes,
//! providers lock, redaction report. `verify` re-checks every digest, every
//! sequence gap, and receipt-chain consistency, deterministically.
//!
//! The receipt kinds are §8's reduced list — fence-generation / commit /
//! validation / terminal / selection — and verification looks for **no lease
//! receipt**, because none is representable.

use crate::cas::Cas;
use crate::store::{Store, StoreError};
use crate::workspace::Workspace;
use rein_core::canon::Sha256Digest;
use rein_core::entities::RedactionReport;
use rein_core::ids::AttemptId;
use rein_core::receipts::{CommitVerdict, ReceiptBody, ReceiptLog};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const BUNDLE_FILES_SCHEMA: &str = "rein.evidence-files/v1";

#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Cas(#[from] crate::cas::CasError),
    #[error(transparent)]
    Bundle(#[from] rein_core::selection::BundleError),
    #[error("io at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("bundle verification failed: {0:?}")]
    VerifyFailed(Vec<String>),
}

fn io_err(path: PathBuf) -> impl FnOnce(std::io::Error) -> EvidenceError {
    move |source| EvidenceError::Io { path, source }
}

/// The file index: every path in the bundle with its digest. `manifest.json`
/// itself is indexed too; `files.json` is the root of trust for the tarball
/// and is the one file verified by re-reading everything else.
#[derive(Debug, Serialize, Deserialize)]
pub struct FileIndex {
    pub schema: String,
    pub files: BTreeMap<String, String>,
}

/// Assemble the bundle directory for an attempt, then tar+zstd it.
pub fn bundle_attempt(
    ws: &Workspace,
    store: &Store,
    attempt_id: &AttemptId,
    out_path: &Path,
) -> Result<PathBuf, EvidenceError> {
    let log = store.load_attempt_log(attempt_id)?;
    let manifest = rein_core::selection::assemble_bundle_manifest(
        &log,
        attempt_id,
        RedactionReport::default(),
    )?;
    let cas = Cas::new(ws.objects());

    let staging = ws.tmp().join(format!("bundle-{}", attempt_id.as_str()));
    let _ = std::fs::remove_dir_all(&staging);
    let dir = staging.join("evidence");
    std::fs::create_dir_all(dir.join("artifacts")).map_err(io_err(dir.join("artifacts")))?;
    std::fs::create_dir_all(dir.join("inputs")).map_err(io_err(dir.join("inputs")))?;

    let mut files: BTreeMap<String, String> = BTreeMap::new();
    let mut write_file = |rel: &str, bytes: &[u8]| -> Result<(), EvidenceError> {
        let path = dir.join(rel);
        std::fs::write(&path, bytes).map_err(io_err(path))?;
        files.insert(rel.to_string(), Sha256Digest::of_bytes(bytes).to_string());
        Ok(())
    };

    // Receipts, in ledger order.
    let mut receipts_ndjson = String::new();
    for e in log.iter() {
        receipts_ndjson.push_str(&serde_json::to_string(e).expect("receipt serializes"));
        receipts_ndjson.push('\n');
    }
    write_file("receipts.ndjsonl", receipts_ndjson.as_bytes())?;

    // Events for every run of the attempt.
    let mut events_ndjson = String::new();
    for ev in store.load_events_for_attempt(attempt_id)? {
        events_ndjson.push_str(&serde_json::to_string(&ev).expect("event serializes"));
        events_ndjson.push('\n');
    }
    write_file("events.ndjsonl", events_ndjson.as_bytes())?;

    // The frozen pack.
    let row = store.get_attempt(attempt_id)?;
    let pack = store.get_pack(&row.context_pack_id)?;
    write_file(
        "context-pack.json",
        &serde_json::to_vec_pretty(&pack).expect("pack serializes"),
    )?;

    // Committed artifact bytes (read-back verified on the way out) and pinned
    // input bytes.
    for e in log.iter() {
        if let ReceiptBody::Commit { artifacts, .. } = &e.body {
            for a in artifacts {
                if a.verdict == CommitVerdict::Verified {
                    if let Some(d) = &a.readback_digest {
                        let bytes = cas.read_verified(d)?;
                        write_file(
                            &format!("artifacts/{}", d.as_str().replace(':', "-")),
                            &bytes,
                        )?;
                    }
                }
            }
        }
    }
    for input in &pack.inputs {
        if let Ok(d) =
            Sha256Digest::parse(input.artifact_ref.as_str().trim_start_matches("artifact:"))
        {
            if let Ok(bytes) = cas.read_verified(&d) {
                write_file(&format!("inputs/{}", d.as_str().replace(':', "-")), &bytes)?;
            }
        }
    }

    // Providers lock (workspace copy at bundle time).
    if let Ok(lock_bytes) = std::fs::read(ws.providers_lock()) {
        write_file("providers.lock", &lock_bytes)?;
    }

    write_file(
        "manifest.json",
        &serde_json::to_vec_pretty(&manifest).expect("manifest serializes"),
    )?;

    // The file index goes last and is not self-indexed.
    let index = FileIndex {
        schema: BUNDLE_FILES_SCHEMA.to_string(),
        files,
    };
    let index_path = dir.join("files.json");
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&index).expect("index serializes"),
    )
    .map_err(io_err(index_path))?;

    // tar + zstd.
    let out = if out_path.extension().is_some() {
        out_path.to_path_buf()
    } else {
        out_path.with_extension("evidence.tar.zst")
    };
    let file = std::fs::File::create(&out).map_err(io_err(out.clone()))?;
    let zw = zstd::stream::write::Encoder::new(file, 3)
        .map_err(io_err(out.clone()))?
        .auto_finish();
    let mut tarb = tar::Builder::new(zw);
    tarb.append_dir_all("evidence", &dir)
        .map_err(io_err(out.clone()))?;
    tarb.into_inner()
        .map_err(io_err(out.clone()))?
        .flush()
        .map_err(io_err(out.clone()))?;
    let _ = std::fs::remove_dir_all(&staging);
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct VerifyReport {
    pub files_checked: usize,
    pub receipts_replayed: usize,
    pub events_checked: usize,
    pub event_gaps: Vec<u64>,
    pub problems: Vec<String>,
}

impl VerifyReport {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// Verify a bundle (tarball or unpacked dir), deterministically:
/// every file re-hashed against the index; the pack re-hashed against its
/// context_hash; receipts replayed (chain continuity + terminal consistency);
/// event sequences gap-checked; committed artifact bytes present and
/// digest-true.
pub fn verify_bundle(path: &Path) -> Result<VerifyReport, EvidenceError> {
    let dir = if path.is_dir() {
        path.join("evidence")
    } else {
        let staging = std::env::temp_dir().join(format!(
            "rein-verify-{}",
            Sha256Digest::of_bytes(path.to_string_lossy().as_bytes())
                .as_str()
                .chars()
                .skip(7)
                .take(12)
                .collect::<String>()
        ));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(io_err(staging.clone()))?;
        let file = std::fs::File::open(path).map_err(io_err(path.to_path_buf()))?;
        let zr = zstd::stream::read::Decoder::new(file).map_err(io_err(path.to_path_buf()))?;
        let mut archive = tar::Archive::new(zr);
        archive.unpack(&staging).map_err(io_err(staging.clone()))?;
        staging.join("evidence")
    };

    let mut problems = Vec::new();

    let index: FileIndex = serde_json::from_slice(
        &std::fs::read(dir.join("files.json")).map_err(io_err(dir.join("files.json")))?,
    )
    .map_err(|e| EvidenceError::VerifyFailed(vec![format!("files.json unparseable: {e}")]))?;

    // 1. Every indexed file re-hashes.
    let mut files_checked = 0;
    for (rel, want) in &index.files {
        files_checked += 1;
        match std::fs::read(dir.join(rel)) {
            Ok(bytes) => {
                let got = Sha256Digest::of_bytes(&bytes).to_string();
                if &got != want {
                    problems.push(format!("{rel}: digest {got} ≠ indexed {want}"));
                }
            }
            Err(_) => problems.push(format!("{rel}: indexed but absent")),
        }
    }

    // 2. The pack re-hashes to its sealed context_hash.
    let pack: Result<rein_core::context_pack::ContextPack, _> =
        std::fs::read(dir.join("context-pack.json"))
            .map_err(|e| e.to_string())
            .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()));
    match pack {
        Ok(p) => {
            if let Err(e) = p.verify_sealed() {
                problems.push(format!("context pack: {e}"));
            }
        }
        Err(e) => problems.push(format!("context-pack.json: {e}")),
    }

    // 3. Receipts replay: chain continuity and terminal consistency.
    let mut receipts_replayed = 0;
    let mut attempt: Option<AttemptId> = None;
    let mut entries = Vec::new();
    if let Ok(text) = std::fs::read_to_string(dir.join("receipts.ndjsonl")) {
        for line in text.lines() {
            match serde_json::from_str::<rein_core::receipts::ReceiptEnvelope>(line) {
                Ok(e) => {
                    attempt.get_or_insert(e.attempt_id.clone());
                    entries.push(e);
                    receipts_replayed += 1;
                }
                Err(e) => problems.push(format!("receipt line unparseable: {e}")),
            }
        }
    } else {
        problems.push("receipts.ndjsonl absent".to_string());
    }
    // §8: no lease receipt kind exists — nothing to look for, and an
    // unknown kind would already have failed deserialization above.
    if let Some(aid) = &attempt {
        let log = ReceiptLog::from_envelopes(entries.clone());
        match rein_core::state::resolve_state(&log, aid) {
            Ok(state) => {
                let has_terminal = log
                    .for_attempt(aid)
                    .any(|e| matches!(e.body, ReceiptBody::Terminal { .. }));
                if matches!(
                    state,
                    rein_core::state::AttemptState::Terminal
                        | rein_core::state::AttemptState::Closed
                ) && !has_terminal
                {
                    problems.push("terminal state without a terminal receipt".to_string());
                }
                // Committed artifact bytes must be in the bundle, digest-true.
                for e in log.for_attempt(aid) {
                    if let ReceiptBody::Commit { artifacts, .. } = &e.body {
                        for a in artifacts {
                            if a.verdict == CommitVerdict::Verified {
                                if let Some(d) = &a.readback_digest {
                                    let rel = format!("artifacts/{}", d.as_str().replace(':', "-"));
                                    if !index.files.contains_key(&rel) {
                                        problems.push(format!(
                                            "committed artifact {} not carried in the bundle",
                                            d
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => problems.push(format!("receipt chain: {e}")),
        }
    }

    // 4. Event sequences: duplicates idempotent, gaps surfaced.
    let mut events_checked = 0;
    let mut event_gaps = Vec::new();
    if let Ok(text) = std::fs::read_to_string(dir.join("events.ndjsonl")) {
        let mut ledgers: BTreeMap<String, rein_core::hand::EventLedger> = BTreeMap::new();
        for line in text.lines() {
            match serde_json::from_str::<rein_core::hand::SequencedEvent>(line) {
                Ok(ev) => {
                    events_checked += 1;
                    let ledger = ledgers
                        .entry(ev.run_id.as_str().to_string())
                        .or_insert_with(|| rein_core::hand::EventLedger::new(ev.run_id.clone()));
                    if let Err(e) = ledger.ingest(ev) {
                        problems.push(format!("event conflict: {e}"));
                    }
                }
                Err(e) => problems.push(format!("event line unparseable: {e}")),
            }
        }
        for (run, ledger) in &ledgers {
            let gaps = ledger.gaps();
            if !gaps.is_empty() {
                // Gaps are surfaced, not silently accepted — but a gap is a
                // finding about the run, not bundle corruption.
                event_gaps.extend(gaps.iter().copied());
                problems.push(format!("run {run}: event sequence gaps {gaps:?}"));
            }
        }
    }

    Ok(VerifyReport {
        files_checked,
        receipts_replayed,
        events_checked,
        event_gaps,
        problems,
    })
}
