---
name: risk-map
description: A risk register with mechanisms, exposed line items, leading indicators, and triggers — wired for monitoring.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
authority_ceiling: proposal
---
# Risk map — the register that can actually fire

**Deliverables:** `dossier.md` and `claims.json`. A risk that cannot be
watched is a mood. Every risk in this register carries a mechanism, the
line items it would hit, a leading indicator someone can monitor, and a
trigger — which makes the register an input to `monitor` tasks, not a
disclaimer page.

## 1. Source the risks, don't brainstorm them

Risks enter from pinned evidence: the risk factors the filings state,
the constraints management concedes on calls (supply, regulatory,
customer concentration), what the balance sheet implies (maturities, FX,
inventory builds), and what the estimates assume away. Each risk cites
where it came from. A risk with no source is labeled a **hypothesis**
and counted separately.

## 2. The row schema — five fields, all mandatory

1. **Mechanism** — the causal sentence: *X happens → Y line item moves
   because Z*. If the mechanism cannot be written, the risk is not
   understood yet; say so.
2. **Exposure** — the statement lines and assumption slots it hits, with
   current values cited (concentration risk names the actual receivables
   share; rate risk names the actual maturity wall).
3. **Leading indicator** — an observable series that moves BEFORE the
   damage: inventory days, a customer's own capex guidance, a spread.
   Prefer indicators derivable from pinnable data; each becomes a
   candidate `monitor` series.
4. **Trigger** — the indicator level at which the risk is considered
   firing: a number and a direction, not "deterioration".
5. **Severity, honestly qualitative** — sized against the exposure cited
   (percent of revenue at risk beats high/medium/low theater).

## 3. Interactions and concentration

The dangerous risks travel together: name the pairs that share a cause
(one customer = concentration + receivables + guidance all at once), and
the single points of failure where several mechanisms converge on one
line item.

## 4. Claims register

Each material risk becomes a **scenario** claim: trigger conditions as
stated, falsifier = the observation that would retire the risk (a
maturity refinanced, a second source qualified, a concentration
diluted). Retired risks stay in the register, marked retired with the
retiring capture — the history of dead risks calibrates the next map.

## Failure modes seen in practice

- The disclaimer page: ten risks, zero indicators, nothing monitorable.
- Severity theater: high/medium/low with no exposure arithmetic.
- Copying filing risk-factors verbatim — filings enumerate for liability,
  not for materiality; select and size, with citations.

## Quality bar

Every risk could be handed to a `monitor` task tomorrow (series +
trigger); every severity survives "show me the exposed number"; the
retired list is nonempty within a year or the map was never live.
