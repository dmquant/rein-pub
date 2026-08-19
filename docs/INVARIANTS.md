# Invariant → symbol → test map

Design v0.2 §2 (sha256 `e685d399…97cb0`), as accepted by rein in AGORA room
`build:rein-financial-research-harness` with objections O1/O2 and decisions
C1–C6. A guarantee lands only with a test that reddens when it is deleted, and
every mutation test names a production symbol; each invariant's test is owed at
the milestone where its production symbol first exists.

M0-owed tests live in `crates/rein-core/tests/invariants.rs` and are named
`invNN__<symbol>__<claim>`. Status: **M0 rows all green** (2026-08-19).

| # | Invariant (short) | Milestone | Production symbol | Test |
|---|---|---|---|---|
| 1 | Six claim vocabularies, never one badge; external axes stated | **M0** ✅ | `axes::AxisReport` | `inv01__axes_axisreport__…` |
| 2 | Exit / self-report = evidence, not classification | **M0** ✅ | `classify::classify` | `inv02__classify_classify__…` |
| 3 | Success = readback + validators + policy resolution + receipt | **M0** ✅ | `classify::classify` | `inv03__classify_classify__…` |
| 4 | Only a TaskSelectionReceipt satisfies a Task | **M0** ✅ | `selection::task_satisfied` | `inv04__selection_task_satisfied__…` |
| 5 | Unknown stays unknown; no force-success | **M0** ✅ | `recovery::RecoveryAction` | `inv05__recovery_recoveryaction__…` |
| 6 | One immutable ContextPack; retry byte-identical; semantic change → new TaskVersion | **M0** ✅ | `idempotency::admit` / `recovery::retry_same_context_pack` | `inv06__attempt_retry__…` |
| 7 | Canonical encoding (C1) + exclusion set (C2) | **M0** ✅ | `canon::parse_canon_json`, `context_pack::ContextPack::semantic_hash` | `inv07__canon_canonicalize__…` + `canonical_vectors.rs` + `prop_canon.rs` |
| 8 | Exact pins or declared method; model_id = requested + served | **M0** ✅ | `pins::ProviderPin`, `hand::ModelIdentity` | `inv08__pins_providerpin__…` |
| 9 | Resolvable attempt join key on every proposed fact | **M0** ✅ | `selection::resolve_attempt_ref` | `inv09__selection_resolve_attempt_ref__…` |
| 10 | Budgets = max_steps + per_step_timeout_ms | M2 (schema at M0: `context_pack::Budget`, `hand::per_step_breach`) | — | owed M2 |
| 11 | Hand-internal retries disabled; `attempts` recorded | M2 (schema at M0: `hand::HandRequest::internal_retries_disabled`) | — | owed M2 (gate-models retries=0 path) |
| 12 | Every stage checkpoints; `--resume`; nothing non-resumable | M2 | — | owed M2 |
| 13 | PIT: past-cutoff epochs read own-CAS only; eval/production modes | M2 (schema at M0: `context_pack::PitMode`) | — | owed M2 |
| 14 | Temporal leakage is a validator | M2 | — | owed M2 |
| 15 | knowledge_cutoff advisory, stamped honestly | M2 | — | owed M2 |
| 16 | Numeric datum carries its time axes or the tool refuses | M2 | — | owed M2 |
| 17 | Captured bytes or claims degrade to unresolved | M2 | — | owed M2 |
| 18 | Citation closure validator | M2 | — | owed M2 |
| 19 | Publisher spread; syndication ≠ corroboration | M2 | — | owed M2 |
| 20 | Coverage denominators over enumerable sets; drops counted | M2 | — | owed M2 |
| 21 | Direct/inherited never summed; falsifier discipline | M2 | — | owed M2 |
| 22 | Every transition appends a receipt; state resolves from ledger | **M0** ✅ → **M1** ✅ re-pointed at the SQLite WAL ledger (append-only by trigger) | `state::apply_transition`, `store::Store` | `inv22__state_transition_apply__…` + `inv22__store_persist__…` (m1_acceptance) |
| 23 | Idempotency scoped to request (C4); retry mints generation | **M0** ✅ | `idempotency::IdempotencyKey` | `inv23__idempotency_key__…` |
| 24 | Fence generations from day one; stale generations cannot commit | **M0** ✅ | `fence::guard_commit`, `fence::issue_next_generation` | `inv24__fence_generation__…` |
| 25 | Async-boundary checks tolerate the boundary's latency | M3 (first async check) | — | owed M3 |
| 26 | Absolute paths; `env -i` scheduled-path test | M2 (schema at M0: `receipts::ReceiptBody::Environment`) | — | owed M2 |
| 27 | configRoot ≠ workspaceRoot | **M1** ✅ | `workspace::SecretBroker::open` | `inv27__workspace_secretbroker_open__…` (m1_acceptance) |
| 28 | Secrets are references; quarantine = verdict + receipt (C6) | **M0** ✅ (schema-side; brokered injection M2) | `secretref::Redactor`, `receipts::ReceiptBody::Quarantine` | `inv28__secretref__…` |
| 29 | Grants explicit, expiring, non-transitive; TOFU | M2 (schema at M0: `entities::CapabilityGrant` — `expires_at` mandatory, no delegation field) | — | owed M2 |
| 30 | Incremental UTF-8 decode retaining partial sequences | **M0** ✅ | `capture::Utf8StreamDecoder` | `inv30__capture_utf8streamdecoder__…` |
| 31 | Absence is stated, never blank | M4 (schema seed at M0: `axes::Axis::NotYetRecorded`, `axes::ExternalAxis`) | — | owed M4 |
| 32 | Disabled actions explain; statuses name their receipt | M4 | — | owed M4 |
| 33 | Findings a gate no longer holds are still reported | M3/M4 | — | owed M3 |

Accepted resolutions the code embodies (recorded in the room before
implementation):

- **O1** — bare `secret-leak` run exits 10 (`failure` per the total §9 map);
  13 is reserved to `--wait --require validation-passed`. Pinned in
  `tests/protocol.rs::failure_matrix_every_fixture_ends_in_its_prescribed_row`.
- **O2** — abort edges `{created, admitted, preparing} → classifying`, each
  with an abort-cause receipt; from `running` onward the drawn pipeline always
  completes; `recovery_pending → terminal` demands a classifier receipt of
  outcome `unknown` (or a separately authorized exception receipt). Pinned in
  `tests/prop_state.rs`.
- **C2 refinement** (reported with the M0 finding): `idempotency_key` is
  request-side, not a pack field — invariant 23 derives it from the context
  hash, so hashing it would be circular.
- **C2 amendment (M1, refroze the pack vector):** the hand binding is
  execution binding, excluded from the semantic hash — M1's own acceptance
  ("same ContextPack through fake-a and fake-b") is unsatisfiable otherwise,
  and recovery's "retry same ContextPack" dies with a dead hand. Attribution
  is unchanged: selector, requested/served ids and the hand pin are in
  receipts. Exclusion set: `{context_pack_id, context_hash, created_at,
  hand}`.
