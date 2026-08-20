---
name: earnings-review
description: Post-print review — actuals vs guidance vs street, the surprise decomposed, the tone gap measured.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
authority_ceiling: proposal
---
# Earnings review — the post-print playbook

**Deliverables:** `dossier.md` and `claims.json`. Run within days of a
print. The question is never "was it good" — it is *what changed in the
evidence base*, and which house assumptions it touches.

## 0. Pin the triangle

Three vertices, all captured: the **actuals** (the new quarterly
statements), the **prior guidance** (last quarter's transcript), and the
**street** (the estimates capture as of just before the print). A review
missing a vertex says which one and reviews the remaining edge honestly.

## 1. The surprise, decomposed

Report actual vs guided vs street per headline metric — then decompose
the delta until it stops being a number and becomes a cause:

- Revenue surprise → which segment, price or volume, one-off or run-rate.
- Margin surprise → mix, input costs, scale, accounting — management's
  stated driver (quoted, cited) versus what the line items support.
- EPS surprise → operational vs below-the-line (tax, buyback count,
  one-offs) — an EPS beat made of share count is a different fact than
  one made of operating income.

Every cell cited; deltas computed, not adjectived.

## 2. The guidance ledger

A table of guidance given last call vs delivered vs newly guided — with
the language shifts. "Approximately $54B" becoming "at least $54B" is a
finding. New guidance enters as **forecast** claims with the next print
as their falsifier.

## 3. The tone gap

Read the transcript against the numbers: confidence vocabulary versus
sequential deltas; what analysts pushed on and whether the answer carried
numbers or adjectives; topics that vanished since last quarter (vanished
topics are findings). Quote short, cite exactly — paraphrase drifts.

## 4. What it touches

Close with the house-impact list: which valuation assumption slots this
print moves (FCF base, growth path rung, share count), and whether the
move crosses any falsifier already on file. A review that ends without
touching the book is a newspaper clipping.

## Failure modes seen in practice

- Reviewing the press release and skipping the statements — the release
  is marketing with numbers; pin the statements.
- Treating an estimate-beat as a guidance-beat: different vertex,
  different meaning.
- The tone section quoting from memory — transcript quotes come from the
  pinned bytes or not at all.

## Quality bar

A reader knows the surprise, its cause, the guidance drift, the tone
gap, and exactly which house numbers to revisit — each with a citation.
