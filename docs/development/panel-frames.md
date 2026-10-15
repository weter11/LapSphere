# Panel frame metrics — the six elements, the reader, and what is claimed

What stage 6 implemented, what the numbers mean, how they are produced, and what is
**not** claimed about them. Written from the code and the layer's own documents;
nothing here was re-measured.

**Scope of the claims on this page.** Everything under "tested" below was observed
by `cargo test` on this commit. Everything under "not tested" was not observed at
all — no live game was driven with this reader in the loop. Stage 7, the first
version's final acceptance, is not claimed and not started.

## 1. The six elements

The owner's decision (recorded in [`panel-v1-baseline.md`](./panel-v1-baseline.md)
§4) makes these first-release panel elements. Their registry rows are in
`gui/src/panel/rows.rs` under `Group::Frame`:

| id | shown as | value | where it comes from |
|---|---|---|---|
| `game_name` | `Game` | the game's `/proc/self/comm` | header `exe_name` |
| `fps` | `FPS` | `1000 / mean(intervals)` | ring, over the statistics window |
| `frametime_ms` | `Frame` | the mean interval, in ms | ring, same window |
| `frametime_graph` | `Graph` | the recorded intervals, drawn | ring, newest 120 |
| `low_1pct` | `1%low` | `1000 / mean(worst 1 %)` | ring, same window |
| `low_01pct` | `0.1%` | `1000 / mean(worst 0.1 %)` | ring, same window |

Notes that matter:

* **`frametime_ms` is the mean, not the median.** The design's item table sketched
  `median(interval)`; the definitions this change implements are the ones recorded
  with the task and the reference tool — `1000 / mean` for the average and the same
  mean interval for the frame time — so the two numbers are consistent with each
  other and with the tool that measures the layer.
* **Both lows are the reference's definition B**, the average of the worst N, not
  `1000 / p99` (its definition A). The panel has one slot per number; two readings
  in one slot is how a readout stops being comparable to anything.
* **The elements are off by default.** The layer that produces their data is
  installed by the user and opt-in per game, so a fresh panel would otherwise open
  with five permanent `—` rows and an empty strip. They are one click away under
  "Frames" in the panel settings, and an older `panel.json` keeps exactly the rows
  it already had.

## 2. Where the data comes from

There is no daemon method, no D-Bus object and no protocol change here, and there
was no alternative: **an external process cannot see a game's present.** The Vulkan
implicit layer timestamps `vkQueuePresentKHR` inside the game's process and
publishes the intervals through shared memory; the GUI reads that file itself.

```
$XDG_RUNTIME_DIR/lapsphere/frames-<pid>      protocol v1, mode 0600
```

`gui/src/panel/frames/` implements the read side:

* **`shm.rs`** — the reader. Before a single interval is read, in this order: the
  file exists and is at least 128 bytes; `magic` is `LSPFRS01`; `version` is 1;
  `header_len` is 128; `api` is 1 (Vulkan — `2` is the reserved GL slot and this
  reader cannot label it); `ring_capacity` is non-zero and at most 4096; the file is
  long enough for the ring the header declares; and the pid in the file name agrees
  with the pid in the header. Each refusal carries the value that caused it, so a
  log line names the number rather than a category.
* **The seqlock is respected, not approximated.** A snapshot reads the header,
  copies the intervals, and reads the header again: if the sequence moved, the whole
  pair is **discarded**. Nothing is repaired, clipped or partially used. After eight
  attempts the read is refused, which the caller reports as "no new data this poll"
  — never as zeros.
* **Reads are bounded and read-only.** At most the ring's 4096 slots (32 KiB), as at
  most two `seek`/`read_exact` runs, on a file opened read-only. The reader never
  writes, never truncates, never renames and **never unlinks**: ADR-5 allows a
  reader to prune orphan segments, and this one deliberately does not, because an
  orphan is already invisible after `STALE_AFTER` and costs one `read_dir` entry per
  poll, which is cheaper than the only failure a wrong deletion has — deleting a
  live game's segment.
