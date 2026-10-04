# Panel measurement tools

Two things live here, both for the mini-panel work:

* `frametime_stats.py` — frame-time statistics and A/B comparison, stdlib
  Python 3 only, with a self-test.
* how to drive it — the recipe at the bottom, for measuring a real game with
  and without the panel.

The synthetic source of frame times for A/B on the agent's machine is
`gui/examples/frametime_probe.rs` (see "Synthetic runs" below).

---

## `frametime_stats.py`

```bash
# one file
python3 docs/development/tools/frametime_stats.py run.csv

# comparison against the first file
python3 docs/development/tools/frametime_stats.py --compare \
    base-a.csv base-b.csv panel-1hz.csv panel-2hz.csv

# verify the implementation on synthetic data
python3 docs/development/tools/frametime_stats.py --self-test
```

### Input formats

**MangoHud CSV** — the log has preamble lines before the header, and the
header names the columns. The tool finds the first line containing a
`frametime` column and uses it. Example of the shape it expects:

```
# MangoHud version 0.7.1
GPU, CPU, frametime, FPS
100, 50, 16.0, 62.5
100, 50, 16.5, 60.6
```

**Plain CSV / text** — one frame time in milliseconds per line, or a CSV whose
first column is numeric. This is what `frametime_probe.rs` writes.

Non-positive values are dropped: they are never real frame times and would
corrupt every statistic.

### Output

Per file: frame count and total duration, average fps, median / mean /
stddev frametime, p95, p99, p99.9, max, **1% low two ways**, **0.1% low two
ways**, count of frames over 2x the median, and count over 33.3 ms.

"1% low" is used inconsistently in the wild, so both readings are reported:

* **A** — `1000 / p99 frametime` (fps implied by the p99 frame)
* **B** — `1000 / mean(frametimes of the worst 1% of frames)`

They coincide when the slowest frames are at least 1% of the total, because
then the worst 1% is entirely slow frames. On a thin tail they differ, and B
is the more common definition.

Percentiles are **nearest-rank** (the k-th smallest), not interpolated: for
frame times the conservative reading is the useful one.

### Comparison mode

`--compare a.csv b.csv [c.csv ...]` reports each file, then a table of
deltas against the first, marking each as `(better)` or `(worse)` — for
frame times an increase is worse, for fps a decrease is.

If several files share a prefix and differ only by a trailing `-a` / `-b`
(the same baseline run repeated), the tool reports the spread within each
group as a **noise floor**, and the note it prints is: *a delta smaller than
that spread is "within noise"*. This matters: repeat your baseline at least
twice before believing a small difference.

### Self-test

`--self-test` checks the statistics against synthetic data with known
answers, including: a 1..100 ms ramp (p99 must be 99), a constant 16.67 ms
stream (60 fps everywhere), a single hitch (must move p99.9 and max but not
p95), a 2% slow tail (p99 must see a slow frame), a 0.5% tail (p99 must
**not** move — the tool must not invent a problem), MangoHud preamble
parsing, plain CSV parsing, and dropping non-positive values.

It is a real check, not a smoke test: it caught two wrong expectations and
one wrong "fix" during development. **Run it after any change to the
statistics.**

---

## Measuring a real game (the owner's machine)

The point of this section: a panel that costs nothing on the agent's
synthetic probe could still cost frames in a real game, and only the real
game settles it. This is why the work below is written for the owner.

### Prerequisites

* The game, running **borderless fullscreen** (that is the target scenario).
* MangoHud for logging. On the agent's host it **is** installed
  (`/usr/bin/mangohud`, **v0.8.1**), and the option names below were read
  from this host's own files rather than assumed:

  ```bash
  command -v mangohud && mangohud --version     # -> v0.8.1 here
  man mangohud                                  # short page
  zcat /usr/share/doc/mangohud/MangoHud.conf.example.gz | grep -A3 output
  ```

  The relevant options in the shipped example config are `output_folder`,
  `output_file` and `log_interval` (`log_interval=0` logs every frame). On
  the owner's machine, re-read the same files: option names have changed
  between MangoHud versions, and a wrong flag fails silently.

