//! Capture: what a run *actually emitted*, on every channel (JOINING §5's
//! capture-artifact shape), and the incremental UTF-8 decoder (invariant 30).
//!
//! Child process exit codes live here — inside evidence — and nowhere else.
//! [`crate::classify::classify`] never reads them as classification.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Incremental UTF-8 decoder that retains trailing partial sequences across
/// chunk boundaries. Per-chunk lossy decode destroys CJK output at measured
/// 100%/256-byte chunks, 29%/2048 (AGORA Stage 0) — this is the fix, and
/// `fake:cjk-splitter` is its executable fixture.
#[derive(Debug, Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode a chunk. Invalid bytes become U+FFFD; an *incomplete* trailing
    /// sequence is retained for the next chunk, never replaced.
    pub fn feed(&mut self, chunk: &[u8]) -> String {
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(chunk);
        let mut out = String::new();
        let mut rest: &[u8] = &buf;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    break;
                }
                Err(e) => {
                    let (valid, after) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(valid).expect("prefix is valid"));
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            rest = &after[n..];
                        }
                        None => {
                            // Incomplete sequence at end of input: retain.
                            self.pending = after.to_vec();
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    /// End of stream. A sequence still incomplete at EOF is genuinely invalid
    /// and becomes one replacement char — stated, not silent.
    pub fn finish(self) -> String {
        if self.pending.is_empty() {
            String::new()
        } else {
            "\u{FFFD}".to_string()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideChannel {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub error_extract: String,
}

/// Everything captured from one run, every channel. Evidence, not verdict
/// (invariant 2): exit code and self-report are fields in here precisely so
/// they cannot be terminal classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureArtifact {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub side_channels: Vec<SideChannel>,
    pub captured_via: String,
    pub tool_versions: BTreeMap<String, String>,
}
