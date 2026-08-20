---
name: settle
description: Settle due valuations and forecasts against realized evidence — verdicts read, never invented.
applies_to: settle
output_schema: rein.settlements/v1
validator_refs: [ops-discipline@1]
authority_ceiling: proposal
---
# Settle — the settlement playbook

**Deliverable:** `settlement.json` (rein.settlements/v1). Settlement is
bookkeeping against reality: what did we predict, what happened, row by
row. The deterministic `finance:ops` hand derives verdicts mechanically;
this playbook governs the operator's inputs and any model hand.

## 0. The iron rules

1. **Verdicts are read off evidence, never invented.** `confirmed` and
   `contradicted` require a pinned realized-evidence capture that bears on
   the falsifier. No capture bears → the row cannot claim either.
2. **`expired_unobserved` only when nothing bears.** It means "the horizon
   passed and the pinned set contains no observation that decides it" — a
   statement about the evidence set, not a euphemism for "we forgot".
3. **The denominator is everything due.** Due valuations AND due forecast
   claims enter the table — settling only the winners is the oldest trick
   in the book, and the coverage arithmetic exists to kill it.

## 1. Due-ness

A thing is due when its horizon or falsifier date has passed. Build the
due list from the ledger (valuations carry horizons; claims carry dated
falsifiers), not from memory. Nothing not-yet-due enters — early
settlement is another word for cherry-picking the current mood.

## 2. Pin the realized evidence

- The `due` input: the register of due items (pin as a capture, note
  containing `due`), each with its falsifier text and horizon.
- Realized evidence: the filings/prints that decide each falsifier —
  pinned captures with as-of dates inside the settlement window.
- Prices for valuation settlements: the quote vintage at the horizon,
  not today's.

## 3. Reading a verdict

- Take the falsifier LITERALLY. "FY revenue below $200B" settles on the
  reported FY revenue line, not on adjusted variants — if the falsifier
  was sloppy, settle it literally and note the sloppiness; the lesson
  feeds the next valuation's falsifier quality.
- A restated figure settles on the figure as first reported within the
  horizon, unless the falsifier said otherwise; note restatements.
- Partial horizons (falsifier decidable early): early contradiction
  settles immediately; early confirmation waits for the full horizon.

## 4. The settlement table

Per row: the item ref, its falsifier, horizon, the deciding capture (or
"none bears"), the verdict, and one line of arithmetic where numbers
decide it. Aggregates report confirmed / contradicted / expired counts —
and the contradiction rate is a *result*, not an embarrassment: a book
that never contradicts is a book that never predicted anything.

## Failure modes seen in practice

- Settling against today's restated data — the point-in-time discipline
  exists precisely here.
- The silent shrink: ten due, six settled, four unmentioned.
- Verdict drift: "roughly confirmed" is not a verdict; the vocabulary is
  closed.

## Quality bar

A reviewer can recompute every verdict from the falsifier text plus the
named capture; the due list reconciles to the ledger; expired rows name
what evidence was missing.