* **`source.rs`** — the directory, the clock, and ADR-5's selection rule:
  **the segment with the most recent `last_present_ns`, younger than
  `STALE_AFTER = 2 s`, ties broken by the larger pid.** The timestamp is
  `CLOCK_MONOTONIC`, the same clock the writer stamps with. The writer's
  `WRITER_ALIVE` flag is deliberately not trusted: it survives a `SIGKILL`
  (`spike/vk-layer/TESTING.md`, R2), so a dead game's last number would stay on
  screen for as long as the panel runs. One game exiting must not leave frozen
  metrics, and this rule is what prevents it.
* **`stats.rs`** — the statistics, ported from the reference definitions in
  [`tools/frametime_stats.py`](./tools/frametime_stats.py) rather than re-derived.
* **`graph.rs`** — the strip: a polyline over a fixed height and a **fixed** 0–50 ms
  scale. Intervals above the scale are clamped to the ceiling (visible, but not
  pretending to a value); an empty source draws the strip's background and nothing
  else — no zero line, because a flat line at the bottom would be a claim about
  frame times.

## 3. Unavailable is `—`, and nothing jumps

* An absent, inaccessible, invalid or stale source produces **unavailable** values:
  `—` for the five text elements, an empty strip for the graph. There is no
  fabricated zero and no stale number carried over from the previous poll. A
  segment that exists but holds no intervals yet (the layer creates it at instance
  creation and writes its first interval one present later) shows the game's name
  with `—` beside it, which is the honest reading of an empty ring.
* **Geometry cannot jump.** The window height is
  `padding + Σ slot heights`, where a text row is one row height and the strip is
  `GRAPH_STRIP_ROWS` (3) row heights — a constant multiple of the font's row height,
  never a function of the sample count or of whether data arrived. The label column
  is the fixed `LABEL_WIDTH`, and `render`'s `every_label_fits_the_label_column`
  measures every registered label against it with real font metrics. The value
  column takes what is left and elides. So arriving data, changing values, an empty
  source and a game exiting all leave every rect where it was, and the repaint tests
  assert exactly that (`the_same_geometry_with_and_without_frame_data`).
* **The reader is not on the rendering path.** One worker thread polls outside the
  UI thread at the configured cadence; the UI clones one immutable snapshot. No file
  is opened, read or stat'd while rendering, and the panel repaints at the poll
  cadence (250 ms at 4 Hz), **never at the game's frame rate**. With no frame
  element on screen the panel keeps its own 500 ms timer, and the worker does no
  filesystem work at all.
* **No log spam.** Availability changes are announced once each: "no source" once,
  the game's pid once when it appears, once more when it exits.

## 4. The two configurable numbers are proposals

`window_seconds` (default **10**) and `refresh_hz` (default **4**) are the design's
Q9 and Q10 **proposals**, not owner decisions — no recorded decision settles either
number. They are therefore configurable in `panel.json` and on the settings surface,
and the surface says in words that they are proposals
(`the_frames_section_names_the_source_and_calls_the_defaults_proposals`). The PR
body records the same assumption. The statistics window is a **time** window: the
newest intervals whose total duration reaches `window_seconds`.

## 5. Activation: what has to be true on the host

Verbatim from the layer's own manifest and installer
(`spike/vk-layer/manifests/VK_LAYER_LAPSPHERE_frames.json`, `spike/vk-layer/install.sh`):

| | |
|---|---|
| Layer name | `VK_LAYER_LAPSPHERE_frames`, type `GLOBAL` |
| Activation | `enable_environment: {"LAPSPHERE_FRAMES": "1"}` — **per process, opt-in** |
| Deactivation | `disable_environment: {"LAPSPHERE_FRAMES_DISABLE": "1"}` |
| Installation | the manifest must be in the loader's **implicit** search path — `~/.local/share/vulkan/implicit_layer.d/` (what `install.sh` writes, no root) or the system-wide `/usr/share/vulkan/implicit_layer.d/` |
| Why not `VK_LAYER_PATH` | measured in spike round 1: the loader lists a manifest found there under *explicit* layers and never activates it, because `enable_environment` is only honoured for implicit layers |
| Where the data appears | `$XDG_RUNTIME_DIR/lapsphere/frames-<pid>`, created by the layer with mode 0600 |

