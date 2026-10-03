#!/usr/bin/env python3
"""frametime_stats.py -- frame-time statistics for A/B comparisons.

Reads either a MangoHud CSV log (which has preamble lines before the header)
or a simple one-column CSV of frame times in milliseconds, and reports the
statistics that matter when judging whether an overlay costs a game frames.

Standard library only. Python 3.8+.

Usage
-----
  # single file
  frametime_stats.py run.csv

  # comparison
  frametime_stats.py --compare baseline.csv panel1hz.csv panel2hz.csv

  # verify the implementation against synthetic data
  frametime_stats.py --self-test

Definitions, stated because "1% low" is used inconsistently in the wild:
  * 1% low, method A:  1000 / p99_frametime      ("fps implied by the p99 frame")
  * 1% low, method B:  1000 / mean(frametimes of the worst 1% of frames)
                       ("average fps over the worst 1% of frames")
  The second is the more common definition; both are reported because they
  can differ substantially when a few very long frames are present. Note they
  coincide when the slowest frames make up at least 1% of the total, because
  the worst 1% is then entirely slow frames.
  0.1% low is the same with p99.9 / worst 0.1%.
"""
import csv
import math
import statistics
import sys

# --------------------------------------------------------------------------
# loading


def load_frametimes(path):
    """Return a list of frame times in ms.

    Accepts:
      * a MangoHud CSV: preamble lines, then a header row containing
        'frametime', then data rows. The frametime column is used.
      * a plain CSV with a single numeric column, or a single column of numbers.
      * a plain text file with one number per line.
    """
    with open(path, "r", errors="replace") as fh:
        lines = fh.read().splitlines()

    # MangoHud: find the header line that names a frametime column
    header_idx = None
    for i, line in enumerate(lines[:80]):
        low = line.lower()
        if "," in line and "frametime" in low:
            header_idx = i
            break

    values = []
    if header_idx is not None:
        cols = [c.strip().strip('"').lower() for c in lines[header_idx].split(",")]
        try:
            col = cols.index("frametime")
        except ValueError:
            col = 0
        rdr = csv.reader(lines[header_idx + 1 :])
        for row in rdr:
            if not row or col >= len(row):
                continue
            cell = row[col].strip().strip('"')
            if not cell:
                continue
            try:
                values.append(float(cell))
            except ValueError:
                continue
    else:
        # plain: pick the first parseable number on each line
        for line in lines:
            s = line.strip()
            if not s or s.startswith("#"):
                continue
            cand = s.split(",")[0].strip().strip('"')
            try:
                values.append(float(cand))
            except ValueError:
                continue

    # Sanity: drop non-positive values, which are never real frame times
    return [v for v in values if v > 0]


# --------------------------------------------------------------------------
# statistics


def percentile(sorted_vals, p):
    """Nearest-rank percentile on an already-sorted list.

    Nearest-rank is used rather than interpolation because for frame times the
    conservative reading ("p99.9 is at least this bad") is the useful one.
    """
    if not sorted_vals:
        return float("nan")
    n = len(sorted_vals)
    k = max(1, math.ceil(p / 100.0 * n))
    # k-th smallest == the p-th percentile. Indexing from the fast end is
    # correct here; the "slow end" reading (sorted_vals[n-k]) was tried and
    # rejected because it breaks the plain case: on a 1..100 ms ramp it
    # returns p99 = 2 ms instead of 99 ms.
    return sorted_vals[min(k, n) - 1]


def low_percent(sorted_vals, pct):
    """Average of the worst `pct` percent of frame times, returned as fps."""
    if not sorted_vals:
        return float("nan")
    n = len(sorted_vals)
    k = max(1, math.ceil(pct / 100.0 * n))
    worst = sorted_vals[-k:]
    mean = sum(worst) / len(worst)
    return 1000.0 / mean if mean > 0 else float("inf")


