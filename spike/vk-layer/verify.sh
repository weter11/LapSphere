#!/bin/sh
# Acceptance harness for stage 1: does the layer really intercept presents?
#
#   ./verify.sh [gpu_index]
#
# Every check prints PASS/FAIL with the numbers it read, and the raw material
# (app output, layer log, reader output, dumped intervals) lands in ./results/.
# Nothing here is a demonstration value: the numbers are read back out of a
# live shared-memory segment by a *separate process*.
#
# Checks, in the order the stage's acceptance criteria state them:
#   C1  a real present is intercepted: vkQueuePresentKHR is entered once per
#       frame of a real Vulkan app, and the app still exits 0
#   C2  frame_count increases, intervals are recorded, an independent reader
#       reads them, and the numbers are physically plausible
#   C3  with the layer not enabled: the app runs normally and no segment exists
#   C4  the manifest's legacy `functions` map changes nothing (the round-1
#       hypothesis); both manifest variants are run
#   D   regressions found while measuring stage 1b: the reader's read cost must
#       contain the read it names (D1), a reader started before the application
#       must see its segment (D2), and an instrumented run must yield one
#       overhead sample per interval (D3)
#
# No GUI, panel, daemon or settings file is touched.
set -u

here=$(cd "$(dirname "$0")" && pwd)
gpu=${1:-0}
res="$here/results"
readerdir="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
reader="$here/target/release/frames_reader"
app="vkcube --gpu_number $gpu --wsi xlib --c 900"
# The same app without a frame limit, for the ring-wraparound check.
app_unlimited="vkcube --gpu_number $gpu --wsi xlib"
fails=0

mkdir -p "$res"
: > "$res/verify.txt"

say() { echo "$@" | tee -a "$res/verify.txt"; }
ck() { # ck <name> <condition-result> <evidence>
    if [ "$2" = "0" ]; then
        say "PASS  $1 :: $3"
    else
        say "FAIL  $1 :: $3"
        fails=$((fails + 1))
    fi
}

say "=== stage 1 verification"
say "date            $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say "git HEAD        $(git -C "$here" rev-parse HEAD)"
say "git status      $(git -C "$here" status --porcelain -- . | wc -l) dirty path(s) under spike/vk-layer"
say "loader          $(ls -l /usr/lib/x86_64-linux-gnu/libvulkan.so.1 | sed 's/.*-> //')"
say "gpu index       $gpu"
say "app             $app"
say ""

# --- build + install -------------------------------------------------------
say "--- build"
"$here/build.sh" > "$res/build.log" 2>&1
build_rc=$?
tail -4 "$res/build.log" | tee -a "$res/verify.txt"
so="$here/target/release/liblapsphere_frames_layer.so"
so_sha=$(sha256sum "$so" | cut -d' ' -f1)
build_id=$(grep -o 'build_id=.*' "$res/build.log" | cut -d= -f2)
ck "build" "$build_rc" "exit=$build_rc build_id=$build_id"

# --- C1/C2 on the plain manifest ------------------------------------------
run_and_observe() { # run_and_observe <label> <env...>
    label=$1
    shift
    rm -f "$readerdir/spike.log"
    rm -f "$readerdir"/frames-*
    ( env "$@" timeout 40 $app > "$res/$label.app.out" 2>&1 ) &
    app_pid=$!
    sleep 3
    missed=0
    seg_during=$(ls "$readerdir"/frames-* 2>/dev/null | wc -l)
    "$reader" --list > "$res/$label.reader.list" 2>&1
    sleep 2
    # The dump is taken last on purpose: it is a single snapshot, and the app's
    # first seconds are startup, not frames.
    "$reader" --dump "$res/$label.frames.csv" > "$res/$label.reader.dump" 2>&1
    "$reader" --list > "$res/$label.reader2.list" 2>&1
    cp "$readerdir/spike.log" "$res/$label.layer.log" 2>/dev/null || missed=1
    wait $app_pid
    app_rc=$?
    cp "$readerdir/spike.log" "$res/$label.layer.log" 2>/dev/null || true
    seg_after=$(ls "$readerdir"/frames-* 2>/dev/null | wc -l)
    return $app_rc
}