**This change installs nothing and configures nothing globally.** There is no
system-wide file, no daemon change and no change to the game's environment; the
panel renders `—` until a user installs the layer and runs a Vulkan game with
`LAPSPHERE_FRAMES=1`.

## 6. Tested on this commit

`cargo test`: **423 tests pass, 0 fail** (341 before this change). The frame
features' own tests, all in `gui/src/panel/frames/`:

* protocol: missing file, short file (both shorter than the header and shorter than
  the ring it declares), bad magic, unsupported version, wrong `header_len`,
  non-Vulkan `api`, zero and oversized `ring_capacity`, and a pid that disagrees
  between the file name and the header — each refused with the value that caused it;
* reads: the newest intervals oldest-first before and after the ring wraps, more
  slots requested than the ring holds, an empty ring, an odd sequence, and a
  **real writer thread** whose every accepted snapshot must be internally consistent
  (a torn pair cannot satisfy the check, and the test asserts the invariant on every
  one of up to 40 accepted snapshots);
* selection: multiple segments, the most recent present winning even when its pid is
  the *smaller* one, the pid tie-break, exactly-`STALE_AFTER`-old counting as stale,
  no fallback to the nearest stale segment, a stamp from the future, and
  determinism across repeated scans;
* statistics: constant intervals, the 1–100 ms ramp, isolated hitches, a mixed tail,
  a thin tail, sample counts of 1/2/3, unwritten zero slots, and a comparison
  against the reference script itself
  (`the_statistics_match_the_reference_script_when_python3_is_available`, which
  skips loudly without `python3`);
* presentation: the graph's sample selection and fixed scale, the reserved strip
  height, identical geometry with and without data, every label fitting the reserved
  label column, `—` for all five text elements with no source and real numbers with
  one, and the repaint rule (250 ms with a frame row, 500 ms without);
* configuration and menu: the two new keys' defaults and clamps, an older
  `panel.json` gaining the six rows switched off, the frame rows appearing in the
  settings group, and the two sliders being wired to the config.

## 7. NOT TESTED — read this before quoting anything above

* **No live run.** This reader has never been pointed at a real Vulkan game on this
  host in this commit: every claim above is unit-test evidence against synthetic
  segments, or the spike's own measurements of the *layer*. The panel showing real
  fps over a running game is stage 7's acceptance, and it has not happened.
* **No measurement of the panel's cost with the frame elements on.** The panel's
  own idle-CPU and latency numbers on
  [`panel-measurements.md`](./panel-measurements.md) were taken without these rows;
  the 4 Hz cadence is a proposal whose cost under a compositor is unmeasured.
* **240 Hz as a *present* rate** is not tested anywhere: the panel is 165 Hz, and the
  spike's 240 Hz figure is a reader poll rate or a synthetic writer.
* **Vulkan only.** No OpenGL hook, no 32-bit games, no Proton/DXVK/Wine, no
  anti-cheat coverage, no GPU-bound title. A game outside those is not "unsupported
  by the panel" — nothing claims it is supported by the layer, and the elements
  render `—`.
* **No native-Wayland presentation claim.** The layer contains no WSI code; the
  spike's Wayland runs went through `vkcube`'s Wayland WSI on this host's xfwl4 +
  Xwayland session, and Wayland presentation *timing* was not studied.
* **One device, one queue, one swapchain per process** is the tested shape; two
  devices in one process and two games at once are arguments, not measurements. The
  selection rule handles the second case; nothing measures the first.
* **The frame-generation question (Q11) is unanswered.** Whether a present that
  interpolates frames (FSR 2 / DLSS / XeSS / AFMF) should count as a frame is an open
  design question; the panel shows what was presented.
* **The layer is not shipped.** Whether the product installs an implicit layer at all
  remains the owner's open question (Q13 in the design). This page describes what
  happens when a user installs it, and nothing more.