def stats(path):
    v = load_frametimes(path)
    if not v:
        return None
    s = sorted(v)
    n = len(s)
    med = percentile(s, 50)
    p95 = percentile(s, 95)
    p99 = percentile(s, 99)
    p999 = percentile(s, 99.9)
    mean = sum(s) / n
    over_2x = sum(1 for x in s if x > 2 * med)
    over_333 = sum(1 for x in s if x > 33.3)
    return {
        "path": path,
        "frames": n,
        "duration_s": sum(s) / 1000.0,
        "avg_fps": 1000.0 / mean if mean > 0 else float("inf"),
        "median_ms": med,
        "mean_ms": mean,
        "p95_ms": p95,
        "p99_ms": p99,
        "p999_ms": p999,
        "max_ms": s[-1],
        "stddev_ms": statistics.pstdev(s) if n > 1 else 0.0,
        "low1_a_fps": 1000.0 / p99 if p99 > 0 else float("inf"),
        "low1_b_fps": low_percent(s, 1.0),
        "low01_a_fps": 1000.0 / p999 if p999 > 0 else float("inf"),
        "low01_b_fps": low_percent(s, 0.1),
        "over_2x_median": over_2x,
        "over_33ms": over_333,
    }


def fmt(st):
    if st is None:
        return f"  {  '(no usable frames)'}"
    return "\n".join(
        [
            f"  file              : {st['path']}",
            f"  frames            : {st['frames']}  ({st['duration_s']:.1f} s of footage)",
            f"  average fps       : {st['avg_fps']:.1f}",
            f"  frametime median  : {st['median_ms']:.3f} ms",
            f"  frametime mean    : {st['mean_ms']:.3f} ms",
            f"  frametime stddev  : {st['stddev_ms']:.3f} ms",
            f"  p95               : {st['p95_ms']:.3f} ms",
            f"  p99               : {st['p99_ms']:.3f} ms",
            f"  p99.9             : {st['p999_ms']:.3f} ms",
            f"  max               : {st['max_ms']:.3f} ms",
            f"  1% low  (A, p99)  : {st['low1_a_fps']:.1f} fps",
            f"  1% low  (B, worst): {st['low1_b_fps']:.1f} fps",
            f"  0.1% low (A, p99.9): {st['low01_a_fps']:.1f} fps",
            f"  0.1% low (B, worst): {st['low01_b_fps']:.1f} fps",
            f"  frames > 2x median: {st['over_2x_median']}  ({100.0*st['over_2x_median']/st['frames']:.2f} %)",
            f"  frames > 33.3 ms  : {st['over_33ms']}  ({100.0*st['over_33ms']/st['frames']:.2f} %)",
        ]
    )


# --------------------------------------------------------------------------
# comparison


def compare(paths):
    sts = [stats(p) for p in paths]
    sts = [s for s in sts if s]
    if not sts:
        print("no usable input")
        return
    for st in sts:
        print(f"--- {st['path']}")
        print(fmt(st))
        print()

    if len(sts) == 1:
        print("only one file given; nothing to compare")
        return

    base = sts[0]
    print("=" * 72)
    print(f"COMPARISON against {base['path']}")
    print("=" * 72)
    keys = [
        ("avg_fps", "average fps", 1),
        ("low1_b_fps", "1% low (worst 1%)", 1),
        ("low01_b_fps", "0.1% low (worst 0.1%)", 1),
        ("median_ms", "median frametime", -1),
        ("p99_ms", "p99 frametime", 1),
        ("p999_ms", "p99.9 frametime", 1),
        ("max_ms", "max frametime", 1),
        ("over_33ms", "frames > 33.3 ms", 1),
    ]
    for st in sts[1:]:
        print(f"\n  {st['path']}")
        for k, label, direction in keys:
            b, v = base[k], st[k]
            if b == 0:
                continue
            rel = 100.0 * (v - b) / abs(b)
            # For frame times an increase is bad; for fps a decrease is bad.
            worse = (rel > 0) if direction < 0 else (rel < 0)
            print(
                f"    {label:<24} {b:>10.3f} -> {v:>10.3f}   "
                f"{rel:+7.2f} %  {'(worse)' if worse else '(better)'}"
            )

    # Noise estimate: if the baseline list has repeat runs (same prefix,
    # e.g. base-a/base-b), report the spread so a single small delta can be
    # called out as inside the noise.
    groups = {}
    for st in sts:
        key = st["path"].rsplit("-", 1)[0] if st["path"][-2] in "ab" else st["path"]
        groups.setdefault(key, []).append(st)
    reps = {k: v for k, v in groups.items() if len(v) > 1}
    if reps:
        print()
        print("  repeat-run spread (use this as the noise floor):")
        for k, v in reps.items():
            for metric in ("avg_fps", "p99_ms", "low1_b_fps"):
                vals = [x[metric] for x in v]
                if metric == "avg_fps":
                    print(
                        f"    {k:<28} {metric:<10} "
                        f"min={min(vals):.3f} max={max(vals):.3f} "
                        f"spread={max(vals)-min(vals):.3f}"
                    )
                else:
                    print(
                        f"    {k:<28} {metric:<10} "
                        f"min={min(vals):.3f} max={max(vals):.3f} "
                        f"spread={max(vals)-min(vals):.3f}"
                    )
        print()
        print("  A delta smaller than the spread above is 'within noise'.")


