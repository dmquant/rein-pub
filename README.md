# Rein — a financial research harness

*Reins are the harness on an animal whose strength you rent but do not own.*

Rein turns a frozen **Task** into fenced **Attempts** through model **Hands**,
captures everything every channel emitted, validates artifacts against
declared contracts, classifies outcomes **from receipts — never from exit
codes or model prose** — and emits portable, self-verifying evidence bundles.
It owns execution behavior and evidence capture, and nothing else: **Gate**
(the knowledge/gate estate) adjudicates what Rein proposes, the **AI
Institute** supplies pinned input material, and **AGORA** is a downstream,
optional publication surface whose outage can never stop a run.

Built from design v0.2 (`docs/Rein-Financial-Research-Harness-Design.md`,
sha256 `e685d399…97cb0`), accepted as binding — with two recorded objections
and their resolutions — in AGORA room `build:rein-financial-research-harness`.

**Status: all five milestones (M0–M5) landed.** 93 tests green across every
suite; all 33 design invariants carry reddening tests that name their
production symbols (`docs/INVARIANTS.md`). Verified against reality, not just
fixtures: live FMP pulls, a real agy/Gemini valuation that survived the full
validator gauntlet, capsules landing at Gate's gate through the installed
`gate` binary, and evidence bundles the binary publishes to AGORA itself.

---

## The discipline

Two sentences organize everything here:

> **Process exit is evidence, not terminal classification.**
> **"Done" without proof is treated as a failure.**

Concretely:

- Six claims about a piece of work keep six separate vocabularies, never one
  badge: process completion ≠ artifact completion ≠ attempt outcome ≠ task
  satisfaction ≠ research acceptance ≠ system admission.
- `success` requires: every required artifact committed content-addressed
  **and read back through a handle the writer did not own**, every mandatory
  validator passed, no unresolved policy failure, and a classifier receipt.
- A successful Attempt still does not satisfy its Task — only a
  `TaskSelectionReceipt` does.
- `unknown` never defaults to anything, and **administrative force-success
  does not exist** — not as a function, a CLI action, or a keybinding.
- Every state transition appends a receipt; state is resolved by replaying
  the ledger, never read from memory. The ledger is append-only *by SQLite
  trigger*, not by convention.
- Every figure a data tool returns is stamped
  `{value, unit, as_of(+basis), provider, retrieved_at}` — or the tool
  refuses. A past-cutoff epoch may read only Rein's own CAS captures made
  inside the cutoff: live vendor APIs serve current-vintage figures no query
  parameter can unwind.

```
  AI Institute ──(reports, facts, odds — pinned, read-only)──► material
       │                                            ┌──────────────┐
       │                                            │     REIN     │
       │                                            │  executes    │
       │                                            │  bounded     │
       │                                            │  financial   │
       │                                            │  research    │
       │                                            └──────┬───────┘
       │                              evidence bundles + candidate capsules
       │                                                   │
       ▼                                            ┌──────▼───────┐
    AGORA ◄──(optional, post-hoc publish)───────────│     GATE      │
  coordination                                      │ knowledge /  │
                                                    │ gate policy  │
                                                    └──────────────┘
```

Rein never writes Gate's graph and has **no commit verb into Gate, ever**: it
drives the installed `gate` binary as a black box (`gate import capsule`), and
everything lands as one open delta at Gate's gate.

## Crates

| Crate | What it holds |
|---|---|
| `rein-core` | The contracts (M0): 15 entities, the 10-state attempt lifecycle, the 10-value `TerminalOutcome` vocabulary with its total exit-code mapping, ContextPack canonical hashing, receipt schemas, the hand protocol, ten conformance fixtures, the incremental UTF-8 capture decoder. **No clock, no randomness, no I/O** — which is what makes determinism testable. |
| `rein-runtime` | Durability (M1/M3): SQLite WAL ledger (append-only by trigger), filesystem CAS with fresh-handle read-back, the §7 execution pipeline, strict replay, the recovery queue, evidence bundles + deterministic verify. |
| `rein-finance` | The domain (M2/M5): FMP data tools behind the PIT gate, compute tools (DCF / WACC / EV→equity bridge / comps / driver series / odds), the split valuation contract, eleven validators, SKILL.md playbooks, hands (`finance:deterministic`, `finance:ops`, the `agy` subprocess adapter), the two-track eval, AGORA publish. |
| `rein-propose` | The crossing (M3): capsules written with `gate-protocol` wire types, driven through the installed `gate` binary. |
| `rein` | The `rein` binary: full CLI plus the four-screen TUI. |

## Building

```sh
cargo build            # rustc/cargo 1.82; Cargo.lock is committed and pinned
cargo test             # 93 tests: vectors, property suites, the invariant
                       # manifest, the §6 failure matrix (pure and durable),
                       # CLI integration, headless TUI renders, the real
                       # gate-binary crossing, the dependency fence
```

Two environmental notes, stated rather than discovered:

