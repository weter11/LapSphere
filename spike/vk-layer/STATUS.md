# STATUS — Vulkan implicit layer spike: **NOT WORKING**

Last updated: 2026-10-03. Read this before reading the code: nothing here has
produced a measurement yet.

## One-line state

The layer **compiles in release**, the loader **enumerates and loads it**, and
`vkCreateInstance` is entered — but **no shared-memory segment is ever created**
and **no frame is ever recorded**. Debugging is in progress.

## What is measured

| # | Test | Method | Result |
| --- | --- | --- | --- |
| 1a | Layer is found by the loader | manifest installed to `~/.local/share/vulkan/implicit_layer.d/`, `VK_LOADER_DEBUG=layer`, `vkcube --gpu_number 0 --c 30` | **PASS** — manifest listed in the implicit-layer search path |
| 1b | Inert without the environment variable | same run with `LAPSPHERE_FRAMES` unset | **PASS** — manifest found, **no** `frames-<pid>` segment, game unaffected |
| — | `VK_LAYER_PATH` as an install location | manifest placed there instead | **FAIL, and the finding matters** — the loader lists it under *"Searching for **explicit** layer manifest files"* and never honours `enable_environment`, which is only read for implicit layers. Install into a real implicit search directory, or use `VK_IMPLICIT_LAYER_PATH`. |

That is all. Tests 1c, 2, 3, 4, 5 and 6 of the agreed list have **no result**:
no MangoHud comparison, no per-hook overhead, no robustness matrix, no reader
latency/CPU, no `frametime_stats.py --compare` A/B.

## Where it stops, exactly

Observed output of a run (release build, `LAPSPHERE_FRAMES=1`):

```
[lapsphere-frames] negotiate: loader offered 2
[lapsphere-frames] vkCreateInstance entered
[lapsphere-frames] next_gipa present: true
```

and then nothing: no `cd.is_some()`/`di.is_some()` line, no segment, and the
process runs the cube normally until the timeout kills it. So the failure is
after the instance chain is successfully walked and inside
`vk_create_instance`, between retrieving `next_gipa` and installing the
instance state.

## Bugs already found and fixed (do not re-introduce)

Each was confirmed by a value printed at the failure point.

1. **`VkLayerInstanceCreateInfo` has no `VkLayerFunction` struct.** The real
   layout (`/usr/include/vulkan/vk_layer.h`) is
   `{ sType, pNext, VkLayerFunction *function /* an enum */, union u }`, where
   `u.pLayerInfo` is a **pointer** to a `VkLayerInstanceLink` whose *first*
   member is `pfnNextGetInstanceProcAddr`. Reading it as a struct of two
   function pointers gave a call through address `0x3` and a segfault in
   `vkCreateInstance` (confirmed with `gdb -batch -ex run -ex bt`).
2. **The link-info node is not first in `p_next`.** The loader also puts a
   `VK_LOADER_FEATURES` node (`function == 3`) in the chain; a walk that
   breaks on the first matching `sType` reads `function=3, pLayerInfo=0x0` and
   wrongly concludes there is no next layer. Fixed: keep walking past
   non-link members. **FIXED but not yet re-verified end to end.**
3. **Manifest `functions` values are C symbol names**, not Rust paths.
   `"vkQueuePresentKHR": "lapsphere_frames_layer::vk_queue_present_khr"` does
   not resolve.
4. **`VK_LAYER_PATH` does not install an implicit layer** (see the table).

## Process notes that cost time — apply them

* **Stale builds were mistaken for behaviour twice.** The manifest references
  the `.so` by absolute path, and `cargo build` can finish while the loaded
  library is the previous one. **Always print the `.so` build time/hash from
  inside the layer (in `vkNegotiateLoaderLayerInterfaceVersion`) and compare it
  against `/proc/<pid>/maps`.** Not yet implemented — it is the first thing to
  add.
* **A quiet failure is not a result.** Both #1 and #2 produced a clean-looking
  run; only the printed values distinguished them from success.
* `ash` was tried and **dropped**: it does not bind the loader structures and
  its 0.38 API diverges from what a layer needs. `src/vkraw.rs` is
  hand-written `#[repr(C)]` with compile-time offset assertions instead.
  Dependency list is `libc` only.

## Layout

| Path | Purpose |
| --- | --- |
| `src/vkraw.rs` | minimal raw Vulkan + loader declarations, static layout assertions |
| `src/shm.rs` | shm protocol v1 writer: 128-byte header, 4096-slot `u64` ns ring, seqlock, overhead region |
| `src/lib.rs` | negotiation, instance/device dispatch chain, hooks |
| `src/bin/frames_reader.rs` | separate-process reader: `--list`, `--dump`, `--watch --hz --seconds --cpu`, `--overhead`, `--prune` |
| `VK_LAYER_LAPSPHERE_frames.json` | manifest with `enable_environment: LAPSPHERE_FRAMES=1`, `disable_environment: LAPSPHERE_FRAMES_DISABLE` |
| `install.sh` | installs the manifest into `~/.local/share/vulkan/implicit_layer.d/` (no root) |

This crate has its own `[workspace]`, so `cargo build --all` at the
repository root does not include it.

## Not proposed production code

It is committed so that every number in the eventual spike document can be
reproduced. It is not part of the panel work and is not proposed for merge as
shipped code.
