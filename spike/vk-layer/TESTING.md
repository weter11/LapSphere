# TESTING — how every number in `STATUS.md` was produced

Everything here was run on this host, against a binary whose build id is printed
by the layer itself. Nothing is a demonstration value: each figure is read back
out of a live shared-memory segment, out of MangoHud's own log, or out of the
kernel's view of the process.

Stages: **1** interception (merged, PR #124) and **1b** overhead, agreement with
MangoHud, robustness (this branch). `STATUS.md` carries the conclusions; this file
carries the method, the commands, the result tables and the limits, in that
order of authority.

## Environment (recorded, because every result is conditional on it)

| | |
| --- | --- |
| Host | AMD Ryzen 7 5800H (16 threads), 32 GB, `schedutil` governor |
| Loader | `/usr/lib/x86_64-linux-gnu/libvulkan.so.1.4.341` (`libvulkan1 1.4.341.0-1`) |
| Layers in the chain | `VK_LAYER_LAPSPHERE_frames` (this), `VK_LAYER_MESA_device_select`, `VK_LAYER_MANGOHUD_overlay_x86_64` when `MANGOHUD=1` |
| ICDs | `RADV RENOIR` (AMD iGPU), `NVIDIA GeForce RTX 3070 Laptop GPU`, `llvmpipe`/`lavapipe` |
| Display | `2560x1440 @ 165.003 Hz` (eDP-1), session `xfwl4` (Xfce Wayland) + Xwayland `:0` |
| Application | `vkcube` from `vulkan-tools 1.4.341.0+dfsg1-1`; `vkcube --c N` exits by itself after N frames |
| MangoHud | `0.8.2~rc2-1`, the user's own `~/.config/MangoHud/MangoHud.conf`, which sets `fps_limit=120,60,58,30,0` |
| Layer ABI source | `Vulkan-Headers` `vulkan-sdk-1.4.341.0` `vk_layer.h`; behaviour from `Vulkan-Loader` same tag `loader/loader.c` |

Two consequences that matter below:

* **`fps_limit`**. MangoHud is loaded on this host, and its config's first
  `fps_limit` entry is **120**. The "p50 8.334 ms = 120 fps" recorded in stage 1
  was *that limiter*, not the panel: with `MANGOHUD_CONFIG=fps_limit=0` the same
  binary on the same panel gives p50 6.0675 ms = 164.8 fps. Every run below says
  which limiter was in force.
* **The host session offers no window control.** With an `--wsi xlib` `vkcube`
  running, `wmctrl -l` prints 0 lines and `xdotool search --all --class .` finds
  0 windows on `:0`. Measured, not assumed — it is why the resize test runs under
  Xvfb (R3).

## Commands

```sh
cd spike/vk-layer

./build.sh                      # build, with the build id compiled in
./install.sh plain              # implicit-layer manifest (no legacy functions map)
./install.sh functions          # the control manifest (with the functions map)
./install.sh none               # remove the manifest again
./run.sh 60                     # one instrumented run + mapped .so + layer log
./verify.sh 0                   # acceptance harness: C1-C5 + D1-D3, ~2 min
VERIFY_LONG=1 ./verify.sh 0     # + the ~50 s ring-wraparound run

./bench.sh                      # out-of-band floors -> results/primitives.txt
./overhead.sh 3                 # experiment 1 (arms A/B/C/D, M1 + M3), ~12 min
./agreement.sh 2                # experiment 2 (MangoHud, 2 cadences x 2 reps)
./robustness.sh 2               # experiment 3 (R1..R8)
./run-battery.sh                # all of the above, serially, on a committed tree

./target/release/frames_reader --list | --dump out.csv | --watch --hz 60 --seconds 8 --cpu | --overhead | --prune
./target/release/frames_synth --hz 240 --seconds 20 --label x    # synthetic writer
./target/release/primitive_bench --iters 1000000 --io-iters 20000
```

`build.sh` prints the id it compiled in and the SHA-256 of the artifact:

```
build_id=<git HEAD, 12 hex>/<sha256 of src/{lib,shm,vkraw}.rs, 12 hex>[+dirty<N>]
so_sha256=<sha256 of the .so>
```

The `dirty` suffix is deliberate and load-bearing: a result must never be quoted
without saying whether the tree it came from was clean. The whole stage-1b
battery was re-run after the harness fix so that every figure below comes from
one revision with **0 dirty paths** (`git HEAD 2b6b534…`, build id
`2b6b534….718336e1a120`). The layer's *source* hash is the interesting half of
the id — if a later revision of the docs quotes the same `718336e1a120` with a
different HEAD, the code that ran is the same code.

## Stage 1: what the output proves (unchanged, re-run)

The layer's own log for a `vkcube --c 900` run: the loader calls our
`vkGetInstanceProcAddr` 64 times while building the instance dispatch, then our
`vkCreateDevice`, `vkGetDeviceQueue`, `vkCreateSwapchainKHR`, and finally our
`vkQueuePresentKHR` once per frame; `writer_final: intervals=899 dropped=0
presents=900` is printed immediately before the segment is unlinked.

The stage-1 acceptance table (C1-C5, 22 checks) still passes unchanged; check
`C2.intervals_are_presents_minus_one` asserts the interval/present relation from
the writer's own final line rather than deriving it.

## Experiment 1 — overhead

```sh
./bench.sh && ./overhead.sh 3      # raw: results/primitives.txt, results/overhead/
```

Arms, all with the manifest installed and the layer enabled by `LAPSPHERE_FRAMES=1`:

| Arm | Environment | What it does per present |
| --- | --- | --- |
| A | `LAPSPHERE_FRAMES` unset | nothing (the loader does not activate the layer) |
| B | `LAPSPHERE_FRAMES_PASS=1` | the hook runs, `record()` returns at its first check, the segment is created but never written |
| C | `LAPSPHERE_FRAMES=1` | the full record path |
| D | `+ LAPSPHERE_FRAMES_OVERHEAD=1` | C, plus one sample of the layer's own work per recorded interval |

### What the measured window contains (this is the part that is easy to get wrong)

The sample is `d = now_ns() - work_start`, where `work_start` is the same
`CLOCK_MONOTONIC` reading the hook took at entry, and the subtraction happens at
the end of `Shm::Writer::record()`. So it contains:

* the `catch_unwind` closure the hook body sits in, the `writer_slot()` mutex
  lock, everything `record()` does (origin check, seqlock, the volatile ring
  write, four atomic header stores) and **one `clock_gettime` call** — the one
  that produces `d` itself.

and it does not contain:

* `bump()`, the latency of the entry clock read (its *result* is the window's
  start), the unlock, the `frames % 600` log line (that is after `record()`
  returns), `next_present_for()` (two mutex lookups plus scans), or the call down
  into the next layer. The sample's own store to the overhead buffer also
  happens *after* `d` is computed.

That list is why `primitive_bench` exists: the excluded parts are priced
out-of-band instead of being assumed small.

### Out-of-band floors (`results/primitives.txt`, same session, `iters=1e6 io=2e4`)

```
clock_pair         n=1000000  median=      20 ns  p99=      30 ns  max=    30127 ns   (0.020 us)
mutex_lock         n=1000000  median=      30 ns  p99=      31 ns  max=    17403 ns   (0.030 us)
record_full        n=1000000  median=      40 ns  p99=      51 ns  max=    19276 ns   (0.040 us)
hook_recording     n=1000000  median=      50 ns  p99=      51 ns  max=     2936 ns   (0.050 us)
mmap_read          n=20000    median=    6722 ns  p99=   10079 ns  max=    36479 ns   (6.722 us)
readdir            n=20000    median=    2355 ns  p99=    2465 ns  max=    19106 ns   (2.355 us)
record_overhead    n=1000000  median=      61 ns  p99=      71 ns  max=     5651 ns   (0.061 us)
hook_passthrough   n=1000000  median=      50 ns  p99=      51 ns  max=    19637 ns   (0.050 us)
```

Read it as: every window above contains one clock pair (20 ns) on top of the body
it names, so `record_full`'s 40 ns window is a ~20 ns body, and the *whole*
present hook's shape without the call down (`hook_recording` = clock read + lock +
release) is a 50 ns window. `record_overhead - record_full` (61 vs 40) is what
being measured costs the writer. `hook_passthrough` is the "loaded but doing
nothing" cost. The 10 ns quantisation of the small values is the clock's
resolution here, not a rounding choice.