say ""
say "--- check 1 + 2: interception on the plain manifest"
"$here/install.sh" plain > "$res/install-plain.log" 2>&1
run_and_observe c1_plain LAPSPHERE_FRAMES=1
app_rc=$?
present_n=$(grep -o 'QueuePresentKHR=[0-9]*' "$res/c1_plain.layer.log" 2>/dev/null | tail -1 | cut -d= -f2)
present_n=${present_n:-0}
reader_frames=$(sed -n '2p' "$res/c1_plain.reader.list" | awk '{print $6}')
reader_frames2=$(sed -n '2p' "$res/c1_plain.reader2.list" | awk '{print $6}')
ck "C1.app_exit0" "$app_rc" "vkcube exit=$app_rc (a layer that breaks the app cannot pass)"
ck "C1.present_intercepted" "$([ "$present_n" -gt 800 ] && echo 0 || echo 1)" \
   "vkQueuePresentKHR entered ${present_n}x (counter from the layer's own log)"
ck "C1.segment_during_run" "$([ "${seg_during:-0}" -gt 0 ] && echo 0 || echo 1)" \
   "segment files present while the app ran: $seg_during"
ck "C2.frame_count_known_to_reader" "$([ -n "${reader_frames:-}" ] && echo 0 || echo 1)" \
   "authoritative reader read frame_count=$reader_frames from the segment"
# The interval count is asserted against the present count, both read from the
# layer's own final line: exactly one present per cadence writes no interval.
writer_final=$(sed 's/^\[[0-9]* [0-9]*\] //' "$res/c1_plain.layer.log" 2>/dev/null | grep -o 'writer_final: intervals=[0-9]*' | tail -1 | cut -d= -f2)
ck "C2.intervals_are_presents_minus_one" \
   "$([ -n "${writer_final:-}" ] && [ "${writer_final}" -eq "$((present_n - 1))" ] && echo 0 || echo 1)" \
   "final intervals=${writer_final:-<no writer_final line>} for ${present_n} presents (the first present of the cadence writes no interval)"
ck "C2.frame_count_increases" \
   "$([ "${reader_frames2:-0}" -gt "${reader_frames:-0}" ] && echo 0 || echo 1)" \
   "reader samples 3 s apart: $reader_frames -> $reader_frames2"
ck "C2.segment_gone_after_exit" "$([ "${seg_after:-1}" -eq 0 ] && echo 0 || echo 1)" \
   "segment unlinked at vkDestroyInstance (${seg_after} left)"

say ""
say "--- check 2b: the dumped intervals are physical"
stat_line=$(python3 - "$res/c1_plain.frames.csv" <<'PY' 2>&1
import sys, statistics
try:
    v = [float(x) for x in open(sys.argv[1]).read().split()[1:]]
except Exception as e:
    print("ERROR", e); raise SystemExit(1)
if not v:
    print("ERROR no intervals"); raise SystemExit(1)
v.sort()
p50 = statistics.median(v)
print("n=%d min=%.3fms p50=%.3fms p99=%.3fms max=%.3fms fps_p50=%.1f"
      % (len(v), v[0], p50, v[int(len(v)*0.99)], v[-1], 1000.0/p50))
PY
)
echo "  $stat_line" | tee -a "$res/verify.txt"
n_iv=$(echo "$stat_line" | awk '{print $1}' | cut -d= -f2)
ok_range=$(python3 - "$res/c1_plain.frames.csv" <<'PY' 2>&1
import sys, statistics
v = [float(x) for x in open(sys.argv[1]).read().split()[1:]]
p50 = statistics.median(v)
print(0 if 0.5 <= p50 <= 100.0 else 1)
PY
)
ck "C2.intervals_recorded" "$([ "${n_iv:-0}" -ge 100 ] && echo 0 || echo 1)" \
   "reader dumped ${n_iv} intervals from the ring"