- `rein-propose` has a **path dependency on the sibling Gate workspace**
  (`../../../../sibling-estate/crates/gate-protocol`) — the repo builds inside the
  estate layout (`~/prog/institute/{sibling-estate, ros/rein}`). That coupling is
  the deliberate Q2 trade, recorded in the room with its mitigation.
- The toolchain is pinned around cargo 1.82: several transitive dependencies
  have newer releases requiring edition2024. `Cargo.toml`/`Cargo.lock` pin
  known-good versions (mirroring the sibling-estate lockfile); if resolution ever
  drifts, `cargo update -p <pkg> --precise <sibling-estate-lock-version>` is the
  playbook.

## Quickstart

Every command takes `--output table|json|yaml|ndjson` and emits a stable
envelope (`rein.cli-result/v1`) on stdout; diagnostics go to stderr; `ok` is
defined as exactly `exit code == 0`.

### Sixty seconds, no network: the deterministic proof

```sh
mkdir demo && cd demo
rein init
rein mission create etf-book --objective "maintain valuations"
rein epoch open 2026-08 --mission etf-book \
    --source-cutoff 2026-08-18T00:00:00Z --seal

cat > plan.yaml <<'EOF'
plan_ref: plan:demo@1
nodes:
  - task_ref: task:proof@1
    task_type: fixture
EOF
rein plan apply -f plan.yaml

rein run task:proof@1 --hand fake:deterministic-a \
    --wait --require task-satisfied        # exit 0 ⇔ a verified TaskSelectionReceipt
rein attempt list
rein replay attempt <attempt_id> --strict  # re-runs the hand, re-hashes, compares
```

Run it again through `fake:deterministic-b` (`rein attempt retry <id> --hand
fake:deterministic-b`): same frozen ContextPack, next generation, **identical
artifact digests** — the M1 acceptance, and the kill criterion that stays
unarmed because nothing in the judgment path can read a clock.

### The real thing: a valuation on live data

```sh
# configRoot (~/.config/rein/) holds credentials — never the workspace:
#   secrets.toml   fmp = "<key>"          (or export FMP_API_KEY, or point
#   config.toml    fmp_env_file = "..."    at an existing env file)

rein data pull-equity NVDA --kinds quote,cashflow,balance
rein capture list                          # stamped, captured to CAS

rein task add task:dcf-nvda@1 --plan plan:demo@1 --type valuation \
    --universe security:nvda \
    --input capture:<digest> --input capture:<digest> --input capture:<digest>

rein run task:dcf-nvda@1 --hand finance:deterministic \
    --wait --require task-satisfied
rein artifact cat <valuation.json digest>
```

The valuation contract is split on purpose: `assumptions.json` carries every
input as `{value, basis}` — a capture digest, a cited claim, or a justified
assumption; **a bare float is unrepresentable** — and `valuation.json`
carries the arithmetic, which the `numeric-consistency` validator recomputes
*from the assumptions alone*. The EV→equity→per-share bridge is mandatory; a
sensitivity table and at least one statable falsifier are required, or the
valuation is not decision-ready.

Swap in a real model: `--hand agy` (configure `agy_model` in config.toml).
The adapter spawns agy by absolute path, single attempt, no internal retries;
the model supplies assumptions, **the adapter recomputes the arithmetic**, and
an empty or non-SUCCESS response is an error regardless of exit code.

### Evidence, recovery, the gate

```sh
rein evidence bundle <attempt> --out nvda.evidence.tar.zst
rein evidence verify nvda.evidence.tar.zst   # re-hashes every file, re-seals
                                             # the pack, replays the receipt
                                             # chain, gap-checks events
rein evidence publish <attempt> --room <agora-room-id>   # explicit, never ambient

rein recover                                 # the typed-anomaly queue
rein attempt recover <id>                    # diagnosis first…
rein attempt recover <id> --action resume-commit   # …then one of exactly three
                                             # (resume-commit | retry | close-unknown)

rein propose to-gate <attempt> --gate-project ~   # capsule → gate import capsule
rein propose status <attempt> --gate-project ~   # polls the gate, appends an
                                                # admission receipt
```

*(§9 of the design spells the propose verb `--to-gate`; the implemented
surface is the subcommand `propose to-gate`.)*

### The TUI

```sh
rein tui
```

Four screens — **1** Mission Control (Current Truth: epoch, cutoff, PIT mode,
providers.lock hash; every outcome cell names its receipt), **2** Live
Attempt (the six vocabularies as separate fields, disagreements and all),
**3** Recovery Console (three actions behind confirm popups; no
force-success keybinding exists), **4** Compare (differences classified into
six classes). Keys: `?` help · `:` palette · `g`+`1–4` goto · `j/k` ·
`a`/`b` mark a compare pair · `F2` mouse · `Esc` unwinds popup → selection →
quit. Committed panes are double-bordered `[committed]`; live reads are
plain `[live]`. Absence is always words, never a blank.

### Eval (scores never touch outcomes)