`mmap_read` is the same operation `frames_reader` performs per snapshot — open,
mmap a real 65664-byte segment, copy both rings (4096 + 4096 `u64`), munmap — and
it is check `D1`'s floor. The file size is asserted by the benchmark itself
(`bytes=65664 expected=65664 = header 128 + ring 32768 + overhead 32768`).

### M1 — in-band samples (arm D, 3 reps per configuration)

`LAPSPHERE_FRAMES_OVERHEAD=1`, one sample per recorded interval, read out of the
segment by `frames_reader --overhead` while the application is alive:

| Regime, MangoHud | samples / intervals | median | p99 | p99.9 | max |
| --- | --- | --- | --- | --- | --- |
| vsync 165 Hz, off (r1) | 1933 / 1933 | 0.141 µs | 0.841 µs | 19.107 µs | 20.449 µs |
| vsync 165 Hz, off (r2) | 1892 / 1892 | 0.120 µs | 1.182 µs | 13.035 µs | 15.389 µs |
| vsync 165 Hz, off (r3) | 1897 / 1897 | 0.140 µs | 1.132 µs | 16.191 µs | 21.181 µs |
| unthrottled, off (r1) | 4096 / 4096 | 0.050 µs | 0.190 µs | 0.421 µs | 0.501 µs |
| unthrottled, off (r2) | 4096 / 4096 | 0.070 µs | 0.150 µs | 0.341 µs | 0.832 µs |
| unthrottled, off (r3) | 4096 / 4096 | 0.101 µs | 0.161 µs | 0.331 µs | 0.731 µs |
| vsync 165 Hz, on (r1) | 1904 / 1904 | 0.151 µs | 1.042 µs | 12.022 µs | 13.586 µs |
| vsync 165 Hz, on (r2) | 1905 / 1906 | 0.140 µs | 1.002 µs | 12.734 µs | 13.996 µs |
| vsync 165 Hz, on (r3) | 1909 / 1909 | 0.120 µs | 0.671 µs | 13.526 µs | 14.257 µs |

