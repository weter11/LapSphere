#!/bin/sh
# Stage 1b, experiment 3: does the layer survive the things that happen to it?
#
#   ./robustness.sh [reps]      # default 2 (reader-cost repetitions)
#
# Sections:
#   R1  clean exit            (the baseline the others are compared against)
#   R2  kill -9               what is left behind, and does the next run work
#   R3  window resize         swapchain recreation, the origin reset, and
#                             whether the interval/present relation still holds
#   R4  two instances at once two live segments, two pids, pick_active behaviour
#   R5  no reader at all      the layer must not depend on being read
#   R6  reader cost and CPU   at a 60 Hz and a 240 Hz reader against writers at
#                             165 Hz (real vkcube), 60 Hz and 240 Hz (frames_synth)
#                             and ~4700 Hz (unthrottled vkcube, contention)
#   R7  stale segment + pid reuse: does an O_EXCL failure stay fail-open
#   R8  SIGSTOP/SIGCONT       (freeze the writer mid-run)
#
# Every section prints what it measured and where the raw output is. Nothing is
# called PASS on the strength of not having crashed.
set -u
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
reps=${1:-2}
res="$here/results/robustness"
run="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
reader="$here/target/release/frames_reader"
synth="$here/target/release/frames_synth"
app="vkcube --gpu_number 0 --wsi xlib"
mkdir -p "$res"
report="$res/robustness.txt"
: > "$report"
say() { echo "$@" | tee -a "$report"; }
clearseg() { rm -f "$run"/frames-* "$run"/spike.log; }
present_count() { grep -o 'QueuePresentKHR=[0-9]*' "$run/spike.log" 2>/dev/null | tail -1 | cut -d= -f2; }
seg_count() { ls "$run"/frames-* 2>/dev/null | wc -l; }
swap_create() { grep -o 'CreateSwapchainKHR=[0-9]*' "$run/spike.log" 2>/dev/null | tail -1 | cut -d= -f2; }

# Poll while <pid> lives, keeping the last dump taken from a live segment.
poll_dump() { # poll_dump <pid> <outfile>
    p=$1; out=$2
    rm -f "$out"
    while kill -0 "$p" 2>/dev/null; do
        "$reader" --dump "$out.tmp" > /dev/null 2>&1 || true
        [ -s "$out.tmp" ] && mv "$out.tmp" "$out"
        sleep 1
    done
}

# The invariant this crate rests on: the first present of a cadence writes no
# interval, and every swapchain recreation starts a new cadence. Asserted from
# the layer's own numbers, in the harness, rather than argued in prose.
check_invariant() { # check_invariant <csv> <label>
    csv=$1; label=$2
    if [ ! -s "$csv" ]; then
        say "    UNRESOLVED ($label): no interval dump was taken while the app lived"
        return
    fi
    python3 - "$csv" "$run/spike.log" "$label" >> "$report" <<'PY'
import sys,statistics,re
csvp,logp,label=sys.argv[1:4]
v=[float(x) for x in open(csvp).read().split()[1:]]
lg=open(logp).read()
pres=int(re.findall(r"QueuePresentKHR=(\d+)",lg)[-1])
cre=int(re.findall(r"CreateSwapchainKHR=(\d+)",lg)[-1])
wf=re.findall(r"writer_final: intervals=(\d+) dropped=(\d+) presents=(\d+)",lg)
iv=int(wf[-1][0]) if wf else -1
s=sorted(v); p50=statistics.median(s)
exp=pres-1-(cre-1)
print("    intervals dumped=%d  p50=%.3f ms  p99=%.3f ms  max=%.3f ms  intervals > 3x p50: %d"
      %(len(s),p50,s[int(len(s)*.99)],s[-1],sum(1 for x in v if x>3*p50)))
print("    invariant: presents - 1 - (swapchain creations - 1) = %d - 1 - %d = %d ; measured %d -> %s"
      %(pres,cre-1,exp,iv,"HOLDS" if iv==exp else "DOES NOT HOLD"))
if wf:
    print("    writer_final: intervals=%s dropped=%s presents=%s"%(wf[-1][0],wf[-1][1],wf[-1][2]))
PY
}