ck "C2.intervals_plausible" "$ok_range" "p50 inside [0.5 ms, 100 ms] -> $stat_line"
ck "C2.reader_is_a_separate_process" "$([ -s "$res/c1_plain.reader.dump" ] && echo 0 || echo 1)" \
   "frames_reader is a second process reading the same segment: $(cat "$res/c1_plain.reader.dump" | head -1)"

say ""
say "--- check 3: the layer must be inert when it is not enabled"
"$here/install.sh" plain >/dev/null 2>&1
rm -f "$readerdir/spike.log" "$readerdir"/frames-*
( VK_LOADER_DEBUG=layer timeout 20 $app > "$res/c3_off.app.out" 2>&1 ) &
off_pid=$!
sleep 3
seg_off=$(ls "$readerdir"/frames-* 2>/dev/null | wc -l)
wait $off_pid
off_rc=$?
seg_off_after=$(ls "$readerdir"/frames-* 2>/dev/null | wc -l)
log_off=$(ls "$readerdir/spike.log" 2>/dev/null | wc -l)
enabled_line=$(grep -c "LAPSPHERE_frames" "$res/c3_off.app.out" 2>/dev/null)
ck "C3.app_exit0_unset" "$off_rc" "vkcube without LAPSPHERE_FRAMES exit=$off_rc"
ck "C3.no_segment" "$([ "${seg_off:-1}" -eq 0 ] && echo 0 || echo 1)" \
   "segments during the run: $seg_off (after: $seg_off_after)"
ck "C3.no_layer_log" "$([ "${log_off:-1}" -eq 0 ] && echo 0 || echo 1)" \
   "layer never ran (spike.log absent=$([ "${log_off:-1}" -eq 0 ] && echo yes || echo no))"
ck "C3.loader_did_not_activate" \
   "$([ "$(grep -c 'VK_LAYER_LAPSPHERE_frames    This layer was enabled' "$res/c3_off.app.out" 2>/dev/null)" -eq 0 ] && echo 0 || echo 1)" \
   "loader log mentions of an *enabled* lapsphere layer: $(grep -c 'VK_LAYER_LAPSPHERE_frames    This layer was enabled' "$res/c3_off.app.out" 2>/dev/null)"

say ""
say "--- check 3b: disable_environment also turns it off"
rm -f "$readerdir/spike.log" "$readerdir"/frames-*
( LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_DISABLE=1 timeout 20 $app > "$res/c3b_disable.app.out" 2>&1 ) &
d_pid=$!
sleep 3
seg_d=$(ls "$readerdir"/frames-* 2>/dev/null | wc -l)
wait $d_pid
d_rc=$?
ck "C3b.app_exit0_disable_env" "$d_rc" "exit=$d_rc with LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_DISABLE=1"
ck "C3b.no_segment" "$([ "${seg_d:-1}" -eq 0 ] && echo 0 || echo 1)" "segments: $seg_d"

say ""
say "--- check 4: the round-1 hypothesis (manifest 'functions' map)"
"$here/install.sh" functions > "$res/install-functions.log" 2>&1
run_and_observe c4_functions LAPSPHERE_FRAMES=1
app_rc=$?
present_n4=$(grep -o 'QueuePresentKHR=[0-9]*' "$res/c4_functions.layer.log" 2>/dev/null | tail -1 | cut -d= -f2)
present_n4=${present_n4:-0}
deprecated=$(grep -c "using deprecated" "$res/c4_functions.layer.log" 2>/dev/null)
ck "C4.app_exit0_functions_manifest" "$app_rc" "vkcube exit=$app_rc"
ck "C4.present_intercepted_too" "$([ "$present_n4" -gt 0 ] && echo 0 || echo 1)" \
   "vkQueuePresentKHR entered ${present_n4}x with the legacy functions map installed"

