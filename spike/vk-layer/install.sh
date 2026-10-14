#!/bin/sh
# Install the layer manifest into the Vulkan loader's *implicit* layer search
# path, user-writable and root-free:
#
#   ~/.local/share/vulkan/implicit_layer.d/
#
# Note: `VK_LAYER_PATH` is NOT enough for an implicit layer. Measured on this
# host with VK_LOADER_DEBUG=layer: the loader lists a manifest found there
# under "Searching for explicit layer manifest files" and never activates it,
# because `enable_environment` is only honoured for implicit layers. That is
# exactly the practical difference between the implicit and explicit routes
# in panel-design.md Q13, and it is the reason this script installs into the
# real implicit search directory instead.
set -e
here=$(cd "$(dirname "$0")" && pwd)
dest="$HOME/.local/share/vulkan/implicit_layer.d"
mkdir -p "$dest"
sed "s|\"./liblapsphere_frames_layer.so\"|\"$here/target/release/liblapsphere_frames_layer.so\"|" \
  "$here/VK_LAYER_LAPSPHERE_frames.json" > "$dest/VK_LAYER_LAPSPHERE_frames.json"
echo "installed: $dest/VK_LAYER_LAPSPHERE_frames.json"
echo "activate with: LAPSPHERE_FRAMES=1 <app>"
echo "remove with:   rm $dest/VK_LAYER_LAPSPHERE_frames.json"