"$here/build.sh" > "$res/build.log" 2>&1
"$here/install.sh" plain > "$res/install.log" 2>&1
say "=== stage 1b experiment 3: robustness"
say "date            $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say "git HEAD        $(git -C "$here" rev-parse HEAD)"
say "dirty paths     $(git -C "$here" status --porcelain -- . | wc -l) under spike/vk-layer"
say "build_id        $(grep -o 'build_id=.*' "$res/build.log" | cut -d= -f2)"
say "so_sha256       $(sha256sum "$here/target/release/liblapsphere_frames_layer.so" | cut -d' ' -f1)"
say "reps            $reps"
say ""

# --- R1 ---------------------------------------------------------------------
say "--- R1 clean exit"
clearseg
( env LAPSPHERE_FRAMES=1 timeout 40 $app --c 900 > "$res/r1.app.out" 2>&1 ) &
ap=$!
sleep 3
say "    during run: segments=$(seg_count)"
wait $ap; rc=$?
segdur=$(seg_count)
say "    exit=$rc  presents=$(present_count)  $(grep -o 'writer_final: .*' "$run/spike.log" | tail -1)"
say "    segments after exit=$segdur (0 = unlinked at vkDestroyInstance)"
say ""

# --- R2 ---------------------------------------------------------------------
say "--- R2 kill -9"
clearseg
env LAPSPHERE_FRAMES=1 $app --c 100000 > "$res/r2.app.out" 2>&1 &
ap=$!
sleep 4
seg=$(ls "$run"/frames-* 2>/dev/null | head -1)
segpid=$(basename "$seg" 2>/dev/null | cut -d- -f2)
say "    app pid=$ap  segment pid=$segpid  same=$([ "$ap" = "$segpid" ] && echo yes || echo NO)"
kill -9 "$ap"; wait "$ap" 2>/dev/null; rc=$?
sleep 1
say "    wait status=$rc (137 = SIGKILL)"
say "    segments left=$(seg_count)  $(ls -l "$run"/frames-* 2>/dev/null | awk '{print "size="$5}')"
"$reader" --list > "$res/r2.list.txt" 2>&1
sed 's/^/    /' "$res/r2.list.txt" | tee -a "$report" > /dev/null
say "    writer_final in the layer log: $(grep -c 'writer_final' "$run/spike.log" 2>/dev/null) (0 = no clean shutdown, as expected)"
"$reader" --prune > "$res/r2.prune.txt" 2>&1
say "    $(tail -1 "$res/r2.prune.txt")   segments now=$(seg_count)"
clearseg
( env LAPSPHERE_FRAMES=1 timeout 30 $app --c 600 > "$res/r2b.app.out" 2>&1 ) &
ap=$!
sleep 3
wait $ap; rc=$?
say "    next run after the kill: exit=$rc presents=$(present_count) segments now=$(seg_count)"
say ""

# --- R3 ---------------------------------------------------------------------
say "--- R3 window resize (vkcube on Xvfb+lavapipe, resized by window id)"
say "    under Xvfb+lavapipe the frame rate is software-bound, and the resize is a"
say "    ConfigureWindow from the client, not a compositor drag."
say "    the host session itself offers no window control: xdotool/wmctrl see no"
say "    windows on the Xwayland display (measured below), which is why Xvfb."
if ! command -v Xvfb >/dev/null 2>&1 || ! command -v xdotool >/dev/null 2>&1; then
    say "    NOT TESTED: Xvfb or xdotool not available"
