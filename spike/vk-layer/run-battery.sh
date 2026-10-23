#!/bin/sh
# Stage 1b: run every experiment once, in order, on the committed tree.
# Serial on purpose: M1/M3 (overhead), the agreement runs and the reader-cost
# runs are all timing measurements and must not compete with each other.
cd /home/wer/devis/lapsphere/repo/spike/vk-layer || exit 1
R=results
mkdir -p "$R"
{
  echo "=== stage 1b battery"
  echo "start        $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "git HEAD     $(git rev-parse HEAD)"
  echo "dirty paths  $(git status --porcelain -- . | wc -l)"
  echo "loadavg      $(cut -d' ' -f1-3 /proc/loadavg)"
  echo "so_sha256    $(sha256sum target/release/liblapsphere_frames_layer.so | cut -d' ' -f1)"
} > "$R/battery-header.txt"
cat "$R/battery-header.txt"

./bench.sh        > "$R/stage1b-bench.txt"        2>&1
echo "bench done   $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
./verify.sh 0     > "$R/stage1b-verify.txt"       2>&1
echo "verify done  $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
./overhead.sh 3   > "$R/stage1b-overhead.txt"     2>&1
echo "overhead done $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
./agreement.sh 2  > "$R/stage1b-agreement.txt"    2>&1
echo "agree done   $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
./robustness.sh 2 > "$R/stage1b-robustness.txt"   2>&1
echo "robust done  $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
VERIFY_LONG=1 ./verify.sh 0 > "$R/stage1b-verify-long.txt" 2>&1
echo "verify-long done $(date -u '+%H:%M:%SZ') loadavg $(cut -d' ' -f1-3 /proc/loadavg)"
echo "=== battery end $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