say ""
say "--- check D: reader regressions found in stage 1b"
# D1: the read cost must include the read. `--watch` timestamps used to sit on
# either side of `pick_active` only, so the mmap+copy of every segment was
# outside the window and the reported cost was ~0.0 us. The floor comes from
# primitive_bench on this host in this session (a hot-loop mmap+copy of the
# same 64 KiB), so the comparison is not against a constant.
if [ ! -s "$res/primitives.txt" ]; then
    "$here/target/release/primitive_bench" --iters 200000 --io-iters 3000 > "$res/primitives.txt" 2>&1
fi
floor_us=$(grep '^mmap_read ' "$res/primitives.txt" | sed 's/.*median= *//; s/ ns.*//')
floor_us=$(python3 -c "print('%.3f'%(${floor_us:-6700}/1000.0))")
segs_dir="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
rm -f "$segs_dir"/frames-* "$segs_dir"/spike.log
# a writer whose ring is full, so the reader copies the same 64 KiB the floor did
( "$here/target/release/frames_synth" --hz 240 --seconds 22 --label d1 > "$res/d1.synth.out" 2>&1 ) &
syn=$!
sleep 18
"$reader" --watch --hz 60 --seconds 3 > "$res/d1.watch.txt" 2>&1
read_us=$(grep -o 'read cost: median [0-9.]*' "$res/d1.watch.txt" | awk '{print $4}')
read_us=${read_us:-0}
wait $syn 2>/dev/null
ck "D1.read_cost_includes_the_read" \
   "$(python3 -c "print(0 if ${read_us} >= ${floor_us} else 1)")" \
   "reader median ${read_us} us vs primitive_bench hot mmap_read floor ${floor_us} us (a window that excludes the copy reads ~0 us)"
# D2: a reader started before the application must see the segment that appears
# later. The segment list used to be cached before the loop, so --watch reported
# max_frame_count=0 for its whole life when it was started first.
rm -f "$segs_dir"/frames-* "$segs_dir"/spike.log
( "$reader" --watch --hz 60 --seconds 10 > "$res/d2.watch.txt" 2>&1 ) &
rpid=$!
sleep 2
( env LAPSPHERE_FRAMES=1 timeout 6 $app > "$res/d2.app.out" 2>&1 ) &
apid=$!
wait $apid
wait $rpid
seen=$(sed -n '1p' "$res/d2.watch.txt" | sed 's/.*max_frame_count=//')
ck "D2.watch_sees_late_segment" "$([ "${seen:-0}" -gt 0 ] && echo 0 || echo 1)" \
   "reader started 2 s before the app, max_frame_count it saw: ${seen:-0}"
# D3: an instrumented run must produce one overhead sample per recorded
# interval (both are indexed by the same frame counter and both are bounded by
# the same ring), and the samples cannot be smaller than the clock pair they
# are made of. Tolerates the few frames that pass between two reader calls.
rm -f "$segs_dir"/frames-* "$segs_dir"/spike.log
( env LAPSPHERE_FRAMES=1 LAPSPHERE_FRAMES_OVERHEAD=1 timeout 40 vkcube --gpu_number "$gpu" --wsi xlib --c 1200 > "$res/d3.app.out" 2>&1 ) &
ap=$!
sleep 2
while kill -0 "$ap" 2>/dev/null; do
    "$reader" --overhead > "$res/d3.overhead.tmp" 2>&1 || true
    "$reader" --dump "$res/d3.frames.csv.tmp" > "$res/d3.dump.tmp" 2>&1 || true
    if grep -q samples "$res/d3.overhead.tmp" 2>/dev/null; then
        mv "$res/d3.overhead.tmp" "$res/d3.overhead"
        mv "$res/d3.frames.csv.tmp" "$res/d3.frames.csv"
    fi
    sleep 1
