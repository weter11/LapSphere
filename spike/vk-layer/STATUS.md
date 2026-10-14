# STATUS — Vulkan frames layer, stage 1: **WORKING** (presents are intercepted)

Last updated: 2026-10-14, branch `spike/vk-layer-stage1`, base
`origin/lapsphere` = `22c14a182b319ed33be9466951162898c526101d`.

Round 1 of this spike (PR #111, branch `feat/mini-panel`, its own
`spike/vk-layer/STATUS.md`) ended at "the segment is created but `frame_count`
stays 0; the loader never routes anything through the layer after instance
creation". **That is now closed, and the round-1 diagnosis was wrong in a way
worth recording**: the blocker was in the layer's use of the loader's dispatch
chain, and one of the three "facts" it rested on was a measurement taken at a
point where only zero is possible.

## One-line state

`vkQueuePresentKHR` is intercepted on a real Vulkan application: 900 presents
counted for `vkcube --c 900`, **899** intervals written into the shared-memory
ring (the first present of a cadence only establishes the origin — the layer's
final line reads `writer_final: intervals=899 dropped=0 presents=900`), read
back by an independent second process, and the application exits 0.

## The exact counters from the recorded run

```
destroy_instance: GetInstanceProcAddr=104 CreateInstance=1 CreateDevice=1
  GetDeviceProcAddr=631 DestroyDevice=1 CreateSwapchainKHR=1
  DestroySwapchainKHR=1 QueuePresentKHR=900 DestroyInstance=1
  GetPhysicalDeviceProcAddr=0 GetDeviceQueue=1 GetDeviceQueue2=0
writer_final: intervals=899 dropped=0 presents=900
```

The last line is emitted just before the segment is unlinked (nothing can read
it afterwards), and `verify.sh` asserts `intervals == presents - 1` rather than
leaving the relation as an inference.

With the layer unset, the same application runs unchanged and creates no
segment (`verify.sh` check C3).

## Why round 1 stalled — three causes, all in the layer

The loader source is the authority here: `Vulkan-Loader`
`vulkan-sdk-1.4.341.0` (the version this host runs,
`/usr/lib/x86_64-linux-gnu/libvulkan.so.1.4.341`), `loader/loader.c`.