else
    # Evidence for the sentence above: with a host xlib window open, the X
    # client tooling of this session still sees nothing to control.
    clearseg
    ( env LAPSPHERE_FRAMES=1 timeout 20 $app --width 640 --height 480 --c 600 > /dev/null 2>&1 ) &
    ha=$!
    sleep 3
    say "    host session: DISPLAY=$DISPLAY  wmctrl lines=$(wmctrl -l 2>/dev/null | wc -l)  xdotool windows (any class)=$(xdotool search --all --class . 2>/dev/null | wc -l)  xdotool --class vkcube=$(xdotool search --class vkcube 2>/dev/null | wc -l)  (an xlib vkcube is running)"
    wait $ha 2>/dev/null
    clearseg
    Xvfb :98 -screen 0 1280x720x24 > "$res/r3.xvfb.log" 2>&1 &
    xv=$!
    sleep 2
    ( DISPLAY=:98 LAPSPHERE_FRAMES=1 timeout 60 vkcube --wsi xlib --width 640 --height 480 --c 2500 > "$res/r3.app.out" 2>&1 ) &
    ap=$!
    sleep 3
    # vkcube sets neither WM_CLASS nor WM_NAME, so xdotool's --class/--name
    # searches find nothing; the window id comes out of the tree instead.
    wid=$(DISPLAY=:98 xwininfo -root -tree 2>/dev/null | sed -n 's/^ *\(0x[0-9a-f]*\) .*[0-9]\+x[0-9]\++.*/\1/p' | head -1)
    if [ -z "$wid" ]; then
        say "    NOT TESTED: xwininfo found no window on :98"
        kill -9 "$ap" 2>/dev/null
    else
        say "    window id=$wid  before: $(DISPLAY=:98 xdotool getwindowgeometry "$wid" 2>&1 | tr '\n' ' ')"
        for wh in "1000 700" "500 400" "900 600"; do
            set -- $wh
            DISPLAY=:98 xdotool windowsize "$wid" "$1" "$2" 2>&1
            sleep 2
            say "    resized to $1x$2: $(DISPLAY=:98 xdotool getwindowgeometry "$wid" 2>&1 | tr '\n' ' ')"
        done
        poll_dump "$ap" "$res/r3.frames.csv"
        wait $ap; rc=$?
        cp "$run/spike.log" "$res/r3.layer.log" 2>/dev/null || true
        say "    exit=$rc  presents=$(present_count)  swapchain creations=$(swap_create)"
        say "    $(grep -o 'writer_final: .*' "$res/r3.layer.log" | tail -1)"
        cp "$res/r3.layer.log" "$run/spike.log" 2>/dev/null || true
        check_invariant "$res/r3.frames.csv" R3
    fi
    kill -9 "$xv" 2>/dev/null; wait "$xv" 2>/dev/null
fi
say ""

# --- R3b --------------------------------------------------------------------
say "--- R3b a second swapchain with no resize at all (host Wayland WSI)"
clearseg
( env LAPSPHERE_FRAMES=1 timeout 60 vkcube --gpu_number 0 --c 800 > "$res/r3b.app.out" 2>&1 ) &
ap=$!
sleep 3
poll_dump "$ap" "$res/r3b.frames.csv"
wait $ap; rc=$?
cp "$run/spike.log" "$res/r3b.layer.log" 2>/dev/null || true
say "    exit=$rc  presents=$(present_count)  swapchain creations=$(swap_create)"
cp "$res/r3b.layer.log" "$run/spike.log" 2>/dev/null || true
check_invariant "$res/r3b.frames.csv" R3b
say ""

# --- R4 ---------------------------------------------------------------------
say "--- R4 two instances at once"
clearseg
env LAPSPHERE_FRAMES=1 timeout 40 $app --c 900 > "$res/r4a.app.out" 2>&1 &
ap=$!
sleep 1
sega=$(ls "$run"/frames-* 2>/dev/null | head -1)
env LAPSPHERE_FRAMES=1 timeout 40 vkcube --gpu_number 1 --wsi wayland --c 900 > "$res/r4b.app.out" 2>&1 &
sleep 3
segb=$(ls "$run"/frames-* 2>/dev/null | grep -v "$(basename "$sega" 2>/dev/null)" | head -1)
pidA=$(basename "$sega" 2>/dev/null | cut -d- -f2)
pidB=$(basename "$segb" 2>/dev/null | cut -d- -f2)
say "    segments=$(seg_count)  pidA=$pidA (xlib, dGPU)  pidB=$pidB (wayland, iGPU)"
"$reader" --list > "$res/r4.list.txt" 2>&1
sed 's/^/    /' "$res/r4.list.txt"
"$reader" --dump "$res/r4.ours.csv" > "$res/r4.dump.txt" 2>&1
say "    $(cat "$res/r4.dump.txt")"
say "    SIGKILL the second instance (pid $pidB) while the first keeps presenting"
kill -9 "$pidB" 2>/dev/null
sleep 1
"$reader" --dump "$res/r4-afterkill.csv" > "$res/r4.dump2.txt" 2>&1
say "    $(cat "$res/r4.dump2.txt")"
say "    after the kill, --list:"
"$reader" --list 2>&1 | sed 's/^/    /'
wait $ap; rc=$?
say "    first instance exit=$rc (0 = a killed sibling does not disturb it)"
say "    segments after the first exited: $(seg_count) (the killed writer's orphan is still there)"
"$reader" --prune > /dev/null 2>&1
say "    after --prune: segments=$(seg_count)"
say ""

