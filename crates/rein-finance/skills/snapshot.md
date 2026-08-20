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
# Snapshot

State what the pinned captures say — price, size, trajectory — with every
figure stamped and cited. Nothing enters that is not in an input.
