//! Semantic terminal styles (§10): meaning → color, one mapping shared by
//! every screen. ANSI named colors only — the user's terminal palette keeps
//! ownership of the exact hues, so dark and light schemes both stay legible;
//! foreground-only except the reverse-video selection cursor.
//!
//! The vocabulary mirrors the outcome vocabulary, not a generic UI kit:
//! `unknown` gets its own loud color because unknown must never fade into
//! the furniture, and nothing here can promote a color to a judgment — the
//! text a cell shows still comes from receipts.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// Verified-good: success, satisfied, passed.
pub fn ok() -> Style {
    Style::default().fg(Color::Green)
}

/// Hard-bad: failure, artifact_invalid, policy_denied, failed verdicts.
pub fn bad() -> Style {
    Style::default().fg(Color::Red)
}

/// Degraded but classified: partial, budget, timed_out, cancelled, differs.
pub fn warn() -> Style {
    Style::default().fg(Color::Yellow)
}

/// Unknown is loud, never neutral.
pub fn unknown() -> Style {
    Style::default().fg(Color::Magenta)
}

/// Identifiers, refs, hashes, keys — the things you act on or cite.
pub fn accent() -> Style {
    Style::default().fg(Color::Cyan)
}

/// Secondary chrome: hints, `[live]` tags, parentheticals, the forbidden line.
pub fn muted() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Panel titles and table headers.
pub fn heading() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// The selection cursor — terminal-native reverse video, theme-safe.
pub fn selected() -> Style {
    Style::default().add_modifier(Modifier::REVERSED)
}

/// A keycap in a hint bar: the key stands out, the label recedes.
pub fn key(k: &str) -> Span<'_> {
    Span::styled(k, accent().add_modifier(Modifier::BOLD))
}

/// Style a status word by its meaning. Substring-matched so
/// `success per rcpt_000123`, `Success`, and `unsatisfied` all land on the
/// right side; order matters (`partial_success` before `success`,
/// `unsatisfied` before `satisfied`).
pub fn status_style(text: &str) -> Style {
    let t = text.to_ascii_lowercase();
    if t.contains("unknown") {
        unknown()
    } else if t.contains("partial")
        || t.contains("cancelled")
        || t.contains("timed_out")
        || t.contains("budget")
        || t.contains("unsatisfied")
    {
        warn()
    } else if t.contains("failure")
        || t.contains("invalid")
        || t.contains("denied")
        || t.contains("contradicted")
        || t.contains("failed")
        || t.contains("missing")
    {
        bad()
    } else if t.contains("success") || t.contains("satisfied") || t.contains("passed") {
        ok()
    } else if t.contains("not adjudicated") || t.contains("not yet") || t.contains("none") {
        muted()
    } else {
        Style::default()
    }
}

/// A status value as a styled span.
pub fn status_span(text: &str) -> Span<'_> {
    Span::styled(text.to_string(), status_style(text))
}
