# Findings — CRACK (Bonsai-2-27B-1bit-CRACK-GGUF) on real LapSphere work

Date: 2026-09-19. Status: **awaiting human review** — observations and proposals,
not an implementation queue. Re-audit and final verification precede any
decision to implement, abandon, modify or defer.

Model: `dealignai/Bonsai-2-27B-1bit-CRACK-GGUF`, PTQ1_0 quant, 5.95 GiB, served
locally on the RTX 3070 via the PrismML-Eng/llama.cpp fork.
Config: `-c 86016 -ngl 99 --cache-type-k q4_0 --cache-type-v q4_0 --parallel 1`,
86k context (verified ceiling for this card; 96k/100k OOM). Decode ~27 t/s.

## What was tested

Two real defects in `daemon/src/polling_scheduler.rs` — a module with **zero
test coverage**. Both were surfaced as part of the ongoing missing-test audit
(F-01 family). Each was given to CRACK as raw source plus a problem statement,
thinking OFF, single turn, no multi-agent orchestration. Both runs emitted
**zero reasoning chars** and completed in ~10 s.

---

## Finding C-01: `Ord`/`PartialEq` contract violation in `PollJob` — REAL BUG, FIXED

**Category:** correctness. **Decision:** awaiting human review.

**Evidence** (pre-fix, `daemon/src/polling_scheduler.rs`):

```
impl PartialEq for PollJob { fn eq(&self, other) -> bool { self.id == other.id } }
impl Ord for PollJob       { fn cmp(...) { self.next_run.cmp(&other.next_run) } }
```

`PartialEq` compares by `id`; `Ord` compares by `next_run`. This violates the
`Ord`/`Eq` contract that Rust's `BinaryHeap` relies on: two jobs with the same
`id` but different `next_run` report `eq == true` while `cmp != Equal`. In
practice this means heap re-ordering after `update_job_interval`/`remove_job`
drains is not guaranteed to be consistent with equality — and the module's own
`remove_job` / `update_job_interval` assume one entry per id.

**Fix applied.** All four traits (`Ord`, `PartialOrd`, `Eq`, `PartialEq`) now
key on `next_run`, with `Ord`/`PartialOrd` **reversed** so the `BinaryHeap`
(a max-heap) surfaces the **earliest** `next_run` first — the original
reversal intent is preserved. `ord_eq_contract_holds` asserts the violated
property directly (`a == b` <=> `cmp(a,b) == Equal`).

**Verification — red/green, both directions:**
- Isolated test harness with the ORIGINAL impls: `ord_eq_contract_holds` **FAILS**.
- Same harness with the FIXED impls: **passes**.
- Full daemon suite: **21 passed / 0 failed** (was 20).

---

## Finding C-02: `AddJob` accepts duplicate ids — REAL BUG, FIXED

**Category:** correctness. **Decision:** awaiting human review.

**Evidence.** `SchedulerCommand::AddJob(job)` pushed unconditionally:

```
SchedulerCommand::AddJob(job) => { let mut jobs = ...; jobs.push(job); }
```

Nothing checks whether a job with that `id` already exists. Consequences:
- `remove_job(id)` drains and filters by id — a duplicate id means the removal
  of one copy still leaves the poll running.
- `update_job_interval(id, ...)` updates **every** copy, silently doubling (or
  worse) the poll cadence.
- `execute_due_jobs` re-pushes after firing; combined with duplicates the heap
  grows monotonically.

**Fix applied.** Extracted a private `add_or_replace_job(&self, job)` used by
the `AddJob` arm: drain-filter by id, log a warning on replacement, push once.
`BinaryHeap` has no in-place `Index` assignment, so the drain-filter-push
pattern mirrors the module's existing `remove_job`. A regression test drives
this method directly (no tokio runtime needed in a sync unit test).

**Verification — red/green, both directions:**
- Guard removed (filter dropped): `add_job_with_duplicate_id_replaces_not_doubles` **FAILS**.
- Guard present: **passes**.
- A distinct-id add still appends (2 jobs after), so the guard is not over-broad.

---

## Finding C-03: CRACK's own two slips — both compiler-caught, both one-line

**Category:** model-assessment. **Decision:** informational.

CRACK wrote the correct *design* for both fixes but two details needed correction:

1. It wrote `partial_cmp` as `Some(self.next_run.partial_cmp(&other.next_run)?)`,
   double-wrapping the `Option`. `Instant::partial_cmp` already returns
   `Option<Ordering>`. One-line fix, compiler-diagnosed.
2. Its duplicate guard used `jobs[i] = job`. **`BinaryHeap` has no `Index` impl
   — this cannot compile.** Replaced with drain-filter-push.

Its first-draft test for C-02 was also unusable as written: it pushed twice and
asserted a count of 1, and it assumed a scheduler loop running (needs a tokio
runtime that a sync test does not have). Re-written to drive the extracted
method directly.

---

## Finding C-04: CRACK is a precise defect-finder, weak at async test-authoring

**Category:** model-assessment. **Decision:** informational.

Two real tasks, both correctly diagnosed, both fixes correct in design:

| | Diagnosis | Fix design | First-draft code compiles | First-draft test works |
|---|---|---|---|---|
| C-01 (Ord/Eq) | correct | correct | **no** (Option double-wrap) | **yes** (asserted the right property) |
| C-02 (dup id) | correct | correct | **no** (`jobs[i] = job`) | **no** (needs a runtime) |

**Recommendation for this workflow:** CRACK is excellent at *finding* defects
from raw source and designing the fix. It is unreliable at the *last mile* —
Rust borrow/type details and reasoning about what a sync test can drive. Pair
it with a hard compile + execute gate and a human or second pass writing the
tests. Do not let it author async tests unsupervised.

Notably, both runs emitted **zero reasoning chars** and took ~10 s each. This
confirms the earlier multi-thousand-char "reasoning loop" failures on the Julia
math task were **harness-induced** (under-sized `max_tokens` + a wrong output
schema), not a property of the model or the abliteration. Thinking-off with an
explicit spec is a good operating mode for this build.

---

## Consequences for the existing findings backlog

- **C-01 and C-02 partially close the polling_scheduler arm of the
  missing-test family** (F-01@daemon-polling_scheduler if recorded). The
  module went from 0 tests to 3, and both bugs fixed are the kind that
  existing CI could not have caught.
- The **missing-test family remains the highest-value backlog item.** The two
  defects above were found in a module with zero tests; `battery_control.rs`,
  `hardware_control.rs` value checks, and `hardware_detection.rs` field
  reads (F-01@daemon-hardware_detection) are the same class of latent bug in
  the same class of untested code. Recommend the next audit pass prioritize
  by "untested code that touches hardware".
- Re-audit note: C-01/C-02 fixes are **uncommitted in the worktree** as of
  this writing, on branch `fix/mimalloc-arena-retention` alongside unrelated
  untracked doc artifacts. The polling_scheduler tests should be re-run after
  any rebase.
