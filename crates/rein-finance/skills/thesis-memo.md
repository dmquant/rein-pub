---
name: thesis-memo
description: The decision-ready memo — one falsifiable thesis assembled from valuation, research, and risk artifacts by digest.
applies_to: research
output_schema: rein.claims/v1
validator_refs: [citation-closure@1, fact-vs-forecast@1, source-cutoff@1, coverage-denominator@1]
authority_ceiling: proposal
---
# Thesis memo — where the work becomes a position

**Deliverables:** `dossier.md` and `claims.json`. Everything upstream —
valuations, dossiers, consensus checks, risk maps — exists so this memo
can be short. It assembles them **by digest** into one falsifiable thesis.
Authority ceiling applies with full force: this memo proposes; it commits
nothing anywhere.

## 0. Pin the estate, not the vibes

Inputs are the upstream artifacts themselves: the valuation's
assumptions.json and valuation.json, the deep dossier, the consensus
check, the risk map — pinned by digest like any capture. The memo may
not introduce a load-bearing number that exists in no pinned artifact:
new evidence means a new upstream task first.

## 1. The thesis, in one falsifiable sentence

*"X is worth ~Y per share because A and B, unless C."* — with Y from the
pinned valuation, A and B from pinned research, and C an observable. If
the sentence cannot be written, the memo's honest conclusion is "no
thesis yet" plus the list of what is missing — that is a legitimate,
decision-relevant deliverable.

## 2. The variant view

State exactly where and why the thesis disagrees with the street, from
the consensus check: the metric, the year, the gap, and the evidence for
taking the other side. A thesis with no variant view is an index
position wearing a memo; say that too, if it is true.

## 3. The pre-mortem

One paragraph written from eighteen months out: *the thesis failed
because…* — forced to name the most probable failure path, not the most
comfortable one. Wire it to the risk map: which registered risk kills
the thesis, and what its leading indicator was doing at memo time (with
the citation).

## 4. The falsifier stack — three levels, all dated

1. **Thesis falsifier** — the observation that kills the whole idea.
2. **Milestone falsifiers** — the checkpoints on the way (next two
   prints: which numbers must hold).
3. **Valuation falsifier** — inherited from the pinned valuation, quoted
   verbatim by digest.

Every one names a date or a print. These become the settle registry's
rows; write them knowing settlement will take them literally.

## 5. Sizing the honesty, not the position

The memo does not size positions (nothing here has that authority). It
sizes the *evidence*: which legs rest on filled slots vs defaulted ones,
which claims are facts vs forecasts, what fraction of the pinned inputs
the thesis actually consumed. A thesis standing on defaults says so on
page one.

## Failure modes seen in practice

- The synthesis that quietly re-derives its own numbers instead of citing
  the pinned valuation — two sources of truth, drifting.
- A pre-mortem that names the smallest risk on the map.
- Falsifiers phrased as opinions ("sentiment deteriorates") — settlement
  cannot read moods.

## Quality bar

The thesis fits in one sentence; every number resolves to a pinned
digest; the falsifier stack could be settled by a clerk with the ledger
and a calendar.
