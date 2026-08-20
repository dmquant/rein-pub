---
name: relative-valuation
description: Peer-multiple triangulation with frame discipline and counted exclusions.
applies_to: valuation
output_schema: rein.valuation/v1
validator_refs: [input-closure@1, bridge-completeness@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [compute.valuation.comps, compute.valuation.bridge]
---
# Relative valuation — the playbook

**Deliverables:** `assumptions.json`, `valuation.json`, `memo.md`. A
multiple is a fraction of two stamped numbers; a peer set is a judgment
you must defend. Everything else follows from taking both seriously.

## 1. The peer list is an input, never an inference

Declare it as a justified assumption slot: which names, and *why these* —
business-model comparability beats sector labels. Excluding an obvious
candidate is fine when the reason is stated ("hardware-attach revenue mix
makes multiples incomparable"). An unstated exclusion is silent truncation,
and the coverage denominator will not add up.

## 2. Frame discipline — comparisons refuse across disagreeing axes

Every input carries its frame: currency, period (LTM vs NTM vs fiscal
labels), accounting basis, unit scale. The hard rules:

- **No cross-currency aggregation** without a stated FX rate and its as-of,
  filed as a slot like any other.
- **No LTM/NTM mixing** inside one multiple set. If the peer set forces a
  mix, split into two tables and say so.
- Fiscal calendars differ (a January fiscal year is not a December one);
  align periods by end date, not by label.
- An axis absent on one side is unknown, not wildcard-compatible — say
  "not comparable" instead of forcing it.

## 3. Denominators, honestly

- Negative or near-zero denominators (loss-makers on P/E, EV/EBITDA at the
  cycle floor) are **excluded AND counted** — the exclusion appears in
  coverage with its reason, and the memo states how many peers survived.
- Outliers are winsorized or excluded with a rule stated *before* the
  numbers are computed, never after seeing them.
- Report median and range, not just mean; three peers do not make a
  distribution, and the memo should admit it.

## 4. The level and the bridge

- **EV-level multiples** (EV/EBITDA, EV/Sales) imply an EV — which must
  route through the full bridge (net debt with as-of, minority, associates,
  other claims) to equity and per-share.
- **Equity-level multiples** (P/E, P/B) imply equity directly — never
  double-bridge.
- The implied per-share is stated against market with both as-ofs, and the
  memo names which multiple carried the conclusion and why.

## 5. What this method can and cannot say

Relative valuation prices the company *against the peer set's current
mood*, not against cash. State the implied value as conditional ("at peer
median EV/EBITDA of X…"), give the falsifier in peer terms ("re-rating of
the set below Y"), and if the DCF and the comps disagree, the memo says so
and does not average them into mush.

## Failure modes seen in practice

- Sector-label peer sets that mix a fab, a fabless designer, and a
  hyperscaler because one index does.
- LTM earnings under NTM prices — the frame validator's reason for
  existing.
- The silent shrink: eight peers named, five used, zero explained.

## Quality bar

A reviewer can re-derive every multiple from the pinned captures; every
exclusion has a sentence; the bridge is complete; the conclusion names its
conditioning multiple.
