#!/bin/sh
# Run a Vulkan app with the layer, then prove that the library which actually
# ran is the one just built.
#
#   ./run.sh [seconds] [extra vkcube args...]
#
# Writes the layer's own log to $XDG_RUNTIME_DIR/lapsphere/spike.log, and
# prints the mapped .so path taken from /proc/<pid>/maps of the run.
here=$(cd "$(dirname "$0")" && pwd)
secs=${1:-10}
shift 2>/dev/null

log_dir="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
rm -f "$log_dir/spike.log"
mkdir -p "$log_dir"
# clear stale segments so the listing cannot show a previous run
rm -f "$log_dir"/frames-*

before=$(ls -l --time-style=full-iso "$here/target/release/liblapsphere_frames_layer.so" | awk '{print $6,$7,$5}')

LAPSPHERE_FRAMES=1 timeout "$secs" "$@" >/dev/null 2>&1 &
app=$!
sleep 2
echo "=== mapped layer in /proc/$app/maps"
grep -i lapsphere_frames /proc/$app/maps 2>/dev/null | awk '{print $6}' | sort -u
wait $app 2>/dev/null

echo "=== built .so before run: $before"
echo "=== layer log ($log_dir/spike.log)"
cat "$log_dir/spike.log" 2>/dev/null || echo "(no log -- layer never reached negotiate)"