1. **The counters were printed too early to mean anything.** Round 1 printed
   them at the end of `vkCreateInstance`. The loader calls the layer's
   `vkGetInstanceProcAddr` at `loader.c:5283`
   (`loader_init_instance_core_dispatch_table`) — *after* `vkCreateInstance`
   returns — and again at `:5504`
   (`nextGIPA(inst->instance, "vkCreateDevice")`) while building the device
   chain. So `gipa=0 create_device=0` was true by construction at the moment it
   was sampled, and the conclusion drawn from it ("the loader never routes
   anything through the layer") did not follow. Counters in this round are
   printed at `vkDestroyDevice`, at `vkDestroyInstance`, and every 600 presents.
2. **The chain link was never consumed, and that is what hung the app.** The
   loader creates **one** `VkLayerInstanceCreateInfo` node
   (`function == VK_LAYER_LINK_INFO`, `loader.c:5004-5009`) and threads the
   layers through it as a LIFO chain: `layer_instance_link_info[i].pNext =
   chain_info.u.pLayerInfo`, then `chain_info.u.pLayerInfo =
   &layer_instance_link_info[i]` (`:5113-5128`). The head is therefore *this*
   layer's link, and a layer must consume it
   (`u.pLayerInfo = u.pLayerInfo->pNext`) before calling down. Round 1 did not,
   so `VK_LAYER_MESA_device_select` saw its own link as "next" and called
   itself with a real instance handle — a user-space spin with no syscalls,
   which is exactly what round 1 observed and attributed to MESA. The device
   chain works the same way (`:5480-5483`).
3. **"Next" must be the link's gipa, not the loader terminator's.** Round 1
   walked the link chain to its last entry (`libvulkan.so`) and used *that* as
   "next". The terminator returns bottom-of-chain (ICD-level) functions, so
   everything routed through the layer afterwards bypassed the layer below it
   and the application stopped making progress. With the link consumed, the
   link's own `pfnNextGetInstanceProcAddr` works — it is the documented route.

**The manifest `functions` map was a red herring.** `loader.c:5043-5110`
consults it only when negotiation did not supply `vkGetInstanceProcAddr`; the
two deprecation warnings it prints are cosmetic. Both manifest variants are run
by `verify.sh` (checks C4) and intercept 900 presents each —
`manifests/VK_LAYER_LAPSPHERE_frames.functions.json` exists so that this is
tested rather than argued.

**One real bug in the round-1 reader, found because it was now exercised:** the
ring index was computed as `cap - n` (the last `n` slots) instead of
`frame_count - cap` (slot 0 until the ring has wrapped). On the first live
reader run every interval read back was `0.000 ms` — the reader was reading
slots the writer had not reached. Fixed in `src/bin/frames_reader.rs`; the
harness check C5 covers the wrapped case too.

## What is proven, and by what

| Claim | Evidence |
| --- | --- |
| Real present interception | `QueuePresentKHR=900` for `vkcube --c 900`, exit 0 |
| `frame_count` increases | reader samples 3 s apart: 45 → 286, and 5965 in the 50 s wrap run |
| Intervals are recorded | reader dumped 285→4096 intervals, p50 8.334 ms (120 fps), zero zero-slots; final count 899 for 900 presents, asserted by the harness |
| An independent process reads them | `frames_reader` is a separate binary/process over the same shm segment |
| Inert when not enabled | no segment, no layer log, loader never activates it, app exit 0 |
| Works across both WSIs and both GPUs | xlib and Wayland WSI, iGPU (RADV RENOIR) and dGPU (RTX 3070), 300 presents each |
| Coexists with MangoHud | `MANGOHUD=1` on this host: MangoHud is below this layer in the chain and presents still flow |

Commands and the full acceptance table: `TESTING.md`. Raw material from the
recorded run: `results/` (untracked; `verify.sh` regenerates it).

## What is NOT proven (do not read the above as more than it is)

* **No overhead measurement.** `LAPSPHERE_FRAMES_OVERHEAD` and the reader's
  `--overhead` path exist but were not measured in this stage; the present hook
  takes a mutex and two clock reads, and no number in this document is an
  overhead number.
* **No agreement measurement against MangoHud** (that is a later stage).
* **No DXVK/Proton/Wine, no OpenGL, no 32-bit, no anti-cheat, no GPU-bound
  title** — the only application exercised is `vkcube`.
* **No Wayland-native *claim*:** the layer contains no WSI code at all, and the
  Wayland runs here went through `vkcube`'s Wayland WSI on this host's
  `xfwl4` + Xwayland session. Wayland presentation *timing* was not studied.
* **One device, one queue, one swapchain per process** is the tested shape.
  Two devices in one process, or two games at once, are untested; the state
  tables are keyed by handle and a second device would be handled, but that is
  an argument, not a measurement.
* **Nothing was changed outside `spike/vk-layer/`.** No GUI, panel, layout,
  settings, daemon or FPS-in-UI work; the diff is confined to this directory.

## Layout

| Path | Purpose |
| --- | --- |
| `src/vkraw.rs` | raw Vulkan + loader declarations, `#[repr(C)]` with compile-time offset/size assertions against `vulkan-sdk-1.4.341.0` `vk_layer.h` |
| `src/lib.rs` | negotiation, chain handling, instance/device dispatch, the `vkQueuePresentKHR` hook; the header comment lists the three round-1 causes with loader line numbers |
| `src/shm.rs` | shm protocol v1 writer (128-byte header, 4096-slot `u64` ns ring, seqlock) — unchanged from round 1 |
| `src/bin/frames_reader.rs` | independent reader (`--list`, `--dump`, `--watch`, `--overhead`, `--prune`); ring indexing fixed |
| `manifests/` | implicit-layer manifest, plain and with the legacy `functions` map (the A/B) |
| `build.sh` | builds with a build id (git HEAD + source hash + dirty count) compiled in |
| `install.sh` | installs the manifest into `~/.local/share/vulkan/implicit_layer.d/`, no root |
| `run.sh` | one instrumented run, printing the mapped `.so` and its hash |
| `verify.sh` | the acceptance harness: 22 checks, `VERIFY_LONG=1` adds the ring-wrap run |
| `TESTING.md` | reproduction recipe, expected output, results table, limitations |

Own `[workspace]`, so `cargo build --all` / `cargo test` at the repository root
do not pick it up (verified: the root `Cargo.toml` members are unchanged).

## Next, when this is taken further

Whatever comes next starts from `verify.sh`: overhead (median/p99 in µs, A/B
against `LAPSPHERE_FRAMES` unset), MangoHud agreement, robustness (kill -9,
resize, two instances, no reader), and only after that any question of what the
panel does with the numbers. The layer still has to be *installed* on a host;
whether the product ships an implicit layer at all remains the owner's open
question (round-1 `panel-design.md` Q13), and this stage does not decide it.
