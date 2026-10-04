# STATUS — Vulkan implicit layer spike: **NOT WORKING**

Last updated: 2026-10-03. Read this before reading the code: nothing here has
produced a measurement yet.

## One-line state

**Progress: the segment is now created. No frames are recorded yet.**

`vkCreateInstance` completes, `frames-<pid>` appears in
`$XDG_RUNTIME_DIR/lapsphere/` with mode 0600 and the right size — but it holds
`frame_count = 0`, and the counters show the device/present path is never
reached.

## Exact counters from the last run

```
enabled; segment created gipa=0 create_instance=1 create_device=0 gdpa=0
  destroy_device=0 create_swapchain=0 destroy_swapchain=0 queue_present=0
```

Read literally, that says:

* `vk_create_instance` ran once and succeeded;
* **our `vkGetInstanceProcAddr` was never called** (`gipa=0`) — not even for
  the names the loader must ask about;
* consequently `vk_create_device` was never entered, so no device state was
  registered, so `vkQueuePresentKHR` was never intercepted.

**The layer is loaded, the instance is created, and then the loader never
routes anything else through it.** That is the whole remaining problem, and it
is a loader-interface question, not a frame-timing one.

## What was fixed in this round (each confirmed by a printed value)

* **Owner hypothesis (в) was correct and it was the blocker.** The
  `pfnNextGetInstanceProcAddr` in the *first* `VkLayerInstanceLink` belongs to
  `libVkLayer_MESA_device_select.so`, and calling *that* function with a real
  instance handle **never returns** on this host — the main thread spins in
  user space with no syscalls (`/proc/<pid>/stat` shows `state=R`, `utime`
  climbing; `strace` shows nothing after the last log write). Walking the link
  chain to its **last** entry and using that gipa — which belongs to
  `libvulkan.so.1.4.341`, the loader terminator — fixed it. The lookups now
  return, `cd.is_some()=true di.is_some()=true`, and the segment is created.
* The full link chain is now logged with the object each gipa belongs to:

  | link | gipa belongs to |
  | --- | --- |
  | `link[0]` | `libVkLayer_MESA_device_select.so` |
  | `link[1]` | `libMangoHud.so` |
  | `link[2]` | `libvulkan.so.1.4.341` (**terminator — use this one**) |

* MangoHud is **not** implicated: the same hang occurs with `MANGOHUD=0`.
* `dlsym(RTLD_DEFAULT, "vkGetInstanceProcAddr")` returns **NULL** inside the
  layer on this host, so the terminator route is the only working one.
* Build-identity stamping is in (`build.sh` writes `src/buildid.rs`;
  `negotiate` prints the stamp, the source mtime and its own pid). Run
  `run.sh`, which also prints the mapped `.so` from `/proc/<pid>/maps`. This
  exists because a stale build was twice mistaken for a behavioural result.

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

## What is now EXCLUDED (do not re-test these)

* **MangoHud interference** — same hang with `MANGOHUD=0`.
* **The first link's gipa being usable** — it belongs to
  `libVkLayer_MESA_device_select.so` and loops on a real instance handle here.
  Use the terminator (`link[2]`).
* **`VK_LAYER_PATH`** — does not install an implicit layer at all.
* **A crash in our code** — there is none; the main thread spins, it does not
  fault, and there is no `catch_unwind` trigger.
* **Stale build** — `build.sh` stamps the id into the binary and `run.sh`
  prints the mapped `.so`; every result above is from a stamped build.

## Code reference needed to finish

The remaining blocker is a **loader-interface** question, not a Vulkan-timing
one, and it is not answerable by more of my own guessing. The most useful
thing the owner could bring, in order of usefulness:

1. **Any working implicit layer for Vulkan 1.3/1.4 on this loader**, as source
   or as something to read — specifically its `vkCreateInstance` /
   `vkCreateDevice` / `vkGetDeviceProcAddr` and its manifest. I want to see
   how a layer that *is* routed after instance creation is written. Candidates:
   `VK_LAYER_MESA_device_select` itself is on this host
   (`/usr/lib/x86_64-linux-gnu/libVkLayer_MESA_device_select.so`) and is the
   layer whose gipa I am calling — its own source
   (`src/amd/vulkan/layers/device_select`) is the closest reference available
   without downloading anything, and it is the layer I can *remove* from the
   picture by setting `NODEVICE_SELECT=1`.
2. **The loader's own rules for a layer's manifest `functions` map and the
   deprecated-vs-negotiated `vkGetInstanceProcAddr` tag.** The loader logged a
   deprecation warning for our manifest's `vkGetInstanceProcAddr` /
   `vkGetDeviceProcAddr` tags. A manifest that drops those tags and relies
   solely on `vkNegotiateLoaderLayerInterfaceVersion` is the obvious next
   experiment — it is cheap and I did not get to it before the time-box.
3. **Confirmation of which concrete next step is right**: with `gipa=0`, the
   loader appears to be resolving device entry points from the *manifest*
   (`functions`) rather than by calling our gipa. If so the fix is a manifest
   problem — either correct `functions` entries, or dropping them entirely and
   letting negotiation supply the pointers. I would test (2) first.

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
