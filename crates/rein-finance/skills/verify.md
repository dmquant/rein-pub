---
name: verify
description: Adversarial verification of a finished attempt's claims — verdict per claim, the harsher verdict wins.
applies_to: verify
output_schema: rein.verdicts/v1
validator_refs: [ops-discipline@1]
authority_ceiling: proposal
---
# Verify — the challenger's playbook

**Deliverable:** `verdict.json` (rein.verdicts/v1) — one verdict row per
challenged claim. The deterministic `finance:ops` hand enforces the
mechanical core; this playbook governs any model hand and, above all, the
operator preparing the inputs.

## 0. The two iron rules

1. **The challenger is a different hand than the producer.** A model
   checking its own work is proofreading, not verification. The meta input
   names the producer; the contract refuses a same-hand challenge.
2. **The harsher verdict wins.** When challenger and producer disagree,
   the record keeps the worse reading. Verification can only remove
   confidence, never add it.

## 1. Pin the inputs

- `claims` — the claims file under challenge (pin the artifact, or a
  distilled claims.json citing it), note containing `claims`.
- `meta` — `{producer_hand, verified_attempt_ref}`, note containing
  `meta`. The join key must resolve to a real attempt.
- Any *counter-evidence* captures the challenge will lean on — newer
  filings, disagreeing sources — pinned like all evidence.

## 2. The verdict vocabulary, used honestly

- `confirmed` — independent evidence in the pinned set supports the
  claim. Name the evidence; "sounds right" confirms nothing.
- `refuted` — pinned evidence contradicts it. Quote the contradiction.
- `inconclusive` — the pinned set cannot decide. This is the honest
  default for forecasts and for challenges run without new evidence, and
  it is not a failure: an inconclusive verdict MUST carry the refutation
  condition — exactly what observable would settle the claim.

## 3. How to actually challenge

- Attack the basis, not the arithmetic: does the cited capture really say
  what the claim says it says? Field-level re-reads catch more than
  recomputation.
- Attack the frame: period labels, fiscal-vs-calendar years, currency,
  LTM/NTM mixing — the classic silent killers.
- Attack the falsifier: is it observable, dated, and specific? "The
  market disagrees" is not a falsifier; propose a better one in the
  refutation condition.
- Check omission: what pinned evidence did the producer NOT use, and
  would it have changed the claim?

## 4. Evidence basis per row

Every verdict row carries its basis — direct references to the captures
read. A verdict without a basis is an opinion with a schema.

## Failure modes seen in practice

- The rubber stamp: all-confirmed with no evidence named.
- The lazy refute: disagreeing with a forecast because it is a forecast —
  forecasts are challenged on their basis and falsifier, not their tense.
- Challenging the producer's prose instead of the claims register.

## Quality bar

Each row could be defended to the producer face-to-face; every
`inconclusive` tells the next researcher exactly what to fetch.
