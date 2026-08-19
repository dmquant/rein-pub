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
| 10 | Budgets = max_steps + per_step_timeout_ms | **M2** ✅ | `hand::per_step_breach` + engine Budget receipt | `inv10__engine_budget__…` (m2_acceptance) |
| 11 | Hand-internal retries disabled; `attempts` recorded | **M2** ✅ | `hands::AgyHand` (single-shot by construction; attempts recorded) | `inv11_26__agy_hand__…` (m2_acceptance). Deviation recorded: rein ships its own agy adapter instead of consuming gate-models (whose retry loops violate this invariant) |
| 12 | Every stage checkpoints; `--resume`; nothing non-resumable | **M2** ✅ | per-phase receipt persistence + `selection::task_satisfied`-driven plan sweep | `inv12__plan_sweep_resume__…` (m2_acceptance) |
| 13 | PIT: past-cutoff epochs read own-CAS only; eval/production modes | **M2** ✅ | `capture::ensure_live_permitted`, `capture::capture_admissible` | `inv13__capture_ensure_live_permitted__…` (m2_acceptance) |
| 14 | Temporal leakage is a validator | **M2** ✅ | validator `fact-vs-forecast@1` | `inv14__fact_vs_forecast__…` (m2_acceptance) |
| 15 | knowledge_cutoff advisory, stamped honestly | **M2** ✅ | engine environment receipt (advisory note on hand_internal_network runs) | covered in engine notes; pane rendering owed M4 |
| 16 | Numeric datum carries its time axes or the tool refuses | **M2** ✅ | `datum::Stamped::new` | `inv16__datum_stamped_new__…` (m2_acceptance) |
| 17 | Captured bytes or claims degrade to unresolved | **M2** ✅ | validator `citation-closure@1` | `inv17_18__citation_closure__…` (m2_acceptance) |
| 18 | Citation closure validator | **M2** ✅ | validator `citation-closure@1` | `inv17_18__citation_closure__…` (m2_acceptance) |
| 19 | Publisher spread; syndication ≠ corroboration | **M2** ✅ | `capture::CaptureStore::capture_page` (host cap) | `inv19__capture_page__…` (m2_acceptance) |
| 20 | Coverage denominators over enumerable sets; drops counted | **M2** ✅ | validator `coverage-denominator@1` + `comps` counted exclusions | `inv20__coverage_denominator__…` + compute suite |
| 21 | Direct/inherited never summed; falsifier discipline | **M2** ✅ (falsifier face; direct-vs-inherited aggregation arrives with verify/settle at M5) | `schemas::claim_admissible`, validator `falsifier-present@1` | `inv21__schemas_claim_admissible__…` |
| 22 | Every transition appends a receipt; state resolves from ledger | **M0** ✅ → **M1** ✅ re-pointed at the SQLite WAL ledger (append-only by trigger) | `state::apply_transition`, `store::Store` | `inv22__state_transition_apply__…` + `inv22__store_persist__…` (m1_acceptance) |
| 23 | Idempotency scoped to request (C4); retry mints generation | **M0** ✅ | `idempotency::IdempotencyKey` | `inv23__idempotency_key__…` |
| 24 | Fence generations from day one; stale generations cannot commit | **M0** ✅ | `fence::guard_commit`, `fence::issue_next_generation` | `inv24__fence_generation__…` |
| 25 | Async-boundary checks tolerate the boundary's latency | **M3** ✅ | `recovery_queue::recovery_queue` (stale threshold ≫ boundary latency) | `inv25__recovery_queue__…` (m3_acceptance) |
| 26 | Absolute paths; `env -i` scheduled-path test | **M2** ✅ | `hands::AgyHand::resolve` (absolute or refuse) | `inv11_26__agy_hand__…` (spawned under a scrubbed PATH-only env) |
| 27 | configRoot ≠ workspaceRoot | **M1** ✅ | `workspace::SecretBroker::open` | `inv27__workspace_secretbroker_open__…` (m1_acceptance) |
| 28 | Secrets are references; quarantine = verdict + receipt (C6) | **M0** ✅ (schema-side; brokered injection M2) | `secretref::Redactor`, `receipts::ReceiptBody::Quarantine` | `inv28__secretref__…` |
| 29 | Grants explicit, expiring, non-transitive; TOFU | **M2** ✅ | `workspace::SecretBroker::env_for` (absence is never permission) + `entities::CapabilityGrant` shape | `inv29__secretbroker_env_for__…` |
| 30 | Incremental UTF-8 decode retaining partial sequences | **M0** ✅ | `capture::Utf8StreamDecoder` | `inv30__capture_utf8streamdecoder__…` |
| 31 | Absence is stated, never blank | M4 (schema seed at M0: `axes::Axis::NotYetRecorded`, `axes::ExternalAxis`) | — | owed M4 |
| 32 | Disabled actions explain; statuses name their receipt | M4 | — | owed M4 |
| 33 | Findings a gate no longer holds are still reported | **M3** ✅ (pane face owed M4) | `rein_propose::build_capsule_objects` (findings ride every payload) + `propose status` findings_reported | `inv9_33__capsule_objects__…` (rein-propose) |

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
