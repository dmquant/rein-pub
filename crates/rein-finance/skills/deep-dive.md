---
name: deep-dive
description: Source-grounded research dossier with closed citations and honest coverage.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
eval_set: financegym
authority_ceiling: proposal
requires_tools: [research.search, research.visit]
---
# Deep dive — the playbook (single-pass variant)

**Deliverables:** `dossier.md` and `claims.json` (rein.claims/v1). This is
the single-pass sibling of the staged `deep-research` method — use it when
the question is narrow enough that one synthesis over the pinned sources
answers it. If you find yourself planning sections, use the staged skill.

## 1. Evidence before prose

- A source is not evidence until its bytes are captured. Cite captures by
  digest through the claims register — never bare URLs, never memory.
- Web material enters via search → visit → capture; captures per host are
  capped, because syndication is not corroboration: two copies of one
  press release are one source.
- If the pinned set cannot answer the question, the dossier's first
  paragraph says exactly that, names what is missing, and stops inflating.

## 2. The dossier

- Open with the answer, cited — not with background.
- Every factual sentence carries [N]; a word in brackets is not a citation
  and closes nothing.
- Numbers keep their period labels and units exactly as captured.
- Distinguish, in the text itself: what the sources SAY (cited), what you
  INFER from them (marked as inference, with the inputs named), and what
  you ASSUME (justified). Three different verbs, never blurred.
- Post-cutoff years appear only on lines that are marked
  (forecast/scenario/reported/ended) or carry a citation — the unmarked,
  uncited post-cutoff assertion is unfalsifiable and fails.

## 3. The claims register

- Each claim: `kind` (fact | forecast | scenario), the time it is about,
  its evidence numbers, and **what would refute it**. A claim with no
  statable falsifier is a research candidate, never decision-ready — and
  saying so in the dossier is better than faking precision.
- Facts cite captures. Forecasts name the settling observation and date.
  Scenarios name their trigger conditions.
- Six to fourteen claims: fewer means the dossier asserts nothing;
  more means it hasn't chosen what is load-bearing.

## 4. Coverage adds up

Consumed + withheld = pinned, every withholding with a reason a reviewer
could reject ("duplicate of [2]", "period outside scope" — not "unused").
The denominator is the honesty of the whole exercise.

## Failure modes seen in practice

- The confident dossier over four thin sources — depth cannot exceed the
  evidence pinned; pin more or claim less.
- Citation decoration: [N] sprinkled at paragraph ends instead of on the
  sentences carrying the numbers.
- The unfalsifiable flourish ("well positioned for the AI era") smuggled
  in as a fact claim.

## Quality bar

Every load-bearing sentence survives the question "which capture, which
field?"; every claim survives "what would prove this wrong?"; the coverage
table survives "where did source 7 go?"
