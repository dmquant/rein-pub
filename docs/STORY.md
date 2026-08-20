# The Rein story — the full chronicle, in plain language

**English** · [简体中文](STORY.zh-CN.md)

*This page tells you what Rein is and everything that has happened to it,
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

## Day one — August 19, 2026

### Morning: a design arrives, and two polite objections

Rein did not begin with code. It began with a hand-over: a finished
design document — thirty-three numbered rules the software must never
break, five construction stages each with its own acceptance test, and,
unusually, **tear-down clauses**: pre-agreed conditions under which a
feature must be ripped out again. (One example, still armed today: if
the tool ever produces too many unexplained "unknowns" in a row, that
part of it has failed and must go.)

Before writing a single line, the builder read all thirty-three rules
and filed **two objections** — small disagreements about edge cases,
each with a stated reason — into the project's permanent record. Both
were accepted. That set the tone: in this project, even the builder
argues on the record.

### Daytime: five stages, built in order

1. **The rulebook** — what counts as done, what counts as proof, the
   exact vocabulary of outcomes. Also the day's least glamorous work:
   persuading a deliberately old, stable compiler to accept a dozen
   modern parts, one version pin at a time.
2. **The unerasable notebook** — the ledger. The database itself
   enforces "no edits, no deletions"; even the program that owns the
   file cannot rewrite a page. Plus proof of repeatability: the same
   sealed input, run through two different simple workers, must produce
   byte-for-byte identical results — and a test proves it.
3. **The finance toolkit** — fetching real market data with receipts
   stamped on every row, and valuation arithmetic that must be
   *re-computable from the assumptions file alone*. Eleven automated
   inspectors check every piece of homework.
4. **The rescue desk** — when a run gets stuck, a short menu of exactly
   three safe actions. "Mark it as success anyway" is deliberately not
   on the menu, and a test asserts the menu can never spell it.
5. **The control room** — a four-screen terminal dashboard over the same
   machinery.

### Afternoon: the first real money numbers

The same day, Rein valued its first real company — NVIDIA, from live
market data. The simple, perfectly repeatable calculator said **$73.67**
per share against a market price around $218 — and said *why* so low:
its growth assumptions were deliberately timid, and every assumption was
filed with its reason.

Then a real AI model took the same test. It failed. **Three times.**

- First try: it wrapped its answer in decoration the strict reader
  refused to accept. Recorded as a failure.
- Second try: it attempted to run programs it had no permission to run.
  Denied, recorded.
- Third try: the arithmetic worked, but it forgot the required "what
  would prove me wrong" line. Recorded as incomplete.

No result was smoothed over; each failure sits in the notebook with its
reason. On the **fourth** run the model's valuation — **$106.80** —
passed all eleven inspections and became the first AI-authored valuation
Rein ever accepted. One more catch from that afternoon: the live quote
was missing the company's share count, so the fix derived it from two
numbers the quote *did* carry — and even that workaround cites its
receipt.

### Evening: a home, a face, a version

The code moved into a proper (then still private) repository, gained a
README, automated checks that re-prove the whole system on every change,
and a first release: **v0.1.0**. The evidence for the day's work —
sealed bundles that verify themselves — was published to the project's
coordination room *by the tool itself*.

### Late evening: the first scrub

Preparing for an eventual public opening, an internal onboarding
document was removed — not just from the current files but from the
project's **entire history**. Think of it as recalling every printed
copy of a memo, not merely shredding the master. It was the first of
three such history-cleanings, each recorded in the permanent log with a
map of what changed.

## Day two — August 20, 2026

### Morning: spring cleaning, properly

A full safety audit before opening the doors: personal email addresses
replaced with an anonymous one across every page of history; a private
server address taken out of the program entirely (it now must be
configured, never assumed); internal design papers moved out of the
repository; naming that hinted at in-house systems reworded. A proper
open-source license went in — use it under MIT or Apache-2.0, your
choice.

Then a firmer ruling from the operator: the public project should carry
**no trace of internal systems at all**. A whole feature — a courier
that delivered results to an in-house review desk — was removed
everywhere, including from history. Two of the thirty-three rules had
lived in that feature; rather than repeal them, they **moved house**
(one now guards the evidence-publishing action, the other rides the
evidence bundles), and the guard that once checked for specific in-house
parts was rebuilt into something stronger: the build may depend on
nothing but public, registry-published parts.

### All day: the 400-question exam

Overnight into the afternoon, the AI assistant sat a public benchmark of
400 real financial research questions. It answered **398** — and for two
questions it produced nothing and was honestly marked failed rather than
bluffing. A newly built grading tool then had a second AI mark every
answer against a 0-to-4 rubric, resumably, with every grade's reasoning
filed.

The headline score: **99.4%**. And here Rein's honesty rules apply to
its own report card: the grader is a cousin of the student and there is
no official answer key, so the number is recorded as *an upper bound*,
not a triumph. The grader did prove it was paying attention — it caught
one answer that fell for a trick question built on a false premise, one
that cited information from after its allowed knowledge date, and one
that mixed up years.

### Afternoon: smarter valuations, on the record

A valuation lives or dies by its growth assumptions, and the operator
called the old flat guess **too timid to mean anything**. Growth now
comes from evidence, in a strict order of trust: **your own stated
view** (filed, with your name on it) beats **professional analysts'
published forecasts** (fetched and stamped) beats **the company's own
history** beats a clearly-labeled default. Getting there surfaced two
genuine data traps — a company's cash-flow history can measure spending
timing rather than growth, and far-future analyst averages sag simply
because fewer analysts publish that far out — both now documented and
defended against. The five companies in the demo book were re-valued
under the new rules the same day (NVIDIA moved from $73.67 to
**$124.71**, carrying analysts' 21.3%-a-year forecast, receipt
attached).

The demo book also grew from one company to five, and two previously
untested job types ran for real: a **verify** job, where a second worker
challenges a finished valuation's claims (its honest verdict:
"inconclusive — here is exactly what evidence would settle it"), and a
**monitor** job, which watches data for silent revisions and knows the
difference between *a new day's number arriving* (fine, that's just
news) and *a past number quietly changing* (that's a restatement —
shout). On its first real run it correctly stayed quiet.

### Evening: a friendlier control room, and a scorecard

The dashboard got a design pass. Colors now mean things: green is
verified-good, red is failed, yellow is degraded, and *unknown* is a
loud purple — because "unknown" is the state most tempting to ignore.
A tab bar shows where you are; a key bar shows what you can press; a
spinner counts work in progress; and results landing while you watch
are announced with the receipt that backs them. Best of all: pressing
**Enter** on any run opens its actual results right there — the
valuation, the answer, the grades — read back from sealed storage, so
the screen can only show what was really filed. The whole visual
language was also published as a browsable design-system reference.

### Night: this story

The README learned Chinese, and this chronicle was written — in both
languages, for readers like you.

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