# --- R5 ---------------------------------------------------------------------
say "--- R5 no reader at all"
clearseg
env LAPSPHERE_FRAMES=1 timeout 40 $app --c 900 > "$res/r5.app.out" 2>&1
rc=$?
sleep 1
say "    exit=$rc (no frames_reader process was started in this section)"
say "    presents=$(present_count)  $(grep -o 'writer_final: .*' "$run/spike.log" | tail -1)"
say "    segments after exit=$(seg_count)"
say ""

# --- R6 ---------------------------------------------------------------------
say "--- R6 reader cost and CPU vs reader rate"
r6_run() { # r6_run <label> <hz>
    label=$1; hz=$2
    out="$res/r6-$label-$hz"
    "$reader" --watch --hz "$hz" --seconds 8 --cpu > "$out.txt" 2>&1
    say "    $label @${hz}Hz: $(head -1 "$out.txt")"
    say "                      $(sed -n 2p "$out.txt")  $(sed -n 3p "$out.txt" | sed 's/^ *//')"
}
r6_writer() { # r6_writer <label> <reps> -- <writer command...>
    label=$1; reps_l=$2; shift 3
    i=1
    while [ "$i" -le "$reps_l" ]; do
        clearseg
        "$@" > "$res/r6-$label-$i.writer.out" 2>&1 &
        wp=$!
        sleep 2
        r6_run "$label-r$i" 60
        r6_run "$label-r$i" 240
        kill "$wp" 2>/dev/null; wait "$wp" 2>/dev/null
        # freshness of what the reader sees, sampled while the writer is live
        i=$((i + 1))
    done
}

# real vkcube at the panel rate
r6_writer vkcube165 "$reps" -- env LAPSPHERE_FRAMES=1 timeout 60 $app --c 4000
# synthetic writers at exactly 60 and 240 Hz (same shm::Writer, no Vulkan)
r6_writer synth60 "$reps" -- "$synth" --hz 60 --seconds 25 --label synth60
r6_writer synth240 "$reps" -- "$synth" --hz 240 --seconds 25 --label synth240
# contention: unthrottled presents, ~4700 Hz, ring wrapping every ~0.9 s
r6_writer unthrottled 1 -- env LAPSPHERE_FRAMES=1 timeout 60 $app --present_mode 0 --c 200000

