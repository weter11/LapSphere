# Panel measurement tools

One file, and it is here as a **reference**, not as a program this tree runs.

## `frametime_stats.py`

Frame-time statistics and A/B comparison, Python 3 standard library only.

```bash
# one file
python3 docs/development/tools/frametime_stats.py run.csv

# comparison against the first file
python3 docs/development/tools/frametime_stats.py --compare base-a.csv panel.csv

# verify the implementation on synthetic data
python3 docs/development/tools/frametime_stats.py --self-test
```

It reads MangoHud CSV logs (skipping their preamble) or a one-column CSV of frame
times in milliseconds, and reports average fps, mean / median / p95 / p99 / p99.9 /
max frame times, **1 % low two ways** (A: `1000 / p99`; B: `1000 / mean(worst 1 %)`)
and the same for 0.1 %, plus the count of frames over 2× the median and over
33.3 ms. Percentiles are nearest-rank, not interpolated.

### Why it is in this tree

`1 % low` is used inconsistently across the ecosystem, so the panel does not define
its own. **This script is the definition.** The panel's Rust reader ports its
formulas (`low_percent()` for both lows, `1000 / mean` for average fps and for
`frametime_ms`) and `gui/src/panel/frames/stats.rs` holds a test that runs *this
file* and compares the Rust numbers with its output on the same interval series
(`the_statistics_match_the_reference_script_when_python3_is_available`; it skips,
loudly, on a machine without `python3`). Changing a formula here changes what the
panel is supposed to show, and that test is what notices.

### Provenance

Brought over unchanged from branch `feat/mini-panel` (PR #111), where the spike fed
the layer's raw intervals through it. **Not edited**, so the comparison is against
the real reference:

```
$ git show origin/feat/mini-panel:docs/development/tools/frametime_stats.py | sha256sum
4058a81f96130a06d3196b9ce1bf1d06b7f90da58665ad28e8bb21b856423fd3
$ sha256sum docs/development/tools/frametime_stats.py
4058a81f96130a06d3196b9ce1bf1d06b7f90da58665ad28e8bb21b856423fd3
```

That branch's own `tools/README.md` is **not** copied: it documents a measurement
recipe built around `gui/examples/frametime_probe.rs`, which does not exist in this
tree. What is needed here is the definitions, not the recipe.
