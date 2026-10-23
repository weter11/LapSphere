#!/bin/sh
# Stage 1b, experiment 2: does this layer agree with MangoHud on the same load?
#
#   ./agreement.sh [reps]        # default 2 reps x 2 configurations
#
# Both instruments run at once on the same frames: MangoHud is BELOW this layer
# in the loader chain on this host, so every present this layer timestamps is
# the same present MangoHud logs. Two configurations, because MangoHud's own
# frame limiter changes the cadence it is measuring:
#
#   limiter=off    MANGOHUD_CONFIG=fps_limit=0     -> vsync, 165 Hz panel
#   limiter=120    MANGOHUD_CONFIG=fps_limit=120   -> the user's own default
#
# How the two are matched (the part that makes this a comparison and not a
# coincidence):
#   * MangoHud logs one row per frame with `frametime` (ms, 5 decimals) and
#     `elapsed`, nanoseconds since its log started.
#   * This reader's intervals are reconstructed onto absolute CLOCK_MONOTONIC
#     time: the last interval ends at the segment's `last_present_ns`, which
#     frames_reader --list reports as an age, so every earlier present time is
#     that minus the sum of the intervals after it.
#   * The unknown constant between the two series (MangoHud's log start) is
#     resolved by minimising the median per-frame residual over a grid.
#   * Only MangoHud frames inside our time coverage are compared; the number
#     dropped for falling outside it is reported, not hidden.
#
# What this does NOT establish: that either instrument matches the compositor's
# own present timing. Both hook vkQueuePresentKHR in the same process, so this
# is agreement about the same hook boundary -- not an absolute time base.
set -u
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
reps=${1:-2}
dur=${AGREE_DUR:-15}
res="$here/results/agreement"
run="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
reader="$here/target/release/frames_reader"
app="vkcube --gpu_number 0 --wsi xlib"
mkdir -p "$res"
report="$res/agreement.txt"
: > "$report"
say() { echo "$@" | tee -a "$report"; }

"$here/build.sh" > "$res/build.log" 2>&1
"$here/install.sh" plain > "$res/install.log" 2>&1
say "=== stage 1b experiment 2: agreement with MangoHud"
say "date            $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
say "git HEAD        $(git -C "$here" rev-parse HEAD)"
say "dirty paths     $(git -C "$here" status --porcelain -- . | wc -l) under spike/vk-layer"
say "build_id        $(grep -o 'build_id=.*' "$res/build.log" | cut -d= -f2)"
say "so_sha256       $(sha256sum "$here/target/release/liblapsphere_frames_layer.so" | cut -d' ' -f1)"
say "mangohud ver    $(dpkg-query -W -f='${Version}' mangohud 2>/dev/null)"
say "log_duration    ${dur}s per run; app --c $((dur * 190))"
say ""

one() { # one <limiter off|120> <rep>
    lim=$1; rep=$2
    case "$lim" in
        off) limv=0;;
        120) limv=120;;
    esac
    out="$res/lim$lim-r$rep"
    rm -rf "$out"; mkdir -p "$out/mh"
    rm -f "$run"/frames-* "$run"/spike.log
    python3 -c 'import time;print(time.monotonic_ns())' > "$out/launch_ns"
    ( env MANGOHUD=1 LAPSPHERE_FRAMES=1 \
        MANGOHUD_CONFIG="output_folder=$out/mh,autostart_log=1,log_duration=$dur,log_interval=0,fps_limit=$limv" \
        timeout 90 $app --c $((dur * 190)) > "$out/app.out" 2>&1 ) &
    ap=$!
    sleep 3
    # Poll; keep the last snapshot that found a live segment, plus the monotonic
    # clock reading taken with it (the anchor for absolute present times).
    while kill -0 "$ap" 2>/dev/null; do
        mono=$(python3 -c 'import time;print(time.monotonic_ns())')
        "$reader" --list > "$out/list.tmp" 2>&1 || true
        "$reader" --overhead > "$out/overhead.tmp" 2>&1 || true
        "$reader" --dump "$out/ours.csv.tmp" > "$out/dump.tmp" 2>&1 || true
        if grep -q "frames" "$out/list.tmp" 2>/dev/null && [ -s "$out/ours.csv.tmp" ]; then
            echo "$mono" > "$out/anchor_ns"
            mv "$out/ours.csv.tmp" "$out/ours.csv"
            mv "$out/list.tmp" "$out/list.txt"
            mv "$out/overhead.tmp" "$out/overhead.txt"
        fi
        sleep 1
    done
    wait $ap; rc=$?
    cp "$run/spike.log" "$out/layer.log" 2>/dev/null || true
    python3 - "$out" "$lim" "$rep" "$rc" "$report" <<'PY'
import sys,os,re,csv,statistics,bisect
out,lim,rep,rc,report=sys.argv[1:6]
def pctl(v,p):
    v=sorted(v)
    if not v: return 0.0
    return v[min(len(v)-1,max(0,int(len(v)*p)))]
