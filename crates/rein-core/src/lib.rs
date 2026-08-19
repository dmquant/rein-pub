//! # rein-core — M0 contracts for Rein, the financial research harness
//!
//! Implements milestone M0 of `docs/Rein-Financial-Research-Harness-Design.md`
//! v0.2 (sha256 `e685d399…97cb0`), as accepted by the rein party in AGORA room
//! `build:rein-financial-research-harness`: entities, the 10-state attempt
//! lifecycle, the 10-value TerminalOutcome vocabulary, ContextPack canonical
//! hashing, receipt schemas, and the fake-hand protocol. Networkless and
//! modelless by construction.
//!
//! Two properties hold crate-wide and are what M0 exists to guarantee:
//!
//! - **No process exit or path can imply success.** [`classify::classify`]
//!   derives `TerminalOutcome` from receipts only; child exit codes live inside
//!   capture evidence and have no route to the outcome (invariants 2, 3, 5).
//! - **No ambient state.** There is no clock, no randomness, no environment
//!   read anywhere in this crate: time enters as values, identifiers come from
//!   an injected [`ids::IdGen`]. This is what arms M1's kill criterion
//!   (digest-equality determinism) rather than fighting it.
//!
//! Accepted deviations from the design text, recorded in the room before
//! implementation: O1 (bare `secret-leak` run exits 10, not 13) and O2 (abort
//! edges `{created, admitted, preparing} → classifying`; `recovery_pending →
//! terminal` requires a classifier receipt).

pub mod axes;
pub mod canon;
pub mod capture;
pub mod classify;
pub mod context_pack;
pub mod entities;
pub mod fakes;
pub mod fence;
pub mod hand;
pub mod idempotency;
pub mod ids;
pub mod outcome;
pub mod pins;
pub mod receipts;
pub mod recovery;
pub mod secretref;
pub mod selection;
pub mod state;
pub mod time;