(unthrottled = `--present_mode 0`, ~4.7-4.9 k presents/s; "samples / intervals"
are the reader's sample count and the interval count from the same snapshot, so a
one-frame difference is the gap between the two reader calls and nothing else.)

Three things to take from this table:

* The median sits 2.5-7x above the out-of-band `hook_recording` window (50 ns) —
  consistent with the window also containing `catch_unwind` and a real
  application's cache state, and inconsistent with the instrument being the whole
  story.
* The median is *smaller* unthrottled (0.05-0.10 µs) than under vsync
  (0.12-0.14 µs), and the tail is 20x shorter. The vsync tail is the process
  being descheduled inside the window (the present blocks in the driver, the
  layer's code runs cold afterwards); it is a property of the regime, not of the
  layer's instructions.
* MangoHud below this layer does not move the number.

### M3 — throughput A/B (arms A/B/C/D, unthrottled, 3 reps, rotated order)

One arm = two runs with different frame counts; the rate is the slope
`(N2-N1)/(T2-T1)`, so the one-time instance/device/swapchain setup cancels
exactly. Arms rotate every rep so a slow drift does not land on one arm. The
layer's own present counter is quoted live as a check that `--c N` produced N
presents.

```
arm A rates 4857 4818 4809 fps  (median 4817.7, spread 1.01%)
arm B rates 4841 4853 4843 fps  (median 4843.3, spread 0.25%)   presents_seen=150000
arm C rates 4838 4824 4835 fps  (median 4835.2, spread 0.29%)   presents_seen=150000
arm D rates 4826 4807 4725 fps  (median 4806.7, spread 2.09%)   presents_seen=150000

paired per-rep deltas vs arm A               median
B - A: +0.71  -1.51  -1.49 us per present    -1.488 us
C - A: +0.84  -0.26  -1.14 us per present    -0.257 us
D - A: +1.36  +0.48  +3.67 us per present    +1.358 us
```

**M3 cannot resolve the layer, and it is reported as such.** A single rep
scatters by ±3.7 µs on a ~208 µs present, and the *paired* medians move between
batteries: a first battery of the same three reps put them at B−A −0.118, C−A
+0.719, D−A +0.487 µs, this one at −1.488, −0.257, +1.358 — including negative
values, where pass-through would have to be *faster* than no layer at all. That
is the method's floor, not a measurement. Meanwhile the in-band measurement puts
the layer's work at 0.05-0.15 µs, i.e. a factor of ~10-30 below the paired
medians. What M3 therefore bounds is the *whole* hook — including everything M1
excludes, notably the two mutex lookups in `next_present_for` and the call down —
at below about 1-2 µs, and it is a check that nothing large is hidden. It is a
rate delta: **not** a frame time, and not the overhead. The `presents_seen=150000`
column is the independent confirmation that the frame counts are what they claim
to be (arm A has no layer, so no counter).

## Experiment 2 — agreement with MangoHud

```sh
./agreement.sh 2                    # raw: results/agreement/lim<off|120>-r<rep>/
```

Both instruments run at once on the same presents (MangoHud is *below* this layer
in the loader chain, so every present this layer timestamps is the present
MangoHud logs), in two configurations because MangoHud's limiter changes the
cadence it is measuring:

```
MANGOHUD=1 LAPSPHERE_FRAMES=1 MANGOHUD_CONFIG="output_folder=…,autostart_log=1,log_duration=15,log_interval=0,fps_limit=<0|120>" \
    vkcube --gpu_number 0 --wsi xlib --c 2850
```

MangoHud logs one row per frame with `frametime` (ms, 5 decimals) and `elapsed`
(nanoseconds since its log started). This layer's intervals are reconstructed onto
absolute `CLOCK_MONOTONIC` time: the last interval ends at the segment's
`last_present_ns`, which `frames_reader --list` reports as an age, so every
earlier present time is that minus the sum of the intervals after it. The
remaining unknown (MangoHud's log start) is resolved by minimising the median
per-frame residual over a 1 ms grid, then refining at 20 µs. Aligned pairs are
written to `pairs.csv` per run.

| | limiter off r1 | limiter off r2 | limiter 120 r1 | limiter 120 r2 |
| --- | --- | --- | --- | --- |
| MangoHud rows | 2440 | 2437 | 1799 | 1799 |
| our intervals in the segment | 2778 | 2772 | 2778 | 2780 |
| frames compared / outside our window | 2440 / 0 | 2437 / 0 | 1799 / 0 | 1715 / 84 |
| MangoHud p50 | 6.0657 ms (164.86 fps) | 6.0649 ms | 8.3331 ms (120.00 fps) | 8.3329 ms |
| layer p50 | 6.0664 ms (164.84 fps) | 6.0651 ms | 8.3337 ms (120.00 fps) | 8.3335 ms |
| MangoHud p99 / p99.9 | 9.5589 / 10.4607 ms | 9.7746 / 10.4445 ms | 8.3944 / 8.4608 ms | 8.3964 / 8.4308 ms |
| layer p99 / p99.9 | 9.5757 / 10.4614 ms | 9.7350 / 10.4453 ms | 8.3979 / 8.7951 ms | 8.3969 / 8.7241 ms |
| paired duration difference, median | −4.85 µs | −4.22 µs | +0.59 µs | +0.47 µs |
| p50 difference | −0.080 % of the frame | −0.070 % | +0.007 % | +0.006 % |
| per-frame timestamp residual | median 164 µs, p99 3285 µs | median 172 µs | median 102 µs, p99 301 µs | median 112 µs, p99 336 µs |

How to read it:

* **The frame-time agreement is real**: p50 within 1 µs (0.007 %) and p99 within
  0.2 % in all four runs, on both cadences, with the two instruments running
  simultaneously. The paired per-frame durations agree to ±5 µs median. That is
  not "both layers loaded without crashing" — it is the two numbers being the
  same number.
* **The residual is the log's own timestamping, not the frame times.** A median
  residual of ~110-160 µs against a 6.06 ms cadence means MangoHud's `elapsed`
  column is stamped ~0.1-0.2 ms away from the present instant it nominally
  describes (its overlay work happens between the two). If its `frametime` were
  derived from `elapsed` the paired differences would be ±150 µs too, and they
  are not: MangoHud's `frametime` comes from its own present timing, independent
  of its logging path. So the log's *timestamps* cannot be used for
  microsecond-scale per-frame work; its *durations* can.
* **The alignment is resolved only modulo the frame period.** With a periodic
  cadence (the 120-limit runs are exactly 8.3331 ms everywhere) a shift by a whole
  number of frames leaves every MangoHud timestamp landing on one of our frames
  again, so the grid search has aliases; one run resolved its log start 449 ms
  *before* the launch timestamp as a result. This is why the evidence above rests
  on the distributions and on the paired *durations* — both invariant under a
  frame-index shift — and not on the absolute offset.
* The `fps_limit` rows are the honest statement about MangoHud: at 120 it is
  MangoHud's limiter, not the panel, that sets the cadence (8.3331 ms), and both
  instruments measure that same throttled cadence.

Limits: no non-vsync cadence under a real game; both hooks are in the same
process, so this is agreement about the present-hook boundary, not about
compositor scanout; MangoHud's measurement point inside its own hook is not
identical to ours (it runs below us), which is part of why the residual is not
zero even in principle.

## Experiment 3 — robustness

```sh
./robustness.sh 2                   # raw: results/robustness/
```

| Section | Result |
| --- | --- |
| R1 clean exit | `vkcube --c 900` exit 0; `writer_final: intervals=899 dropped=0 presents=900`; 0 segments left |
| R2 `kill -9` | wait status **137**; the writer's segment survives (65664 bytes, `alive=true pid_alive=false`, no `writer_final` line because there was no clean shutdown); `--prune` removes it (`pid_alive=false, too_old=false`); the next run exits 0 with 600 presents and its own segment |
| R3 resize | Xvfb+lavapipe `vkcube --wsi xlib --width 640 --height 480 --c 2500`, resized to 1000x700, 500x400, 900x600 by window id: exit 0, **2500 presents, 4 swapchain creations**, `intervals = 2496 = 2500-1-3` **HOLDS**, 0 intervals above 3x p50, 0 dropped |
| R3b second swapchain, no resize | host Wayland WSI, `vkcube --c 800`: exit 0, 800 presents, 2 creations, `intervals = 798 = 800-1-1` **HOLDS** |
| R4 two instances | 2 segments, 2 pids (xlib/dGPU and wayland/iGPU); `--dump` reads the freshest; `SIGKILL` of the second leaves the first running to exit 0 with 567 intervals dumped afterwards; the killed writer's orphan stays until `--prune` |
| R5 no reader | exit 0, 900 presents, `intervals=899`, segment unlinked — no reader ever started |
| R6 reader cost/CPU | table below |
| R6b read-cost decomposition | no segment 37.6 µs; static segment, 599-slot ring 82.8 µs; static segment, full ring (4319 slots) 107.8 µs; live 60 Hz writer 80.6 µs (298 frames); live `vkcube` 165 Hz 37.0 µs (404 frames) |
| R7 writer creation failure | segment dir made unwritable: `vkcube` exit **0**, no segment created; the error text (`frames_synth: Writer::create failed: open …/lapsphere/frames-N: Permission denied (os error 13)`) taken from `frames_synth`, which prints it, because the layer's own log lives in that same directory |
| R8 `SIGSTOP`/`SIGCONT` | frames frozen at 325 while stopped and the reported age grows to 2.008 s; after `SIGCONT` the count continues (686) and the run exits 0 with `intervals=1999` |

### R6 — reader cost and CPU at 60 and 240 Hz

`frames_reader --watch --hz <60|240> --seconds 8 --cpu` against four writers. The
"read cost" is one full snapshot: directory scan, one mmap + copy per segment,
selection. "240 Hz" here is the *reader's poll rate* (and, for `synth240`, also
the writer's cadence — it is a synthetic writer, not a present load; the panel is
165 Hz).

| Writer (cadence, frames seen) | reader 60 Hz | reader 240 Hz |
| --- | --- | --- |
| `vkcube` 165 Hz (1165-2129) | 39.8 / 42.9 µs, cpu 0.125-0.25 % | 39.5 / 32.0 µs, cpu 0.875-1.0 % |
| `frames_synth` 60 Hz (598-1079) | 93.9 / 87.4 µs, cpu 0.625-0.75 % | 84.2 / 90.8 µs, cpu 2.5-3.0 % |
| `frames_synth` 240 Hz (2395-4321) | 86.2 / 92.3 µs, cpu 0.5-0.75 % | 96.9 / 97.7 µs, cpu 2.75-2.875 % |
| `vkcube` unthrottled ~4.8 k/s (884-1846) | 37.8 µs, cpu 0.25 % | 29.1 µs, cpu 0.875 % |

Seqlock retries: 0 in every run of this battery; the earlier battery of the same
configuration saw 10 retries in one 240 Hz run of 1920 samples against a full
ring, which is the mechanism working rather than a failure. Actual poll rates were
60.0 and 240.0 Hz in every run.

`R6b` is the reason this number is not quoted as a copy cost: with no segment at
all it is already 37.6 µs, while the hot-loop floors for the same operations are
6.722 µs (mmap+copy) and 2.355 µs (`read_dir`). The 60/240 Hz reader pays a cold
path (it wakes from a sleep and is the first thing to touch those pages), and the
ring's live size adds on top of that. **This is the number a panel would pay per
poll on this host (0.125-3 % of a core); it is not the cost of the copy.** The
layer's own hot path is unaffected by any of it (arm D's samples above).

