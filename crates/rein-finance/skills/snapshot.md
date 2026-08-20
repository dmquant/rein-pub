---
name: snapshot
description: One-screen state of an instrument from pinned captures only.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.quote, data.equity.fundamentals]
---
# Snapshot — the playbook

**Deliverables:** `dossier.md` (one screen — this is a discipline, not a
limit to pad toward) and `claims.json`. A snapshot answers: *what do the
pinned captures say this instrument is, right now* — nothing more, and
provably nothing more.

## 1. The one-screen structure

1. **Identity & price** — ticker, exchange, last price with its retrieval
   time, day move, 52-week range, market cap. All from the quote capture,
   all cited [N].
2. **Size & trajectory** — latest annual revenue, FCF, and their
   year-over-year deltas from the statements capture. A number without its
   period label is not a number.
3. **Balance-sheet posture** — cash, total debt, the net position, one
   line.
4. **Valuation markers** — whatever the captures actually carry (P/E from
   the quote, EV if derivable with the bridge inputs present). Do NOT
   derive what the captures cannot support.
5. **What the snapshot cannot see** — one closing line naming what is
   absent from the pinned set (no transcript pinned → "no management
   commentary in scope"). Absence is stated, never padded over.

## 2. Rules that make it a snapshot and not an essay

- Present tense, no narrative arcs, no adjectives that a number could
  replace ("large" is not a market cap).
- Zero forecasts. If a pinned capture contains estimates, they may be
  *quoted as estimates with their source cited* — the snapshot itself
  predicts nothing.
- Every figure cited [N]; every [N] resolves to a pinned capture digest
  through claims.json. A word in brackets is not a citation.
- Derivations allowed only from fields inside one capture (e.g. net debt =
  totalDebt − cash), citing that capture, formula stated in the claim.

## 3. Claims register

Three to six claims, all `kind: fact`, each with evidence — the load-bearing
numbers a reader would act on (price/size/trajectory/net cash). Falsifiers
here are trivially "the capture says otherwise on re-read"; prefer stating
the restatement risk: "a provider restatement of FY revenue would void c2."

## 4. Coverage

Every pinned input is consumed or withheld-with-reason. A snapshot pinned
with six captures that cites two and says nothing about four will fail —
correctly.

## Failure modes seen in practice

- The snapshot that quietly became a thesis — forecasts belong to
  consensus-check or a deep dossier, never here.
- Deriving EV without minority/associates present, silently — either pin a
  balance sheet or state the omission.

## Quality bar

A reader gets the state of the instrument in under a minute; an auditor
can verify every figure in under five.
