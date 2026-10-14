#!/bin/sh
# Run one Vulkan application with the layer and print the evidence that the
# library which ran is the one just built.
#
#   ./run.sh [seconds] [command...]
#
# Default: vkcube, 300 frames, on the AMD iGPU (`--gpu_number 0`; without it
# vkcube picks device 0 = the NVIDIA dGPU and creates no usable window). The
# WSI can be forced with `--wsi wayland|xlib`.
#
# Prints, in order:
#   * the .so path taken from /proc/<pid>/maps of the run (identity of what ran)
#   * the SHA-256 and mtime of the built .so (identity of what was built)
#   * the layer's own log, $XDG_RUNTIME_DIR/lapsphere/spike.log
set -e
here=$(cd "$(dirname "$0")" && pwd)
secs=${1:-60}
shift 2>/dev/null || true

if [ "$#" -gt 0 ]; then
    cmd="$*"
else
    cmd="vkcube --gpu_number 0 --c 300 --wsi xlib"
fi

log_dir="${XDG_RUNTIME_DIR:-/tmp}/lapsphere"
mkdir -p "$log_dir"
rm -f "$log_dir/spike.log"
rm -f "$log_dir"/frames-*

so="$here/target/release/liblapsphere_frames_layer.so"
echo "=== built .so: $so"
sha256sum "$so"
ls -l --time-style=+%Y-%m-%dT%H:%M:%S "$so"

echo "=== running: LAPSPHERE_FRAMES=1 $cmd   (timeout ${secs}s)"
LAPSPHERE_FRAMES=1 timeout "$secs" sh -c "$cmd" >"$here/results/app.out" 2>&1 &
app=$!
sleep 2
echo "=== mapped layer in /proc/$app/maps"
grep -i lapsphere_frames /proc/$app/maps 2>/dev/null | awk '{print $6}' | sort -u || true
wait $app && echo "=== app exit: 0" || echo "=== app exit: $?"

echo "=== app output (tail)"
tail -20 "$here/results/app.out" 2>/dev/null || true

echo "=== layer log ($log_dir/spike.log)"
cat "$log_dir/spike.log" 2>/dev/null || echo "(no log -- the layer never reached negotiate)"

echo "=== segments"
ls -l "$log_dir"/frames-* 2>/dev/null || echo "(no segment created)"

echo "=== reader (independent process) --list"
"$here/target/release/frames_reader" --list 2>&1 || true
