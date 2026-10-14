#!/bin/sh
# Install the layer manifest into the Vulkan loader's *implicit* layer search
# path, which is user-writable and needs no root:
#
#   ~/.local/share/vulkan/implicit_layer.d/VK_LAYER_LAPSPHERE_frames.json
#
# `VK_LAYER_PATH` is NOT enough for an implicit layer: measured in round 1 with
# VK_LOADER_DEBUG=layer, the loader lists a manifest found there under
# "Searching for **explicit** layer manifest files" and never activates it,
# because `enable_environment` is only honoured for implicit layers.
#
# The absolute path of the built .so is substituted for the manifest's
# "./liblapsphere_frames_layer.so".
#
#   ./install.sh              -> manifests/VK_LAYER_LAPSPHERE_frames.json
#   ./install.sh functions    -> the control variant WITH the legacy functions
#                                map (the round-1 manifest shape)
#   ./install.sh none         -> remove the manifest
set -e
here=$(cd "$(dirname "$0")" && pwd)
variant=${1:-plain}
dest="$HOME/.local/share/vulkan/implicit_layer.d/VK_LAYER_LAPSPHERE_frames.json"

case "$variant" in
    none|remove)
        rm -f "$dest"
        echo "removed: $dest"
        exit 0
        ;;
    plain)
        src="$here/manifests/VK_LAYER_LAPSPHERE_frames.json"
        ;;
    functions)
        src="$here/manifests/VK_LAYER_LAPSPHERE_frames.functions.json"
        ;;
    *)
        echo "usage: $0 [plain|functions|none]" >&2
        exit 2
        ;;
esac

so="$here/target/release/liblapsphere_frames_layer.so"
[ -f "$so" ] || { echo "build first: ./build.sh" >&2; exit 1; }

mkdir -p "$(dirname "$dest")"
sed "s|\"./liblapsphere_frames_layer.so\"|\"$so\"|" "$src" > "$dest"
echo "installed ($variant): $dest"
echo "  library_path -> $so"
echo "activate with: LAPSPHERE_FRAMES=1 <app>"
