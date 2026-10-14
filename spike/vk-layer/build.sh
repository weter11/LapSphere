#!/bin/sh
# Build the layer with a build identity compiled into it, then print that
# identity and the SHA-256 of the artifact.
#
# Why: the manifest points at the .so by absolute path, so "cargo build
# finished" does not imply "this is the code that ran". Round 1 twice mistook a
# stale build for a behavioural result. The identity is derived from the git
# HEAD plus a hash of the sources, so a result can always be tied back to a
# binary and to a commit -- and a dirty tree is visible in the id itself.
set -e
here=$(cd "$(dirname "$0")" && pwd)
cd "$here"

stamp=$(cat src/lib.rs src/shm.rs src/vkraw.rs | sha256sum | cut -c1-12)
mtime=$(date -r src/lib.rs '+%Y-%m-%dT%H:%M:%S')
head=$(git -C "$here" rev-parse --short=12 HEAD 2>/dev/null || echo nogit)
dirty=$(git -C "$here" status --porcelain -- . 2>/dev/null | wc -l)
suffix=""
[ "$dirty" -gt 0 ] && suffix="+dirty$dirty"

id="$head/$stamp$suffix"

LAPSPHERE_BUILD_ID="$id" \
LAPSPHERE_BUILD_MTIME="$mtime" \
LAPSPHERE_GIT_HEAD="$head" \
  cargo build --release "$@"

so="$here/target/release/liblapsphere_frames_layer.so"
echo "build_id=$id"
echo "source_mtime=$mtime"
echo -n "so_sha256="
sha256sum "$so" | cut -d' ' -f1
ls -l --time-style=+%Y-%m-%dT%H:%M:%S "$so"
