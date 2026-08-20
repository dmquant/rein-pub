# The Rein story — updates in plain language

**English** · [简体中文](STORY.zh-CN.md)

*This page tells you what Rein is and what has happened to it so far,
written for readers with no technical background. The technical reference
is the [README](../README.md).*

## What is this thing?

Imagine you hire a brilliant but overconfident research assistant to
analyze companies for you. The assistant is an AI. It works fast, writes
beautifully — and sometimes it makes things up, quotes pages it never
read, or announces "done!" while handing you an empty folder.

**Rein is the harness you put on that assistant.** The name means the
reins on an animal whose strength you rent but do not own. Rein doesn't
make the assistant smarter. It makes the assistant *accountable*:

- **Every number must carry a receipt.** Where it came from, when it was
  fetched, from which provider. A number without a receipt cannot even be
  written down — the forms simply have no box for it.
- **Everything goes into a notebook that cannot be erased.** Every step,
  every result, every mistake is written in ink. Not even the software's
  own author can quietly rewrite yesterday's page.
- **"I don't know" stays "I don't know."** When a run ends unclear, it is
  recorded as *unknown* — and nothing anywhere can quietly turn an
  unknown into a success. We checked: there is no such button, key, or
  command. It was never built.
- **Empty screens explain themselves.** "No results — nothing has run
  yet" is an answer. A blank page is a bug.
- **You can check everything later.** Any conclusion can be traced back,
  step by step, to the exact pages and filings it rests on — and the
  whole run can be replayed to prove nothing was tampered with.

## The story so far

### August 19, 2026 — Built in a day

Rein went from a design document to a working tool in one day, in five
planned stages: the rulebook (what counts as done, what counts as proof),
the unerasable notebook, the finance toolkit (fetching market data with
receipts, doing valuation arithmetic that must be re-checkable), the
rescue desk (a short menu of safe actions for stuck runs — with
"mark it as success anyway" deliberately not on the menu), and the
control room (a four-screen terminal dashboard).

That same day it valued its first real company — NVIDIA, from live
market data — twice: once by a simple, perfectly repeatable calculator,
and once by a real AI model whose homework had to survive eleven
automated inspections before it counted.

### August 19, evening — Standing on its own

Rein cut its last ties to in-house systems. A fresh copy now builds and
passes its full test suite on any machine, with no accounts, no
services, and no sibling projects required.

### August 20 — Spring cleaning, then a public face

Getting ready to open the doors: private details were scrubbed not just
from the current files but from the project's entire history; internal
design papers moved out of the repository; a proper open-source license
went in (use it under MIT or Apache-2.0, your choice). What ships is
what a stranger can safely read.

### August 20 — The 400-question exam

Overnight, the AI assistant sat a public benchmark of 400 real financial
research questions. It answered **398** — and for two questions it
produced nothing and was honestly marked as failed, rather than bluffing.
A second AI then graded every answer against a 0-to-4 rubric.

The headline score was **99.4%**. And here Rein's honesty rules apply to
its own report card: the grader is a cousin of the student, and there is
no official answer key — so the number is recorded as *an upper bound*,
not a triumph. The grader did prove it was paying attention: it caught
one answer that fell for a trick question built on a false premise, one
that cited information from after its allowed knowledge date, and one
that mixed up years. All of that — every answer, every grade, every
reason — is on file and checkable.

### August 20 — Smarter valuations

A valuation lives or dies by its growth assumptions. Rein's built-in
calculator used to assume a flat, cautious growth rate — honest, but
too timid to mean much. Now growth comes from evidence, in a strict
order of trust: **your own stated view** (if you provide one, it wins,
and it is filed with your name on it), otherwise **professional
analysts' published forecasts** (fetched, stamped, and filed), otherwise
the company's own history, otherwise a clearly-labeled default. Every
year of the forecast records where its number came from.

The five companies in the demo book were re-valued the same day under
the new rules — and the write-up says plainly what changed and why.

### August 20 — A friendlier control room

The dashboard learned some manners. Colors now mean things: green is
verified-good, red is failed, yellow is degraded, and *unknown* is a
loud purple — because "unknown" is the state most tempting to ignore.
Pressing **Enter** on any run now opens its actual results right there
on screen — the valuation, the answer, the grades it earned — read back
from sealed storage, so the screen can only show what was really filed.
A live activity indicator spins while work is running, and when a result
lands while you watch, a small announcement names the receipt behind it.

## The promises that never change

However the project grows, these stay fixed, and automated tests guard
each one:

1. There is no "mark as success" button. Anywhere. Ever.
2. "Unknown" never quietly becomes anything else.
3. Every number carries its receipt; every status names its proof.
4. The notebook is append-only — the past cannot be rewritten.
5. Empty screens say why they are empty.
6. Scores and grades never touch conclusions — a good report card
   cannot promote a bad answer.
