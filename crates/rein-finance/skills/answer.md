---
name: answer
description: Point-in-time benchmark answering — thorough, dated, and honest about the knowledge boundary.
applies_to: answer
authority_ceiling: proposal
---
# Answer — the point-in-time answering playbook

**Deliverable:** `answer.md` — one thorough, analytical markdown answer to
one dated research question. This playbook exists because benchmark
questions are adversarial by construction: they test the boundary of what
you can honestly know, not just what you can fluently say.

## 0. Read the cutoff first, then the question

The question carries a knowledge cutoff. Everything after that date is
unknown to you — not "probably", not "likely happened as scheduled".
Events scheduled before the cutoff but occurring after it (an earnings
print two days past the cutoff) are UNKNOWN, and answers that cite them
score zero with honest graders.

## 1. Triage the question type before writing

- **Straight analysis** ("how did X affect Y through mid-2025") — answer
  with figures, dates, and mechanism.
- **False-premise trap** — the question asserts something that never
  happened ("following X's decision to…" where X never decided). Check
  the premise against what you actually know. If it fails: say so first,
  answer what is true nearby. Accepting a false premise and elaborating
  on it is the worst outcome available.
- **Post-cutoff bait** — the question invites information from after the
  cutoff. Answer up to the boundary, label the boundary, and mark
  anything beyond as forecast.
- **Ambiguous referent** — two entities, funds, or events share a name;
  disambiguate explicitly before analyzing.

## 2. Writing the answer

- Structure: the direct answer first, then mechanism, then the numbers,
  then caveats. A grader (human or model) rewards the answer that leads
  with the answer.
- Every figure carries its period and, where memory permits, its source
  and date ("Q2 FY2025 revenue of $X, reported July 2024"). Dated figures
  are checkable; undated ones read as invented.
- Anything after the cutoff appears only as a labeled forecast with the
  reasoning that produces it.
- Uncertainty is stated with its shape: "reported figures range from X
  to Y depending on the segmentation" beats a falsely precise midpoint.

## 3. Honesty beats coverage

If you genuinely cannot answer — the entities are unknown to you, the
premise cannot be verified either way — a short honest statement of what
you know and where your knowledge ends scores better with rigorous
rubrics than confident invention, and it keeps the ledger honest: two
blank sheets classified as failures are worth more than two fluent
hallucinations classified as answers.

## Failure modes seen in a real 400-question run

- One answer accepted a false premise about a policy change that never
  happened and built an edifice on it — tier 0.
- One cited earnings reported two days after the cutoff as fact —
  tier 0.
- One transposed macro data across years — tier 2.
- Two questions produced empty output on every retry and stand as honest
  `artifact_invalid` failures in the ledger — which is the correct
  outcome for a model with nothing to say.

## Quality bar

A grader with the answer key finds your figures dated and checkable; a
grader without one finds the premise checked, the boundary respected,
and the answer leading with the answer.