done
wait $ap; rc3=$?
n_samp=$(grep -o 'samples [0-9]*' "$res/d3.overhead" 2>/dev/null | awk '{print $2}')
n_iv=$(($(wc -l < "$res/d3.frames.csv" 2>/dev/null || echo 1) - 1))
med_us=$(grep -o 'median [0-9.]* us' "$res/d3.overhead" 2>/dev/null | awk '{print $2}')
clock_ns=$(grep '^clock_pair ' "$res/primitives.txt" | sed 's/.*median= *//; s/ ns.*//')
clock_us=$(python3 -c "print('%.3f'%(${clock_ns:-20}/1000.0))")
ck "D3.one_overhead_sample_per_interval" \
   "$(python3 -c "s=${n_samp:-0}; i=${n_iv:-0}; print(0 if s > 0 and abs(s-i) <= 8 else 1)")" \
   "exit=$rc3 samples=${n_samp:-0} intervals=${n_iv:-0} (same ring, same index; a mismatch of a few frames is the gap between the two reader calls)"
ck "D3.samples_above_the_clock_floor" \
   "$(python3 -c "print(0 if ${med_us:-0} >= ${clock_us} else 1)")" \
   "median sample ${med_us:-0} us vs the clock pair it contains (${clock_us} us, primitive_bench)"
say ""

say ""
if [ "${VERIFY_LONG:-0}" = "1" ]; then
    say "--- check 5: ring wraparound (frame_count beyond the 4096-slot ring)"
    rm -f "$readerdir/spike.log" "$readerdir"/frames-*
    ( LAPSPHERE_FRAMES=1 timeout 90 $app_unlimited > "$res/c5_wrap.app.out" 2>&1 ) &
    w_pid=$!
    sleep 50
    "$reader" --dump "$res/c5_wrap.frames.csv" > "$res/c5_wrap.reader.dump" 2>&1
    "$reader" --list > "$res/c5_wrap.reader.list" 2>&1
    kill "$w_pid" 2>/dev/null
    wait "$w_pid" 2>/dev/null
    cp "$readerdir/spike.log" "$res/c5_wrap.layer.log" 2>/dev/null || true
    wrap=$(python3 - "$res/c5_wrap.frames.csv" <<'PY' 2>&1
import statistics, sys
v = [float(x) for x in open(sys.argv[1]).read().split()[1:]]
v.sort()
if not v:
    print("0 0 0"); raise SystemExit(0)
print(len(v), sum(1 for x in v if x == 0.0), round(statistics.median(v), 3))
PY
)
    set -- $wrap
    n_wrap=${1:-0}; zeros_wrap=${2:-1}; p50_wrap=${3:-0}
    say "  frames recorded by the layer: $(grep -o 'frame_count[^ ]*' "$res/c5_wrap.reader.list" | head -1) ; reader returned n=$n_wrap zeros=$zeros_wrap p50=${p50_wrap}ms"
    ck "C5.ring_full" "$([ "${n_wrap:-0}" -eq 4096 ] && echo 0 || echo 1)" \
       "intervals returned once the ring has wrapped: $n_wrap (capacity 4096)"
    ck "C5.no_zero_slots" "$([ "${zeros_wrap:-1}" -eq 0 ] && echo 0 || echo 1)" \
       "zero-valued intervals in the wrapped ring: $zeros_wrap"
    ck "C5.still_plausible" "$(python3 -c "print(0 if 0.5 <= ${p50_wrap:-0} <= 100.0 else 1)")" \
       "p50 after wraparound: ${p50_wrap} ms"
else
    say "--- check 5 skipped (set VERIFY_LONG=1 to include the ~50 s ring-wrap run)"
fi

say ""
say "--- artifacts"
say "results dir     $res"
say "layer log       $res/c1_plain.layer.log (the chain walk, counters, presents)"
say "reader list     $res/c1_plain.reader.list"
say "intervals       $res/c1_plain.frames.csv"
say "built .so       $so"
say "so sha256       $so_sha"
say "build id        $build_id"
say ""
if [ "$fails" -eq 0 ]; then
    say "RESULT: all checks passed"
else
    say "RESULT: $fails check(s) FAILED"
fi
exit "$fails"
