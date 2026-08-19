# Rein — a financial research harness

Rein turns a frozen Task into fenced Attempts through model Hands, captures
everything, validates artifacts, classifies outcomes from receipts, and emits
evidence bundles. It owns execution behavior and evidence capture, and nothing
else: Gate adjudicates, the institute produces, AGORA is downstream and
optional.

- **Design:** `docs/Rein-Financial-Research-Harness-Design.md` (v0.2, sha256
  `e685d399…97cb0`), accepted with objections O1/O2 and decisions C1–C6 in
  AGORA room `build:rein-financial-research-harness`.
- **Invariant map:** `docs/INVARIANTS.md` — 33 invariants, each owed a
  reddening mutation test at the milestone its production symbol lands.
- **Joining AGORA:** `docs/JOINING.md`.

## Status

**All five milestones landed: M0 contracts ✅ · M1 deterministic proof ✅ ·
M2 finance layer ✅ · M3 recovery/evidence/propose ✅ · M4 TUI ✅ · M5 eval ✅.** `crates/rein-core`
holds the contracts (entities, 10-state lifecycle, TerminalOutcome vocabulary
with its total exit mapping, canonical hashing, receipts, the ten fake-hand
fixtures, the incremental UTF-8 decoder). `crates/rein-runtime` makes them
durable: SQLite WAL ledger append-only *by trigger*, filesystem CAS with
fresh-handle read-back, the §7 pipeline, strict replay. `crates/rein` is the
`rein` binary — §9's M1 command set, JSON envelope, closed exit-code
vocabulary, wait assertions.

```
cargo test        # vectors, property suites, invariant manifest, the §6
                  # failure matrix (pure and over the real store), the
                  # dependency fence, CLI integration
```

Two properties hold crate-wide: no process exit or path can imply success
(classification derives from receipts only), and no ambient state exists (no
clock, no randomness, no environment reads) — which is what makes M1's
digest-equality determinism acceptance testable at all.

## The dependency fence

`gate-state`, `gate-graph`, `gate-ontology`, `gate-tui` are forbidden in
dependencies **and** dev-dependencies, transitively — enforced by
`tests/fence_deps.rs` over the resolved cargo graph, not by convention. Rein
reaches Gate only through the installed `gate` binary (`rein propose --to-gate`,
M3), the same way an outside consumer would.

## Milestones

M0–M5 landed in order, each with §13's acceptance tests green. All 33
invariants carry green reddening tests (docs/INVARIANTS.md). Verified against
reality: live FMP pulls, a real agy/gemini valuation surviving the validator
gauntlet, capsules landing at gate's gate through the installed binary, and
evidence bundles the binary publishes to AGORA itself.