# --------------------------------------------------------------------------
# self test


def _self_test():
    import random

    ok = True

    def check(label, got, want, tol=1e-6):
        nonlocal ok
        if abs(got - want) > tol:
            print(f"  FAIL {label}: got {got}, want {want}")
            ok = False
        else:
            print(f"  ok   {label}: {got}")

    # percentile on a known ramp: 1..100 ms
    ramp = [float(i) for i in range(1, 101)]
    check("p50 of 1..100", percentile(ramp, 50), 50.0)
    check("p95 of 1..100", percentile(ramp, 95), 95.0)
    check("p99 of 1..100", percentile(ramp, 99), 99.0)
    check("p99.9 of 1..100", percentile(ramp, 99.9), 100.0)
    check("max of 1..100", percentile(ramp, 100), 100.0)

    # constant 16.67 ms -> 60 fps everywhere, no outliers
    const = [16.67] * 600
    st_c = stats_from(const)
    check("constant avg fps", round(st_c["avg_fps"], 1), 60.0, 0.2)
    check("constant p99", round(st_c["p99_ms"], 2), 16.67, 0.01)
    check("constant 1% low A", round(st_c["low1_a_fps"], 1), 60.0, 0.2)
    check("constant max", round(st_c["max_ms"], 2), 16.67, 0.01)

    # one huge hitch must move p99.9 and max but NOT p95
    hitch = [16.67] * 590 + [200.0] * 10
    st_h = stats_from(hitch)
    check("hitch p95 unaffected", round(st_h["p95_ms"], 2), 16.67, 0.01)
    check("hitch p99.9 catches it", round(st_h["p999_ms"], 1), 200.0, 0.5)
    check("hitch max", round(st_h["max_ms"], 1), 200.0, 0.01)
    check("hitch 0.1% low A is bad", st_h["low01_a_fps"] < 10.0 and True, True)

    # p99 of a 1..100 ramp must be 99 ms, not 2 ms. This is the case that
    # caught a wrong "fix": indexing the slow end breaks it.
    check("ramp p99 is 99 not 2", round(percentile(ramp, 99), 1), 99.0, 0.01)

    # A tail fatter than 1% must push p99 onto a slow frame.
    thick = [10.0] * 980 + [100.0] * 20
    st_th = stats_from(thick)
    check("2% tail p99 sees a slow frame", round(st_th["p99_ms"], 1), 100.0, 0.5)
    check("2% tail median unaffected", round(st_th["median_ms"], 2), 10.0, 0.01)
    check("2% tail 1% low A is low", st_th["low1_a_fps"] < 15.0 and True, True)

    # A 0.5% tail must NOT move p99 -- the tool must not invent a problem.
    thin = [10.0] * 995 + [100.0] * 5
    st_tn = stats_from(thin)
    check("0.5% tail leaves p99 fast", round(st_tn["p99_ms"], 1), 10.0, 0.01)

    # 1% low A and B differ when the worst 1% mixes fast and slow frames.
    st_t = stats_from([10.0] * 995 + [100.0] * 5)
    if abs(st_t["low1_a_fps"] - st_t["low1_b_fps"]) < 1.0:
        print(
            f"  FAIL 1% low A={st_t['low1_a_fps']:.1f} B={st_t['low1_b_fps']:.1f} "
            "should differ when the worst 1% mixes fast and slow frames"
        )
        ok = False
    else:
        print(
            f"  ok   1% low A={st_t['low1_a_fps']:.1f} "
            f"B={st_t['low1_b_fps']:.1f} differ on a mixed tail"
        )

    # stddev sanity
    check("stddev of 1..100", round(statistics.pstdev(ramp), 4), 28.8664, 1e-3)

    # loader: a MangoHud-style preamble must be skipped
    import tempfile

    mh = "\
# MangoHud version 0.7.1\n\
# skipping\n\
GPU, CPU, frametime, FPS\n\
100, 50, 16.0, 62.5\n\
100, 50, 16.5, 60.6\n\
100, 50, 17.0, 58.8\n"
    with tempfile.NamedTemporaryFile("w", suffix=".csv", delete=False) as f:
        f.write(mh)
        name = f.name
    st_m = stats(name)
    check("mangohud frames parsed", float(st_m["frames"]), 3.0)
    check("mangohud median", round(st_m["median_ms"], 2), 16.5, 0.01)
    import os

    os.unlink(name)

    # loader: plain one-column CSV
    with tempfile.NamedTemporaryFile("w", suffix=".csv", delete=False) as f:
        for v in ("10.0", "20.0", "30.0"):
            f.write(v + "\n")
        name2 = f.name
    st_p = stats(name2)
    check("plain csv frames", float(st_p["frames"]), 3.0)
    check("plain csv median", round(st_p["median_ms"], 2), 20.0, 0.01)
    os.unlink(name2)

    # non-positive values must be dropped, not counted as frames
    with tempfile.NamedTemporaryFile("w", suffix=".csv", delete=False) as f:
        f.write("16.0\n0\n-1\n16.0\n")
        name3 = f.name
    st_n = stats(name3)
    check("non-positive dropped", float(st_n["frames"]), 2.0)
    os.unlink(name3)

    print()
    print("SELF-TEST:", "PASS" if ok else "FAIL")
    return 0 if ok else 1


