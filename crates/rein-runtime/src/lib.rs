//! # rein-runtime — the durable half of the harness (M1+)
//!
//! What M0 specified, this crate makes real: an append-only SQLite WAL ledger
//! (append-only by *trigger*, not convention), a filesystem CAS whose read-back
//! goes through a handle the writer did not own, the §7 execution pipeline
//! driving the M0 contracts, and strict replay.
//!
//! The M0 discipline carries over: classification still sees receipts only;
//! time enters through an injected [`clock::Clock`]; identifiers resume from a
//! persisted high-water mark so no ambient state sneaks in with durability.

pub mod cas;
pub mod clock;
pub mod engine;
pub mod hands;
pub mod providers;
pub mod replay;
pub mod store;
pub mod validators;
pub mod workspace;

pub use engine::Engine;
pub use store::Store;
pub use workspace::Workspace;
