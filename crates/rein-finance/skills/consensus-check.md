---
name: consensus-check
description: Where the house view and street estimates disagree, with the disagreement quantified.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.estimates]
---
# Consensus check — the playbook

**Deliverables:** `dossier.md` and `claims.json`. The finding is the
**disagreement**, quantified — not the level of either side. A consensus
check that concludes "analysts expect growth" has found nothing.

## 1. Pin both sides

- The street: an analyst-estimates capture (know its shape — averages per
  forward fiscal year, with analyst counts per row).
- The house: the assumptions file of a pinned valuation attempt, or an
  operator-pinned view (`rein data pin view.json --note house-view`).
  A house view that exists only in your head is not in scope.

## 2. Align frames before comparing

- Fiscal labels vs calendar years: compare period-end dates, not labels.
- Revenue vs FCF vs EPS: convert nothing silently. If the house models FCF
  and the street publishes revenue, compare *growth rates* and say the
  proxy out loud.
- Note the analyst-count column: a far-year "consensus" of four analysts
  is a straw poll, and the dossier says so. Interior-year average dips that
  track coverage drops are artifacts, not forecasts — flag, don't
  interpret.

## 3. Quantify the gap

For each compared metric-year: house value, street value, absolute and
percentage gap, and — the useful number — **what the gap implies**: the
growth rate the market's own price needs vs the street's path vs the
house's. One table, every cell cited.

## 4. Classify each disagreement

- `data` — different vintages or definitions (resolve it; not a finding).
- `path` — same destination, different years (timing risk).
- `thesis` — genuinely different views of the business (THE finding; write
  the paragraph on what evidence would settle it).

## 5. Discipline

- Estimates are **forecasts** — every street number is marked so, and every
  claim about the future carries kind `forecast` with a falsifier naming
  the settling report and date ("FY2028 revenue print below $X").
- The house view is not privileged: where the street has better evidence,
  the dossier says the house should move — this skill audits both
  directions.
- Post-cutoff years appear only marked or cited (the fact-vs-forecast rule
  is watching).

## Failure modes seen in practice

- Comparing FY2027 (January fiscal) against CY2026 street rows by label.
- Reading a thin-coverage out-year dip as "the street sees deceleration."
- The uncommitted conclusion: a gap table with no thesis classification is
  a spreadsheet, not a check.

## Quality bar

A reader knows, per metric-year: the gap, its class, and the observable
that would settle it. Both sides' numbers trace to captures.