def stats_from(values):
    """Same as stats() but on an in-memory list, for the self test."""
    s = sorted(values)
    n = len(s)
    med = percentile(s, 50)
    mean = sum(s) / n
    p95 = percentile(s, 95)
    p99 = percentile(s, 99)
    p999 = percentile(s, 99.9)
    return {
        "path": "<memory>",
        "frames": n,
        "duration_s": sum(s) / 1000.0,
        "avg_fps": 1000.0 / mean,
        "median_ms": med,
        "mean_ms": mean,
        "p95_ms": p95,
        "p99_ms": p99,
        "p999_ms": p999,
        "max_ms": s[-1],
        "stddev_ms": statistics.pstdev(s) if n > 1 else 0.0,
        "low1_a_fps": 1000.0 / p99,
        "low1_b_fps": low_percent(s, 1.0),
        "low01_a_fps": 1000.0 / p999,
        "low01_b_fps": low_percent(s, 0.1),
        "over_2x_median": sum(1 for x in s if x > 2 * med),
        "over_33ms": sum(1 for x in s if x > 33.3),
    }


def main(argv):
    if "--self-test" in argv:
        return _self_test()
    args = [a for a in argv[1:] if not a.startswith("--")]
    if not args:
        print(__doc__)
        print("no input files")
        return 2
    if "--compare" in argv or len(args) > 1:
        compare(args)
    else:
        st = stats(args[0])
        print(fmt(st))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))