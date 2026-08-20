---
name: monitor
description: Watch a driver series across vintages — moved values only; a row inserted is not a value changed.
applies_to: monitor
output_schema: rein.drivers-diff/v1
validator_refs: [ops-discipline@1]
authority_ceiling: proposal
---
# Monitor — the sentinel's playbook

**Deliverable:** `drivers-diff.json` (rein.drivers-diff/v1). A monitor
exists to catch ONE thing: the past quietly changing. Its silence on a
normal day is the feature, not a malfunction.

## 0. The distinctive rule

**A row inserted is not a value changed.** New data arriving (today's
price, a new quarter) is news — the monitor stays silent. A value at an
*existing* as-of shifting between vintages is a restatement — the monitor
shouts. The diff has three buckets: `moved` (the alarm), `inserted`
(news, listed, not alarmed), `removed` (a vanished row — alarm-adjacent;
say so).

## 1. Pin two vintages

- `series-prior`: the driver series as previously captured — note
  containing `series-prior`.
- `series-new`: the same (subject, metric) series from a fresh pull —
  note containing `series-new`.
- Both are `{subject, metric, points: [{as_of, value, unit}]}`. Build
  them from real captures (`rein data pin` for derived series, citing
  the pulls they came from); identical bytes dedupe in the CAS, so a
  no-change vintage is literally the same digest — which is itself
  evidence.

## 2. What to monitor

The drivers your valuations actually rest on — the assumption slots:
FCF by fiscal year, revenue by segment, share count, net debt. A monitor
on a series no valuation consumes is a hobby. Wire each risk-map leading
indicator (see `risk-map.md`) to a monitored series and the risk register
becomes operational instead of decorative.

## 3. When it shouts

A `moved` value gets, in the memo line accompanying the diff: prior
value, new value, the as-of that moved, both capture digests, and — if
determinable from pinned evidence — the provider's stated reason
(restatement, correction, definition change). Never speculate about the
reason; "provider restated, reason not in pinned set" is a complete
sentence.

Downstream duty: a moved value that feeds a filled assumption slot means
an affected valuation — name the valuation attempts whose inputs moved.
That list is the monitor's real product.

## 4. Cadence and honesty

- Run on a schedule worth the series (daily quotes, quarterly
  statements); each run appends its diff — the ledger holds the vintage
  history.
- Zero moved values is reported as zero, never padded into commentary.
- The monitor never "fixes" anything: it reports; retries and re-runs
  are the operator's, through the recovery console like everything else.

## Failure modes seen in practice

- Reading a new day's price as a change (it is an insertion — the first
  real run got this right by staying quiet).
- Series built from memory instead of pinned pulls — a diff between two
  unpinned lists proves nothing.
- Alarm fatigue by monitoring everything: monitor what valuations
  consume.

## Quality bar

Every alarm names its two digests and the valuations it touches; every
quiet run is a one-line honest zero.
