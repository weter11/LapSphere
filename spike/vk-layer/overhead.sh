#!/bin/sh
# Stage 1b, experiment 1: the layer's overhead per present, in microseconds.
#
#   ./overhead.sh [reps]          # default 3 reps per arm per regime (~10 min)
#
# ARMS (the manifest is installed in all of them; only the environment differs)
#   A  no layer        LAPSPHERE_FRAMES unset          -- the baseline
#   B  pass-through    LAPSPHERE_FRAMES_PASS=1         -- loaded, does nothing
#   C  recording       LAPSPHERE_FRAMES=1              -- writes intervals
#   D  recording+ovh   + LAPSPHERE_FRAMES_OVERHEAD=1   -- also samples its own cost
#
# METHODS
#   M1  in-band (arm D, vsync and unthrottled, MangoHud on/off): the layer's own
#       sample of its present-hook work, read out of the segment by frames_reader.
#       Direct measurement; what the window contains is stated in TESTING.md.
#   M3  throughput (arms A/B/C/D, unthrottled presents): the per-present cost is
#       the difference in present RATE, taken as a slope between two frame counts
#       so that startup cancels. It is the only method that includes everything
#       the hook does (queue-table lookup and call down included). It is a rate
#       delta -- NOT a frame time.
#
# FLOOR: run ./bench.sh in the same session. Its numbers say how much of M1 is
# the instrument's own clock read and how much is the layer.
#
# The interval distribution of a vsync'd run is NOT an overhead number: it is
# ~6.1 ms at 165 Hz and cannot resolve microseconds at all. It is reported by
# verify.sh (checks C2) as a plausibility check, not as overhead.
set -u
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
reps=${1:-3}
res="$here/results/overhead"
run="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
reader="$here/target/release/frames_reader"
app="vkcube --gpu_number 0 --wsi xlib"
mkdir -p "$res"
log="$res/overhead.txt"
: > "$log"
say() { echo "$@" | tee -a "$log"; }
clearseg() { rm -f "$run"/frames-* "$run"/spike.log; }

"$here/build.sh" > "$res/build.log" 2>&1
"$here/install.sh" plain > "$res/install.log" 2>&1

say "=== stage 1b experiment 1: overhead"
say "date            $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say "git HEAD        $(git -C "$here" rev-parse HEAD)"
say "dirty paths     $(git -C "$here" status --porcelain -- . | wc -l) under spike/vk-layer"
say "build_id        $(grep -o 'build_id=.*' "$res/build.log" | cut -d= -f2)"
say "so_sha256       $(sha256sum "$here/target/release/liblapsphere_frames_layer.so" | cut -d' ' -f1)"
say "reps            $reps"
say "app             $app"
say "mangohud conf   fps_limit is the user's default (120) when MangoHud is on"
say ""

# ---------------------------------------------------------------------------
# M1: in-band, arm D
# ---------------------------------------------------------------------------
m1() { # m1 <regime> <mangohud on|off> <rep>
    regime=$1; mh=$2; rep=$3
    case "$regime" in
        vsync)       args="$app --c 2000" ;;                 # ~12 s at 165 Hz
        unthrottled) args="$app --present_mode 0 --c 60000" ;;  # ~13 s at ~4700 fps
    esac
    if [ "$mh" = "on" ]; then mh_env="MANGOHUD=1"; else mh_env="MANGOHUD="; fi
    out="$res/m1-$regime-$mh-r$rep"
    clearseg
    # shellcheck disable=SC2086
    ( env $mh_env LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_OVERHEAD=1 timeout 60 $args \
        > "$out.app.out" 2>&1 ) &
    ap=$!
    # Poll until the application exits, keeping the LAST read that found a
    # segment: the segment is unlinked at vkDestroyInstance, so sampling once
    # early would report a prefix of the run (measured: 455 of 1999 intervals).
    sleep 2
    while kill -0 "$ap" 2>/dev/null; do
        "$reader" --overhead > "$out.overhead.tmp" 2>&1 || true
        "$reader" --dump "$out.frames.csv.tmp" > "$out.dump.tmp" 2>&1 || true
        "$reader" --list > "$out.list" 2>&1 || true
        if grep -q "samples" "$out.overhead.tmp" 2>/dev/null; then
            mv "$out.overhead.tmp" "$out.overhead"
            mv "$out.frames.csv.tmp" "$out.frames.csv"
        fi
        sleep 1
    done
    wait $ap; rc=$?
    cp "$run/spike.log" "$out.layer.log" 2>/dev/null || true
    python3 - "$out" "$regime" "$mh" "$rep" "$rc" >> "$log" <<'PY'
