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
