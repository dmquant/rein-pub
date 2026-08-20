---
name: dcf-valuation
description: Intrinsic valuation through an explicit FCF schedule, terminal value, and the mandatory EV→equity→per-share bridge.
applies_to: valuation
output_schema: rein.valuation/v1
validator_refs: [input-closure@1, numeric-consistency@1, bridge-completeness@1, falsifier-present@1, source-cutoff@1, coverage-denominator@1]
eval_set: internal-settled
authority_ceiling: proposal
requires_tools: [data.equity.fundamentals, data.equity.quote, compute.valuation.dcf, compute.valuation.bridge]
---
# DCF valuation

Produce `assumptions.json` (rein.assumptions/v1) and `valuation.json`
(rein.valuation/v1) as SEPARATE artifacts, plus `memo.md`.

1. Every numeric input is a slot `{name, value, unit, basis, status}`. The
   basis is a pinned capture digest, a cited claim, or a declared assumption
   with a justification. A bare number does not exist here.
2. Slots required: fcf_y1..fcf_yN (contiguous), discount_rate,
   terminal_growth, net_debt, minority_interest, associates, other_claims,
   share_count, market_price.
3. The valuation must recompute from assumptions.json alone — the
   numeric-consistency validator will do exactly that.
4. Route through the bridge: EV → equity (net debt with as-of, minority,
   associates, other claims) → per-share (count with method and as-of).
5. State implied value vs market (both as-ofs), a horizon, sensitivity on at
   least TV growth / discount rate / year-1 FCF, and one statable falsifier —
   or the valuation is not decision-ready.
6. Never state a post-cutoff year as fact. Mark forecasts as forecasts.