import re,sys
d,regime,mh,rep,rc=sys.argv[1:6]
ov=open(d+".overhead").read()
iv=[float(x) for x in open(d+".frames.csv").read().split()[1:]]
lg=open(d+".layer.log").read() if __import__("os").path.exists(d+".layer.log") else ""
wf=re.findall(r"writer_final: intervals=(\d+) dropped=(\d+) presents=(\d+)",lg)
pres=re.findall(r"QueuePresentKHR=(\d+)",lg)
m=re.search(r"samples (\d+).*?median ([\d.]+) us\s+p99 ([\d.]+) us\s+p99\.9 ([\d.]+) us\s+max ([\d.]+) us",ov,re.S)
print("M1  %-11s mangohud=%-3s r%s exit=%s" % (regime,mh,rep,rc))
if not m:
    print("    NO SAMPLES :: %s" % " | ".join(ov.strip().splitlines()[-2:])[:160]); raise SystemExit(0)
n,med,p99,p999,mx=[float(x) for x in m.groups()]
print("    samples=%-6d intervals=%-6d median=%.3f us p99=%.3f us p99.9=%.3f us max=%.3f us"
      % (n,len(iv),med,p99,p999,mx))
if wf:
    print("    writer_final intervals=%s dropped=%s presents=%s (sample/interval relation)"
          % wf[-1])
else:
    print("    writer_final absent; presents seen=%s" % (pres[-1] if pres else "?"))
PY
    say ""
}

say "--- M1: in-band overhead samples (arm D)"
for rep in $(seq 1 "$reps"); do
    m1 vsync off "$rep"
    m1 unthrottled off "$rep"
    m1 vsync on "$rep"
done

# ---------------------------------------------------------------------------
# M3: throughput A/B/C/D, unthrottled
# ---------------------------------------------------------------------------
say "--- M3: throughput, unthrottled presents, MangoHud absent"
N1=${M3_N1:-20000}
N2=${M3_N2:-150000}
say "    arms A/B/C/D; slope between --c $N1 and --c $N2 so startup cancels"
say "    arm order rotates every rep so a slow drift does not land on one arm"
: > "$res/rates.txt"
for rep in $(seq 1 "$reps"); do
    case $(( (rep - 1) % 4 )) in
        0) order="A B C D";;
        1) order="B C D A";;
        2) order="C D A B";;
        *) order="D A B C";;
    esac
    for arm in $order; do
        clearseg
        # The present counter the layer prints is an independent check that
        # --c N produced N presents (arm A has no layer, so no counter).
        set -- env MANGOHUD=
        case "$arm" in
            B) set -- env MANGOHUD= LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_PASS=1 ;;
            C) set -- env MANGOHUD= LAPSPHERE_FRAMES=1 ;;
            D) set -- env MANGOHUD= LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_OVERHEAD=1 ;;
        esac
        t1a=$(date +%s%N)
        "$@" timeout 90 $app --present_mode 0 --c "$N1" >/dev/null 2>&1
        t1b=$(date +%s%N)
        t2=$(date +%s%N)
        "$@" timeout 120 $app --present_mode 0 --c "$N2" >/dev/null 2>&1
        t3=$(date +%s%N)
        present_n=$(grep -o 'QueuePresentKHR=[0-9]*' "$run/spike.log" 2>/dev/null | tail -1 | cut -d= -f2)
        # rate = (N2-N1) / (T2 - T1), where T1/T2 are the durations of the two
        # runs: the one-time startup S cancels exactly (T = N/rate + S).
        rate=$(python3 -c "print('%.4f'%(($N2-$N1)/((($t3-$t2)-($t1b-$t1a))/1e9)))" 2>/dev/null)
        echo "$rep $arm $rate ${present_n:-0}" >> "$res/rates.txt"
        say "    r$rep arm=$arm rate=$rate fps (presents_seen=${present_n:-none})"
    done
    say ""
done

say "--- M3 result: per-present cost = 1e6*(1/rate_arm - 1/rate_A), microseconds"
python3 - "$res/rates.txt" >> "$log" <<'PY'
import sys,statistics
rows=[l.split() for l in open(sys.argv[1]).read().splitlines() if l.strip()]
by={}
for rep,arm,rate,pn in rows:
    by.setdefault(arm,{})[rep]=float(rate)
for arm in ["A","B","C","D"]:
    if arm not in by: continue
    rates=[by[arm][r] for r in sorted(by[arm])]
    print("arm %s rates %s fps  (median %.1f, spread %.2f%%)  presents_seen=%s"
          % (arm," ".join("%.0f"%r for r in rates),statistics.median(rates),
             100*(max(rates)-min(rates))/statistics.median(rates),
             ",".join(sorted({r[3] for r in rows if r[1]==arm}))))
for arm in ["B","C","D"]:
    if arm not in by or "A" not in by: continue
    d=[1e6*(1/by[arm][r]-1/by["A"][r]) for r in sorted(by[arm]) if r in by["A"]]
    if d:
        print("arm %s - arm A: %s us per present  (median %.3f us)"
              % (arm," ".join("%.2f"%x for x in d),statistics.median(d)))
print("NOTE: a rate delta is a per-present cost, not a frame time; it contains the")
print("      whole hook (queue-table lookup and call down included) and is only")
print("      valid while the present loop is CPU-serialised.")
PY
tail -12 "$log"
say ""
say "raw: $res  (per-run .overhead/.frames.csv/.layer.log, rates.txt)"