```sh
rein eval financegym                       # bundled sample; bring the public
rein eval financegym -f qs.jsonl --answers answers.json   # set yourself (CC BY-NC)
rein eval internal                         # hands ranked on the estate's own
                                           # settled valuations
```

Rubric tiers 0–4, score `s/(4n)`, seeded deterministic bootstrap CI. Scoring
reads artifacts only and appends no receipts — benchmark reward never
classifies runtime success.

## Exit codes and wait assertions

Closed vocabulary (child process exits are captured *inside* evidence and
never passed through): `0` asserted-true · `2` usage · `3` reserved · `4`
not-found · `5` conflict/stale-fence · `6` provider unresolved · `7` policy
denied · `8` budget · `9` transport · `10` attempt terminal non-success ·
`11` unknown · `12` artifact commit/readback failed · `13` validation
wait-assertion failed · `14` cancelled/timeout · `15` evidence/replay
mismatch · `70` internal.

| TerminalOutcome | exit | | TerminalOutcome | exit |
|---|---|---|---|---|
| success | 0 | | budget_exhausted | 8 |
| partial_success | 10 | | policy_denied | 7 |
| failure | 10 | | lease_lost | 5 *(reserved)* |
| cancelled | 14 | | artifact_invalid | 12 |
| timed_out | 14 | | unknown | 11 |

`--wait --require <assertion>` certifies exactly one thing via a verified
receipt: `attempt-terminal` · `artifact-committed` · `validation-passed`
(unmet → 13) · `task-satisfied` · `plan-completed`. **Without `--wait`, exit
0 means the attempt was admitted and ran — it asserts nothing about the
outcome**, and the envelope says so.

## Validators

`artifact-wellformed` · `secret-scan` (quarantine = a verdict plus a receipt
that withholds the artifact from selection) · `input-closure` (no
hallucinated basis survives) · `numeric-consistency` (the DCF recomputes from
assumptions alone) · `bridge-completeness` · `falsifier-present` ·
`source-cutoff` · `fact-vs-forecast` (a post-cutoff year stated as fact
fails — the 2027-claim class) · `citation-closure` (`[N]` resolves to
captured bytes; a word in brackets is not a citation) ·
`coverage-denominator` (eligible/consumed/withheld must add up; every drop
carries a reason) · `ops-discipline` (verify/settle/monitor artifacts
re-derived, not trusted). SKILL.md playbooks in `.rein/skills/` add their
`validator_refs` to the task contract at pack freeze — enforcement lives on
the side the executor does not control.

## Configuration

`configRoot` (default `~/.config/rein/`, override `--config-root` /
`REIN_CONFIG_ROOT`) is **refused if it sits inside the workspace** —
credentials never resolve from a model-writable tree.

```toml
# ~/.config/rein/config.toml        # ~/.config/rein/secrets.toml
default_hand = "finance:deterministic"     # fmp    = "…"
searxng_url  = "http://localhost:8080"     # <name> = "…"  → secret-ref:<name>
fmp_env_file = "/path/to/.env"       # pointer to an existing FMP_API_KEY file
agy_path     = "agy"                 # resolved to an absolute path or refused
agy_model    = "gemini-3.7-flash-low"
agora_key_path = "~/.agora/rein-party-key"
```

Workspace layout (`.rein/`): `workspace.yaml`, `providers.lock`
(deterministic except one labeled timestamp), `policies/`, `plans/`,
`skills/`, `ledger.db`, `objects/` (CAS), `cache/`, `logs/`, `tmp/`.

## The dependency fence

`gate-state`, `gate-graph`, `gate-ontology`, `gate-tui` are forbidden in
dependencies **and** dev-dependencies, transitively — enforced by
`tests/fence_deps.rs` over the resolved cargo graph. The only sanctioned
crate-level tie to the Gate estate is `gate-protocol` (wire types for the
boundary). Cross-product integration goes through binaries, the way an
outside consumer would arrive.

## Provenance and record

- **Design:** `docs/Rein-Financial-Research-Harness-Design.md` (v0.2) —
  synthesized from the Agora deep-design spec Part III, FinanceHarness/
  FinanceGym as design reference (CC BY-NC, nothing vendored),
  deepseek-harness's composition seams, the AGORA record, and Gate's 45
  paid-for lessons.
- **Acceptance and amendments:** AGORA room
  `build:rein-financial-research-harness` — objections O1/O2 with their
  resolutions, decisions C1–C6, and the two implementation-forced C2
  amendments, all recorded *before* the code sealed them. Reversing any of
  them silently reddens tests.
- **Invariant map:** `docs/INVARIANTS.md` — 33/33 green, each row naming its
  production symbol and test.
- **Deliberately unbuilt** (§12, each with its reinstatement condition):
  lease service, multi-tenant/remote execution, container sandbox tiers,
  plugin PKI, `backtest`. The M2 kill criterion (>5 typed-anomaly unknowns
  per trailing 100 attempts) is armed and counts as volume arrives.

Private estate tooling; not published to crates.io.
