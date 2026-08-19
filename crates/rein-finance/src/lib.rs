//! # rein-finance — the finance domain layer (M2)
//!
//! What makes the harness a *financial* harness (§4): stamped data tools
//! that capture to CAS or refuse, deterministic compute tools with strict
//! parameter surfaces, PIT enforcement where it is real, the split valuation
//! contract, finance validators, SKILL.md playbooks, and the first hands —
//! a deterministic valuation producer and the agy subprocess adapter.
//!
//! Domain expertise lives in profiles, skills, tools and validators — never
//! in runtime code paths.

pub mod agora;
pub mod capture;
pub mod datum;
pub mod eval;
pub mod fmp;
pub mod frame;
pub mod hands;
pub mod ops;
pub mod schemas;
pub mod skills;
pub mod validators;

pub mod compute {
    pub mod bridge;
    pub mod comps;
    pub mod dcf;
    pub mod odds;
    pub mod series;
    pub mod wacc;
}
