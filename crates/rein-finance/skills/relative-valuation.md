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
# Relative valuation

The peer list is an input you must justify — never inferred. No
cross-currency aggregation without a stated FX rate and as-of; no LTM/NTM
mixing; negative denominators are excluded AND counted. EV-level multiples
imply EV and go through the bridge; equity-level multiples imply equity.
