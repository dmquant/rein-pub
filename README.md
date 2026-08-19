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

**M0 — contracts (networkless, modelless): landed.** `crates/rein-core` holds
the entities, the 10-state attempt lifecycle, the 10-value TerminalOutcome
vocabulary with its total exit-code mapping, ContextPack canonical hashing,
receipt schemas, the fake-hand protocol with all ten conformance fixtures, and
the incremental UTF-8 capture decoder. No binary yet — the first `rein` CLI
surface lands at M1 per §9.

```
cargo test        # 38 tests: vectors, property suites, invariant manifest,
                  # the §6 failure matrix at M0 depth, the dependency fence
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

M0 contracts ✅ · M1 local deterministic proof (SQLite ledger, CAS,
commit/readback, strict replay) · M2 finance layer (data/compute tools, PIT
modes, valuation contract, first real hand) · M3 recovery + evidence +
propose-to-gate · M4 TUI (four screens) · M5 eval + integration. Each carries
acceptance tests and a kill criterion in design §13.
