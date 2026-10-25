#!/bin/sh
# Out-of-band floors for stage 1b: what the instrument and the reader's own
# primitives cost on THIS host, measured with no Vulkan, no loader and no game
# in the process.
#
#   ./bench.sh            # 1e6 hot iterations, 2e4 io iterations
#   BENCH_ITERS=... BENCH_IO_ITERS=... ./bench.sh
#
# Writes results/primitives.txt and prints it. Every overhead number in
# TESTING.md is read against these lines: without them, the clock read the
# measurement itself pays for would be indistinguishable from the layer's work.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"
res="$here/results"
mkdir -p "$res"

"$here/build.sh" > "$res/bench.build.log" 2>&1
build_id=$(grep -o 'build_id=.*' "$res/bench.build.log" | cut -d= -f2)
so_sha=$(sha256sum "$here/target/release/liblapsphere_frames_layer.so" | cut -d' ' -f1)
iters=${BENCH_ITERS:-1000000}
io=${BENCH_IO_ITERS:-20000}

{
    echo "=== primitive_bench (out-of-band floors)"
    echo "date            $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    echo "git HEAD        $(git -C "$here" rev-parse HEAD)"
    echo "dirty paths     $(git -C "$here" status --porcelain -- . | wc -l) under spike/vk-layer"
    echo "build_id        $build_id"
    echo "so_sha256       $so_sha"
    echo "cpu             $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ //')"
    echo "nproc           $(nproc)"
    echo "loadavg         $(cut -d' ' -f1-3 /proc/loadavg)"
    echo "governor        $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null || echo n/a)"
    echo "cmd             primitive_bench --iters $iters --io-iters $io"
    echo ""
    "$here/target/release/primitive_bench" --iters "$iters" --io-iters "$io"
} | tee "$res/primitives.txt"
