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
# Consensus check

Compare pinned estimate captures against the house assumptions. Estimates
are forecasts — mark them so; the disagreement, not the level, is the
finding.
