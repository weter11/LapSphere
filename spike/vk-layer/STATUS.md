# STATUS — Vulkan frames layer: **WORKING**, and now measured

Last updated: 2026-10-14, branch `spike/vk-layer-stage1b`, base
`origin/lapsphere` = `ec7a359254113988cd728120a5f4a47fa9e46aed`.

Stage 1 (branch `spike/vk-layer-stage1`, PR #124, merged) proved that
`vkQueuePresentKHR` is intercepted on a real application. This stage answers the
three questions stage 1 explicitly left open: what the interception costs, whether
its numbers agree with MangoHud's, and what happens to it under the things that
happen to a game. **Nothing here is a frame time**, and no number below is
derived on paper: every one is read back out of a live segment, out of
MangoHud's own log, or out of the kernel's view of a process, and the commands
and raw files are in `TESTING.md`.

## One-line state

The present hook's own work measures **0.061 µs median** in a hot unthrottled
present loop and **0.130 µs median** under vsync at 165 Hz (3 of 3 runs each;
the reader's sample count equals the interval count in 8 of the 9 runs, the ninth
off by a single frame because the two counts come from two reader calls ~5 ms
apart). The unthrottled present rate cannot see the
layer at all — the throughput A/B scatters by ±2.5 µs per present — so the
measurement rests on the in-band samples plus out-of-band floors for the parts
they exclude, not on throughput. MangoHud and this layer, running on the same
presents, agree on the frame-time distribution to **0.006-0.009 % at p50** and on
per-frame durations to ±5 µs median, with the *timestamps* in MangoHud's log
carrying ~100-200 µs of its own jitter. The layer survives clean exit, `kill -9`,
three live resizes, two simultaneous instances, no reader, `SIGSTOP` and a
writer that cannot be created; three checks stay **NOT TESTED** and are named as
such below.

## The numbers, and what each one is

| What | Value | What it is exactly |
| --- | --- | --- |
| In-band hook work, unthrottled | median 0.050 / 0.061 / 0.090 µs, p99 0.12-0.15 µs, max ≤ 1.4 µs (one 5.9 µs outlier in 4096) | the layer's own sample of one present: `record()` plus the lock and the closure around it, from entry-clock-read to end of the ring write. Excludes the call down, `next_present_for`, and the unlock. |
| In-band hook work, vsync 165 Hz | median 0.120 / 0.130 / 0.141 µs, p99 0.81-0.88 µs, p99.9 15-18 µs, max 16.7-20.6 µs | same window; the long tail is the process being descheduled inside it (it is ~20x smaller unthrottled, where the loop never sleeps) |
| In-band hook work, vsync with MangoHud | median 0.120 / 0.121 / 0.130 µs, p99 0.62-0.73 µs, p99.9 12.1-13.7 µs, max 12.2-26.5 µs | same window with MangoHud below this layer in the chain |
| Out-of-band floors (same host, same session) | clock pair 30 ns (20-30 ns across sessions), writer mutex 10-30 ns, `record()` body 10-20 ns, mmap+copy of a 64 KiB segment 6.792 µs, `read_dir` 2.365 µs | `primitive_bench`: what the instrument and the reader's primitives cost when nothing else is running. These are the numbers that stop the clock read being read as the layer. |
| Throughput A/B (unthrottled, per present) | B−A −0.25 / −1.49 / −0.12 µs, C−A −1.32 / −0.26 / +0.72, D−A +1.87 / +1.36 / +0.49 (paired medians of three 3-rep batteries); single reps scatter ±2.5 to ±3.7 µs | a rate delta, not a frame time. It contains the *whole* hook (call down included) and it is too coarse to resolve it: see "M3 cannot resolve the layer" below. |
| Agreement with MangoHud | p50 6.0662 vs 6.0661 ms (164.85 fps both); p99 9.8543 vs 9.8243 ms; paired durations median +0.54 µs (limiter off) and +0.67 µs (limiter 120) | the same presents, both layers live, limiter off and limiter 120 |
| Reader poll cost at 60 / 240 Hz | 27-98 µs median per poll; 0.25-2.875 % of one core | one `--watch` snapshot: directory scan + one mmap+copy per segment + selection, at the rate the panel would poll |
| Data age the reader sees | 0.001-0.016 s at a 60 Hz writer, 0.000-0.003 s at 240 Hz | `last_present_ns` age, sampled 15x with `--list` |

## Two reader defects found by measuring, and the checks that now hold them

1. **`--watch` reported a "read cost" that did not contain the read.** The
   timestamps sat on either side of `pick_active`, so the mmap + copy of every
   segment — 64 KiB, the dominant cost — was outside the window: the measured
   value was `median 0.0 us` while the code was copying 64 KiB per sample. The
   window now opens before the read, and check `D1` asserts the reported median
   is at least the out-of-band mmap+copy floor for the same 64 KiB **on the same
   host in the same session** (measured: 101.6 µs vs a 6.792 µs floor; the
   old code could not pass this check on any host).
2. **A reader started before the application never saw its segment.** The
   segment list was scanned once, before the `--watch` loop, so the reader
   reported `max_frame_count=0` for its whole life while the segment file
   existed (measured: 65664 bytes on disk, reader reporting 0). It re-scans per
   sample now, and check `D2` starts the reader two seconds before the app and
   requires a non-zero count (measured: 406).

A third, smaller change came out of the same work: `--list` now prints
`pid_alive=` next to `alive=`, because the header's `WRITER_ALIVE` flag survives
a `SIGKILL` and a consumer reading only `alive=` would trust a dead writer. R2
measures exactly that: `alive=true  pid_alive=false` on the orphan, and
`--prune` (pid liveness + age) as the authority. That is a protocol property,
not a defect, and the flag is unchanged.

Checks `D3` are new invariants rather than regressions: an instrumented run must
yield one overhead sample per recorded interval (`samples=1179 intervals=1179`
measured) and no sample may be smaller than the clock pair it contains.

## What is proven, and by what

| Claim | Evidence |
| --- | --- |
| Real present interception (stage 1) | `QueuePresentKHR=900` for `vkcube --c 900`, exit 0, `intervals=899 = presents-1` |
| The layer's per-present work is sub-microsecond | in-band samples, 3 reps x 3 configurations; sample count == interval count in 8 of 9 runs (the ninth off by one frame, two reader calls apart) |
| It is not the instrument's own cost | `primitive_bench` floors for the same primitives on the same host: clock pair 30 ns (20-30 ns across sessions), mmap+copy 6.792 µs |
| The measurement does not change the layer's behaviour | `intervals == presents - 1` holds in every instrumented run, from the writer's own final line |
| MangoHud and this layer measure the same frame times | same presents, both live: p50 within 1 µs (0.009 %), p99 within 0.3 %, paired durations ±5 µs median, 4 runs over 2 cadences |
| Clean exit | exit 0, `intervals == presents-1`, segment unlinked (R1, R5) |
| `kill -9` | status 137, orphan segment 65664 bytes readable, no `writer_final`, `--prune` removes it, next run exit 0 (R2) |
| Window resize / swapchain recreation | 3 resizes under Xvfb: 2500 presents, 4 swapchains, `intervals = 2500-1-3 = 2496` **HOLDS**, zero intervals above 3x p50 (R3); Wayland WSI recreates once: `798 = 800-1-1` **HOLDS** (R3b) |
| Two instances at once | 2 segments, 2 pids, reader picks the freshest; killing one leaves the other running to exit 0 (R4) |
| No reader required | exit 0, presents counted, segment unlinked at exit, no reader ever started (R5) |
| Freeze/resume | `SIGSTOP` freezes the count and the age grows; `SIGCONT` continues; exit 0 (R8) |
| A writer that cannot be created is not fatal | unwritable segment dir: app exit 0, no segment created, error text captured from a process that prints it (R7) |
| Reader cost at 60 and 240 Hz | 27-98 µs per poll, 0.25-2.875 % of one core, zero seqlock retries in the quoted battery (one run of an earlier battery saw 10 in 1920 samples against a full ring) |

## What is NOT proven (do not read the above as more than it is)

* **No real game, no DXVK/Proton/Wine, no OpenGL, no 32-bit, no anti-cheat.**
  The only application exercised is still `vkcube`, plus a synthetic writer that
  drives the same `shm::Writer` without Vulkan.
* **No 240 Hz *present* load.** The panel is 165 Hz. "240 Hz" below always means
  either the reader's poll rate or a synthetic writer's cadence, and the two are
  labelled separately every time.
* **No interactive host resize.** The host session exposes no window control to
  this session (`wmctrl -l` 0 lines, `xdotool search --all --class .` 0 windows
  while an xlib `vkcube` was running), so the resize was measured under
  Xvfb+lavapipe, where the frame rate is software-bound and the resize is a
  ConfigureWindow from the client rather than a compositor drag.
* **The pid-reuse `O_EXCL` variant of R7 is NOT TESTED.** Producing it needs a
  pid namespace and unprivileged ones are unavailable here (`unshare -p -f
  --mount-proc` → Operation not permitted; `unshare -U -p -f` → cannot change
  root filesystem propagation). What *is* tested is the neighbouring failure:
  an unwritable segment directory.
* **The throughput A/B cannot resolve the layer.** Its single-rep scatter is
  ±2.5 to ±3.7 µs across three batteries, i.e. ±1.2-1.8 % of a ~208 µs
  unthrottled present, while the layer's own work is 0.05-0.14 µs. It is reported
  as a bound and as a check that nothing large is hidden — not as the overhead.
* **Overhead was measured on one GPU and one WSI** (`--gpu_number 0`, `--wsi
  xlib`, NVIDIA dGPU, 165 Hz vsync or IMMEDIATE) and on one host with
  `schedutil`. The reader-cost numbers are likewise host-specific.
* **Agreement does not cover a non-vsync cadence under a real game**, and it is
  agreement about the *same present hook boundary*, not about the compositor's
  scanout timing. MangoHud's own `fps_limit` shapes the cadence it measures.
* **The in-band number excludes** the unlock, `next_present_for` (two mutex
  lookups plus scans), the periodic log line every 600 presents, `bump()`, and
  the call down into the next layer. Those are priced out-of-band in
  `primitives.txt`, not measured in place.
* **Two devices in one process** is still untested; two *processes* with one
  device each is what R4 covers.
* **The layer's own diagnostic log is unreadable when the segment directory is
  unwritable** — `log_line` writes into that directory. R7 shows the failure
  path with its message taken from a process that prints it to stderr.
* **Nothing was changed outside `spike/vk-layer/`.** No GUI, panel, layout,
  settings, daemon or FPS-in-UI work; PRs #111 and #115 untouched.

## Layout

| Path | Purpose |
| --- | --- |
| `src/vkraw.rs` | raw Vulkan + loader declarations, `#[repr(C)]` with compile-time offset/size assertions against `vulkan-sdk-1.4.341.0` `vk_layer.h` |
| `src/lib.rs` | negotiation, chain handling, instance/device dispatch, the `vkQueuePresentKHR` hook |
| `src/shm.rs` | shm protocol v1 writer (128-byte header, 4096-slot ring, 4096-slot overhead buffer, seqlock) |
| `src/bin/frames_reader.rs` | independent reader (`--list`, `--dump`, `--watch`, `--overhead`, `--prune`); read-cost window and per-sample rescan fixed in 1b |
| `src/bin/primitive_bench.rs` | **new**: out-of-band floors — clock pair, writer mutex, real `record()`, mmap+copy, `read_dir` — the control that separates instrument cost from layer cost |
| `src/bin/frames_synth.rs` | **new**: a synthetic writer at an exact 60/240 Hz over the same `shm::Writer`, for reproducible reader loads |
| `manifests/` | implicit-layer manifest, plain and with the legacy `functions` map (the A/B) |
| `build.sh` / `install.sh` / `run.sh` | build with a compiled-in build id, install the manifest, one instrumented run |
| `bench.sh` | **new**: records the floors with the environment they were taken in |
| `overhead.sh` | **new**: experiment 1 — arms A/B/C/D, in-band samples (M1) and throughput (M3) |
| `agreement.sh` | **new**: experiment 2 — MangoHud's per-frame log against this layer's intervals on the same presents |
| `robustness.sh` | **new**: experiment 3 — R1..R8 |
| `run-battery.sh` | **new**: runs every experiment once, serially, on a committed tree |
| `verify.sh` | the acceptance harness: stage-1 checks C1-C5 plus D1-D3 |
| `TESTING.md` | reproduction recipe, expected output, every result table, limitations |

Own `[workspace]`, so root `cargo test` / `cargo build --all` never pick it up
(verified: the root `Cargo.toml` members are unchanged). The crate has no unit
tests: its acceptance is `verify.sh`, which needs a display, a GPU and the
loader, so CI cannot run it — CI staying green is not evidence about this crate.

## Next, when this is taken further

Overhead under DXVK/Proton and a real title; agreement at a non-vsync cadence;
an interactive resize on the real session; 32-bit and OpenGL; two devices in one
process; and the reader's 32-95 µs poll cost is worth a look on its own before
anything polls it at 60 Hz in a UI. The layer still has to be *installed* on a
host; whether the product ships an implicit layer at all remains the owner's open
question (round-1 `panel-design.md` Q13), and this stage does not decide it.
