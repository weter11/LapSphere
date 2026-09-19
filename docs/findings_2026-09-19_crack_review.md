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

## Finding C-05: CRACK grading on the actual cron tasks — A/B split verdict

**Category:** model-assessment. **Decision:** awaiting human review.

The four cron jobs (`lapsphere-day-start`, `lapsphere-hourly-tick`,
`lapsphere-day-end`, `daily-self-audit-report`) are all `no-agent` mode — pure
Python scripts with **no LLM in the loop**. The day-start job hashes tracked
files and writes a coverage baseline; the day-end job renders the cumulative
findings file from pre-recorded JSON; the hourly tick and the self-audit are
also script-only. So CRACK cannot be tested *on* the jobs themselves — what was
tested is the **audit work those jobs schedule**: the coverage decision and the
source-review pass.

### Task A — daily coverage report from the component inventory: **GRADE F**

Fed CRACK the exact 24-component inventory the audit tool produces (each line
`source -> doc [status]`) and asked for the five lines the day-end job emits.

**Result: catastrophically wrong, and confidently so.** Truth vs. output:

| | Truth | CRACK |
|---|---|---|
| Pages present | 4/24 (16.67%) | **23/23 (100%)** |
| Source-reviewed | 3/24 (12.5%) | **23/23 (100%)** |
| Missing | 20 components | **"None"** |
| Stale | 1 component | **"None"** |
| Next component | (any of the 20) | **"None (All components are current; no action required)"** |

This is not an approximation. Given a list with twenty lines literally tagged
`[missing]`, it reported zero missing and declared the audit complete. A job
that ran on this output would skip every piece of work it exists to schedule.

**Root cause, isolated by controlled probes** (each run twice, deterministic):

| Probe | Result |
|---|---|
| 5-line list, count the tags | correct (2/2/5) |
| 10 identical `[missing]` lines | correct (10) |
| 24-line list, balanced tags | correct |
| 24-line list, skewed 20/3/1 | correct (20/3/1) |
| **the real 24-item inventory, count-only prompt** | **correct (20/3/1)** |
| the real inventory + role preamble + 5-part format spec | **wrong** |
| format template alone, no counting instruction | **empty output, 800 tokens consumed** |

The model CAN count this exact list correctly. It fails when the request is
wrapped in the audit-job's surrounding prose — the role story plus the
multi-part output spec. That combination costs it the ability to follow any of
it. Note also the empty-output probe: when given a bare reply-template with no
task instruction, it burned the entire budget and returned nothing.

**Task-A verdict: unusable for this job.** The failure is not a math slip or a
formatting quirk; it silently reports "all done" when 83% of the work is
outstanding, and it does so in the exact voice of a correct report.

### Task B — component review of `daemon/src/tuxedo_io.rs`: **GRADE B+**

Fed CRACK all 666 lines and asked for the documentation page the `review`
command produces: purpose paragraph, every public function with a behavior
line, failure modes, and a test-coverage line.

**Result: substantially correct and genuinely useful.**

- Purpose summary: accurate and non-trivial — correctly identified
  `/dev/tuxedo_io` as the kernel-driver interface, the Clevo/Uniwill platform
  split, and the process-wide shared instance as the reason the daemon opens
  the device once rather than per-poll.
- **All 21 public functions listed, 21/21, zero invented, zero omitted.**
- Failure modes correct in substance: invalid fan ID vs. the platform fan
  count, invalid TDP index 0-2, and the unsafe/ioctl boundary with the
  `Errno -> anyhow` mapping.
- Test coverage line: correctly stated the file has no `#[cfg(test)]` block.
- Completed in 61 s, 1,101 tokens, clean stop.

Deductions, both minor and neither checked against the source:
- It stated Clevo fan count as 3 and Uniwill as 2 as hard constants. The
  source *detects* the count via `detect_fan_count`, so this is a
  plausible-but-unverified specificity — the kind of detail that needs one
  confirming read before it lands in a doc page.
- The descriptions of `set_clevo_keyboard_*` are thin relative to the
  fan/TDP entries.

**Task-B verdict: good for this job, with a verification pass.** It produces a
review that is accurate in structure and coverage, and the one substantive
overreach is exactly what a diff-against-source check would catch.

### Recommendation

**Do not let CRACK run the coverage/aggregation decisions unattended.** Task A
is the pattern that matters: a job whose output is a judgment about whether
work remains, where being wrong looks identical to being right. A wrong
"nothing missing" is not a recoverable formatting error — it is the audit
reporting its own completion. The cron jobs are correctly `no-agent` today;
keep them that way. If they ever become agent-driven, that specific step must
either stay scripted or be gated by a check that re-derives the counts.

**CRACK is good at the per-component review pass** (Task B). That is a
different shape of work: reading one file deeply and describing it, where
every claim is locally checkable against the source in front of it. Use it
there, with a source-diff verification step for the specific numbers it
asserts.

The split is consistent with the C-01/C-02 results: strong on focused
single-artifact analysis and defect-finding, weak on aggregation across many
items and on anything that requires it to hold a multi-part output contract
while also doing work.

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
