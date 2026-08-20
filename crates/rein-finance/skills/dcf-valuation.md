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
# DCF valuation — the playbook

**Deliverables:** `assumptions.json` (rein.assumptions/v1) and
`valuation.json` (rein.valuation/v1) as SEPARATE artifacts, plus `memo.md`.
The split is the design: assumptions face the research validators;
the valuation must recompute from the assumptions file alone.

## 0. What to pin before you start

At minimum: the quote (price, market cap), the cash-flow statement
(multi-year — the FCF base AND its history), the balance sheet (net-debt
components), and analyst estimates (the forward growth evidence). Quarterly
statements sharpen the trajectory; an earnings-call transcript justifies
qualitative assumptions. A capture you did not pin cannot be cited, and an
assumption you cannot cite must carry its own justification.

## 1. Slots — every number is a slot, and a bare number does not exist

Each numeric input is `{name, value, unit, basis, status}`:

- `basis` is exactly one of: a pinned capture digest with the field it came
  from, a cited claim, or a declared assumption with a justification that a
  reviewer could argue with.
- `status` is `filled` (derived from a capture) or `defaulted` (you supplied
  it) — and defaults are COUNTED; the coverage denominator is real.

Required slots: `fcf_y1..fcf_yN` (contiguous), `discount_rate`,
`terminal_growth`, `net_debt`, `minority_interest`, `associates`,
`other_claims`, `share_count`, `market_price`.

## 2. The growth path — provenance order, strictly

1. **Operator-pinned view** (a `growth` capture): if present, it wins, and
   its digest is the basis. Operator authority; no clamp.
2. **Analyst estimates**: endpoint CAGR of `revenueAvg` across the forward
   window, clamped [−10%, +40%]. Beware the two traps proven in practice:
   interior-year averages sag when analyst coverage thins (endpoint CAGR
   ignores interior rows on purpose), and net-income averages dip on
   coverage mix — revenue carries the breadth.
3. **FCF history**: oldest-to-newest CAGR, clamped [0, 25%]. Know what this
   measures: a capex ramp can hold reported FCF flat while the business
   compounds — history is a fallback, not a forecast.
4. **A stated default**, labeled as such.

Whichever rung supplies it, every forecast year's justification names the
rung, the formula, any clamp applied, and the source digest.

## 3. Derivations are still citations

A missing field is derived from fields the capture DOES carry, citing the
same capture — e.g. `share_count = market_cap ÷ price` when the quote
omits shares outstanding. The derivation formula goes in the
justification. Never a bare number, even in a workaround.

## 4. The bridge, mandatory and explicit

EV → equity (net debt with its as-of, minority interest, associates, other
claims — zeros are fine when stated) → per-share (share count with method
and as-of). EV-level and equity-level quantities never mix silently; the
`bridge-completeness` validator holds the chain.

## 5. Decision-ready or say why not

State: implied per-share vs market (both as-ofs), a horizon, sensitivity on
at least {terminal growth, discount rate, year-1 FCF}, and **one statable
falsifier** — the observable outcome that would prove the valuation wrong
(e.g. "FY next reported FCF below X" — not "market disagrees"). Missing any
of these, the valuation is not decision-ready, and `falsifier-present`
will say so.

## 6. How the validators will read your work

- `numeric-consistency` recomputes the entire DCF from assumptions.json;
  a transcription error between artifacts is a failure, not a rounding note.
- `input-closure`: every slot basis resolves inside the pinned inputs.
- `source-cutoff`: no capture newer than the epoch's cutoff.
- `coverage-denominator`: filled + defaulted = the real denominator.
- Never state a post-cutoff year as fact; forecasts are marked forecasts.

## Failure modes seen in practice

- The flat-growth trap: one buried constant made every valuation timid and
  none of them meaningful. Growth is evidence, ranked as in §2.
- The absurd per-share from a missing shares field — fixed by §3, not by
  typing a number from memory.
- Terminal value dominating EV (>85%): not forbidden, but say it in the
  memo and let the sensitivity row carry it.

## Quality bar

A reviewer holding only `assumptions.json` can rebuild `valuation.json` to
the cent; every slot's provenance survives an argument; the falsifier names
a date and a number.