# freshness: what age does a reader see at all
say "    freshness (age printed by --list while a writer runs):"
for lbl in "synth60:$synth --hz 60 --seconds 8" "synth240:$synth --hz 240 --seconds 8"; do
    name=${lbl%%:*}; cmd=${lbl#*:}
    clearseg
    $cmd --label "$name" >/dev/null 2>&1 &
    wp=$!
    sleep 1
    ages=""
    n=0
    while [ "$n" -lt 15 ]; do
        # field positions in --list shift when a long exe name is padded, so
        # take the age by name rather than by column
        ages="$ages $("$reader" --list 2>/dev/null | sed -n 2p | sed 's/.*last_present *//; s/ s ago.*//')"
        n=$((n + 1))
    done
    wait $wp 2>/dev/null
    say "      $name: ages(s) =$ages"
done
say ""

# --- R6b --------------------------------------------------------------------
say "--- R6b what the read cost is made of (median, one 60 Hz reader)"
say "    the reader's number is a poll cost, not a copy cost: the window opens"
say "    right after a sleep wakeup, so it pays a cold path that the hot-loop"
say "    floors in results/primitives.txt do not model."
r6b() { # r6b <label>
    "$reader" --watch --hz 60 --seconds 4 > "$res/r6b-$1.txt" 2>&1
    say "    $(printf '%-28s' "$1")$(sed -n 2p "$res/r6b-$1.txt" | sed 's/^ *//')  $(sed -n 1p "$res/r6b-$1.txt" | sed 's/.*max_frame_count=/frames=/' )"
}
clearseg
r6b "no segment"
clearseg
"$synth" --hz 240 --seconds 3 --label fill > /dev/null 2>&1 &
sp=$!; sleep 2.5; kill -9 $sp 2>/dev/null; wait $sp 2>/dev/null
r6b "static segment, small ring"
clearseg
"$synth" --hz 240 --seconds 20 --label stale > /dev/null 2>&1 &
sp=$!; sleep 18; kill -9 $sp 2>/dev/null; wait $sp 2>/dev/null
r6b "static segment, ring full"
clearseg
"$synth" --hz 60 --seconds 8 --label live60 > /dev/null 2>&1 &
sp=$!; sleep 1
r6b "live synth 60 Hz"
wait $sp 2>/dev/null
clearseg
env LAPSPHERE_FRAMES=1 timeout 20 $app --c 3000 > /dev/null 2>&1 &
ap=$!; sleep 2
r6b "live vkcube 165 Hz"
wait $ap 2>/dev/null
clearseg
say ""

# --- R7 ---------------------------------------------------------------------
say "--- R7 writer creation failure stays fail-open"
clearseg
mkdir -p "$run"
chmod 555 "$run"
env LAPSPHERE_FRAMES=1 timeout 30 $app --c 600 > "$res/r7.app.out" 2>&1
rc=$?
# The same failure through frames_synth, which prints the error to stderr: the
# layer's own log_line writes into the very directory that is unwritable here
# (measured: the message is absent from the app's stderr, and log_line only
# writes $XDG_RUNTIME_DIR/lapsphere/spike.log), so the app-level evidence is
# "ran anyway, no segment", and this is the error text itself.
rm -rf "$res/r7dir"; mkdir -p "$res/r7dir/lapsphere"; chmod 555 "$res/r7dir/lapsphere"
XDG_RUNTIME_DIR="$res/r7dir" "$synth" --hz 60 --seconds 1 --label r7 > "$res/r7.synth.out" 2>&1
synth_rc=$?
chmod 755 "$run"
say "    segment directory made unwritable (chmod 555): vkcube exit=$rc (0 = the app ran anyway), segments now=$(seg_count)"
say "    writer creation error text, from a process that prints it: exit=$synth_rc :: $(head -1 "$res/r7.synth.out")"
say "    NOTE: the EEXIST variant (a recycled pid meeting an old segment) needs a pid"
say "          namespace to produce and unprivileged ones are unavailable here"
say "          (measured: 'unshare -p -f --mount-proc' -> Operation not permitted;"
say "          'unshare -U -p -f' -> cannot change root filesystem propagation)."
say "          It stays NOT TESTED; it is not passed by default."
say ""

# --- R8 ---------------------------------------------------------------------
say "--- R8 SIGSTOP/SIGCONT of the writer mid-run"
clearseg
env LAPSPHERE_FRAMES=1 $app --c 2000 > "$res/r8.app.out" 2>&1 &
ap=$!
sleep 3
before=$("$reader" --list 2>/dev/null | sed -n 2p)
kill -STOP "$ap"
sleep 2
stopped=$("$reader" --list 2>/dev/null | sed -n 2p)
"$reader" --overhead > "$res/r8.stopped.overhead" 2>&1
andoverhead=$(cat "$res/r8.stopped.overhead" | tr '\n' ' ')
kill -CONT "$ap"
sleep 3
after=$("$reader" --list 2>/dev/null | sed -n 2p)
wait $ap; rc=$?
say "    before STOP : $before"
say "    during STOP : $stopped"
say "    --overhead while stopped: $andoverhead"
say "    after CONT  : $after"
say "    exit=$rc  $(grep -o 'writer_final: .*' "$run/spike.log" | tail -1)"
say ""
say "raw: $res"
