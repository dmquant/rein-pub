---
name: filing-review
description: Systematic read of a filing — segments, accounting changes, footnote flags, and language drift, all cited.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
authority_ceiling: proposal
---
# Filing review — the systematic read

**Deliverables:** `dossier.md` and `claims.json`. A filing review reads
what companies are obliged to write but hope goes unread. Pin the filing
itself (fetched and captured — a filing quoted from memory is not in
scope) plus, ideally, the prior period's filing for the diff.

## 1. The reading order (deliberately not page order)

1. **Auditor and accounting changes first** — a new auditor, a changed
   revenue-recognition policy, a shifted useful-life assumption: each is
   a finding before a single number is read, because it changes what the
   numbers mean.
2. **Segments** — revenue and margin by segment vs prior period; the
   consolidation seams (what moved between segments, and did the
   definition change — a segment redefinition can manufacture growth).
3. **The cash walk** — earnings to operating cash to free cash: where
   the gap lives (receivables, inventory, capitalized costs) and its
   direction across periods.
4. **Footnotes with teeth** — commitments and contingencies, related
   parties, concentration disclosures, off-balance obligations, purchase
   obligations, litigation ranges. Quote the numbers; these notes are
   where exposure hides.
5. **Language drift** — against the prior filing: risk factors added or
   quietly dropped, hedging words appearing around previously firm
   statements, "substantial doubt" vocabulary anywhere near liquidity.
   A dropped risk factor is as informative as an added one.

## 2. Rules of evidence

- Every observation cites the filing capture, with the item or note
  number in the claim's locator — an auditor should land on the page.
- Numbers copied exactly, with period labels and units; derived deltas
  show their arithmetic.
- What the filing does NOT contain is stated when it matters ("no
  customer-concentration disclosure this period; prior period disclosed
  one at 22%") — with the prior filing cited for the contrast.
- No speculation about intent: report the change and its mechanical
  consequence; motive is not in the pinned set.

## 3. Claims register

Facts for what the filing states (cited to item/note); scenario claims
for exposures with trigger conditions (a contingency resolving adversely,
a covenant tripping) — each with the falsifier being the future filing or
event that settles it. Feed material exposures to the risk map, and
segment-level series to `monitor`.

## Failure modes seen in practice

- Reading the MD&A narrative and skipping the notes — the narrative is
  the company's dossier about itself; the notes are the evidence.
- Missing a definition change and "discovering" segment growth.
- Flagging boilerplate risk language as signal — the finding is the
  *drift*, not the existence, of risk factors.

## Quality bar

Every claim lands on a page; the diff against the prior filing is
explicit; anything that should feed the risk map or a monitor series is
handed off by name.