lines=[]
iv=[float(x)*1000.0 for x in open(out+"/ours.csv").read().split()[1:]]   # us
lst=open(out+"/list.txt").read()
m=re.search(r"last_present\s+([\d.]+) s ago",lst)
anchor=int(open(out+"/anchor_ns").read().strip())
launch=int(open(out+"/launch_ns").read().strip())
age_ns=int(float(m.group(1))*1e9) if m else 0
t_last=anchor-age_ns
T=[]; acc=0
for x in reversed(iv):                     # absolute ns of every present we have
    T.append(t_last-acc); acc+=int(x*1000)
T.append(t_last-acc); T.sort()
mh=sorted(os.listdir(out+"/mh"))
if not mh:
    lines.append("limit=%-3s r%s exit=%s :: NO MANGOHUD LOG"%(lim,rep,rc))
    print("\n".join(lines)); open(report,"a").write("\n".join(lines)+"\n"); raise SystemExit(0)
rows=list(csv.reader(open(out+"/mh/"+mh[0])))
hdr=rows[2]; data=[r for r in rows[3:] if len(r)==len(hdr)]
ift=hdr.index("frametime"); iel=hdr.index("elapsed")
ft=[float(r[ift])*1000.0 for r in data]    # us
el=[int(float(r[iel])) for r in data]      # ns since MangoHud's log start

def resid(C,idx):
    rv=[]
    for k in idx:
        t=C+el[k]; j=bisect.bisect_left(T,t)
        rv.append(min(abs(t-T[x]) for x in (j-1,j,j+1) if 0<=x<len(T)) if 0<=j<=len(T) else 1e18)
    return statistics.median(rv)
# MangoHud's log starts during application startup: coarse grid around launch.
best=None
for C in range(launch-int(0.5e9),launch+int(3.5e9),1000000):
    mr=resid(C,range(0,min(len(el),300)))
    if best is None or mr<best[1]: best=(C,mr)
C0=best[0]
best=None
for C in range(C0-2000000,C0+2000001,20000):
    mr=resid(C,range(0,min(len(el),1000)))
    if best is None or mr<best[1]: best=(C,mr)
C=best[0]
pairs=open(out+"/pairs.csv","w")
pairs.write("mh_elapsed_ns,ours_present_ns,residual_us,mh_frametime_us,ours_interval_us\n")
resv=[]; diffs=[]; matched=0; outside=0
for k in range(len(el)):
    t=C+el[k]
    if t<T[0] or t>t_last:
        outside+=1; continue
    j=bisect.bisect_left(T,t)
    cand=[x for x in (j-1,j,j+1) if 0<=x<len(T)]
    jn=min(cand,key=lambda x:abs(t-T[x]))
    d=abs(t-T[jn])
    resv.append(d/1000.0)
    if d<=1_000_000 and 0<=jn-1<len(iv):
        matched+=1; diffs.append(iv[jn-1]-ft[k])
    pairs.write("%d,%d,%.3f,%.3f,%.3f\n"%(el[k],T[jn],d/1000.0,ft[k],
                iv[jn-1] if 0<=jn-1<len(iv) else -1))
pairs.close()
lines.append("limit=%-3s r%s exit=%s  log_start=%.3fs after launch  mh_rows=%d  our_intervals=%d  compared=%d  outside_window=%d"
             %(lim,rep,rc,(C-launch)/1e9,len(el),len(iv),len(resv),outside))
lines.append("    residual |dt| per frame (us): median %.2f  p99 %.2f  max %.2f"
             %(pctl(resv,0.50),pctl(resv,0.99),max(resv) if resv else 0))
lines.append("    MangoHud frametime : p50 %.4f ms  p99 %.4f ms  p99.9 %.4f ms  max %.4f ms  fps_p50 %.2f"
             %(pctl(ft,0.50)/1000,pctl(ft,0.99)/1000,pctl(ft,0.999)/1000,max(ft)/1000,1e6/pctl(ft,0.50)))
lines.append("    layer  intervals   : p50 %.4f ms  p99 %.4f ms  p99.9 %.4f ms  max %.4f ms  fps_p50 %.2f"
             %(pctl(iv,0.50)/1000,pctl(iv,0.99)/1000,pctl(iv,0.999)/1000,max(iv)/1000,1e6/pctl(iv,0.50)))
if diffs:
    lines.append("    paired frame-duration difference (ours - MangoHud): median %.2f us  p99 %.2f us  max %.2f us"
                 %(pctl(diffs,0.50),pctl(diffs,0.99),max(diffs)))
    lines.append("    p50 difference is %.3f %% of the frame time"
                 %(100*pctl(diffs,0.50)/pctl(ft,0.50)))
else:
    lines.append("    paired durations: NONE (nothing matched within 1 ms)")
print("\n".join(lines))
open(report,"a").write("\n".join(lines)+"\n")
PY
    say ""
}

say "--- runs (MangoHud below this layer, same presents)"
for rep in $(seq 1 "$reps"); do
    one off "$rep"
    one 120 "$rep"
done
tail -n +2 "$report" | tail -20
say "raw: $res/lim<off|120>-r<rep>/{mh/*.csv, ours.csv, pairs.csv, layer.log}"
