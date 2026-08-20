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
# Deep dive

Produce `dossier.md` and `claims.json` (rein.claims/v1).

1. A source is not evidence until its bytes are captured — cite captures by
   digest, never bare URLs. `[N]` in the dossier must resolve through
   claims.json citations to a capture. A word in brackets is not a citation.
2. Each claim carries kind (fact | forecast | scenario), the time it is
   about, its evidence, and what would refute it. No falsifier → the claim is
   a research candidate, never decision-ready.
3. Coverage adds up: every pinned input is consumed or withheld-with-reason.
   Captures per host are capped — syndication is not corroboration.
4. Never state a post-cutoff time as fact.