### Freshness

`last_present_ns` age sampled 15x with `--list` while a writer ran: 0.000-0.016 s
at a 60 Hz writer, 0.000-0.004 s at a 240 Hz writer — i.e. bounded by the writer's
period, as expected. That is also the only sense in which "reader latency" is
bounded by the layer: the segment is written before the present returns, so a
reader never sees data older than the writer's last present.

## The acceptance harness

`./verify.sh 0` prints one PASS/FAIL line per check with the value it read.
Stage-1 checks C1-C5 as recorded in the stage-1 revision of this file, plus:

| Check | Result |
| --- | --- |
| `D1.read_cost_includes_the_read` | PASS — reader median **103.0 µs** vs the out-of-band hot mmap+copy floor **6.722 µs** (the pre-fix code reported ~0.0 µs and cannot pass this on any host) |
| `D2.watch_sees_late_segment` | PASS — reader started 2 s before the app, `max_frame_count=407` (the pre-fix code reported 0) |
| `D3.one_overhead_sample_per_interval` | PASS — `samples=1179 intervals=1179` from one instrumented run |
| `D3.samples_above_the_clock_floor` | PASS — median sample 0.120 µs vs the 0.020 µs clock pair it contains |

Raw material, regenerated by every run and **not** committed: `results/`
(app output, layer logs, reader output, dumped intervals, benchmark output, the
three experiment reports).

## Two conventions a later stage must not silently change

* **The timestamp is taken at entry to `vkQueuePresentKHR`**, so an interval is
  present-to-present as the application issues it.
* **`frame_count` counts intervals, i.e. presents − 1 per cadence**, and each
  swapchain recreation starts a new cadence. Both stages assert the relation
  rather than deriving it: `intervals == presents - 1` for one swapchain
  (`C2`), and `intervals == presents - 1 - (creations - 1)` measured in R3
  (2500/4 → 2496) and R3b (800/2 → 798).

## Not tested

No DXVK/Proton/Wine, no OpenGL, no 32-bit, no anti-cheat, no real game, no GPU-
bound title; no 240 Hz *present* load (the panel is 165 Hz; 240 Hz below means a
reader poll rate or a synthetic writer); no interactive resize on the host
session (it exposes no window control — measured — so the resize ran under
Xvfb+lavapipe); no pid-namespace `O_EXCL`/pid-reuse case (unprivileged pid
namespaces unavailable here); no two-devices-in-one-process; no overhead
measurement on the AMD iGPU or under Wayland WSI; nothing outside
`spike/vk-layer/` was touched, and no LapSphere service was started, stopped or
configured.