* The panel, built from the mini-panel branch.

### Three runs, same scene, same settings

Run the **same scene** three times. A menu, a benchmark or a fixed camera
path is better than gameplay, because it keeps the GPU load comparable.
**2–3 minutes each.** Let each run warm up for ~20 s before you start
counting, and do not change anything else between runs.

| Run | Panel | Notes |
| --- | --- | --- |
| A | **off** | baseline; repeat it as A-b to get a noise floor |
| B | panel at **1 Hz** | |
| C | panel at **2 Hz** | |

Suggested order: A, B, C, then **A again**. The second A is your noise
floor: if A and A-b differ by more than the B-vs-A difference you were
about to call real, the difference is not real.

### With MangoHud

Log to CSV. The shape to aim for is a file whose header names a `frametime`
column; `frametime_stats.py` skips MangoHud's preamble and finds it. With the
options this host's config documents, that is roughly:

```bash
# check the real option names for your version first (see above)
mkdir -p ~/mangologs
MANGOHUD=output_folder=~/mangologs,output_file=game,log_interval=0 \
         mangohud <your-game> --fullscreen
```

Then:

```bash
python3 docs/development/tools/frametime_stats.py --compare \
    game-a.csv game-a-b.csv game-panel1hz.csv game-panel2hz.csv
```

### Without MangoHud

Any external frame-time source works as long as it emits one frame time per
line. On this host the synthetic generator is `frametime_probe.rs`:

```bash
cd <lapsphererepo>
cargo run --release --example frametime_probe -- --seconds 180 --out base.csv
# then start the panel and repeat for each panel rate
cargo run --release --example frametime_probe -- --seconds 180 --out panel1hz.csv
```

For a *real* game without MangoHud, use whatever the game itself reports
(Steam's own overlay graph, `nvtop`, `presentmon` on Windows) and export the
frame times to a one-column CSV. The statistics are identical; only the
source differs.

### Reading the result

The numbers that matter for "does the panel cost frames":

* **average fps** — a small drop is usually noise; check it against the
  noise floor from your repeated baseline.
* **p99 and p99.9 frametime** — where an overlay's cost shows up first: the
  panel steals a slice of the compositor occasionally, which rarely moves the
  average but does add tail latency.
* **1% low (B)** — the number a player actually feels as stutter.

Report the numbers with the noise floor next to them. "Panel costs 0.4% of
average fps, which is inside the baseline spread" is a useful result;
"panel costs 0.4%" on its own is not.

### One caution about this host's numbers

Everything measured on the agent's machine is **xfwm4 4.20.0 with the
compositor OFF** (`xprop -root _NET_WM_COMPOSITING` → no such atom). Without
a compositor, a fullscreen window is unredirected and drawn straight to the
framebuffer, so an overlay cannot cost it much — the easy case. **With a
compositor, every frame goes through the compositor, and an overlay's cost is
much more likely to show.** B21 could not obtain a compositing session here
(see `panel-overlay-tests.md`), so the composited case is **NOT TESTED** and
is the single most important thing for the owner to check.

Commands to toggle it on the owner's machine, for xfwm4:

```bash
# check the current setting
xfconf-query -c xfwm4 -p /general/use_compositing

# turn it on (takes effect at the next xfwm4 start)
xfconf-query -c xfwm4 -p /general/use_compositing -s true

# confirm it is actually live (must print 1, not "no such atom")
xprop -root _NET_WM_COMPOSITING
```

Note: on the agent's host that setting already reads `true` while the
running WM has compositing **off** — the running WM was started before the
change, and a restart is what applies it. Always confirm with `xprop`, not
with `xfconf-query`.
