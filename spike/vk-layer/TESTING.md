# TESTING — how every number in `STATUS.md` was produced

Everything here was run on this host, on a binary whose build id is printed by
the layer itself. Nothing is a demonstration value: each figure is read back
out of a live shared-memory segment, by a second process, or out of the
application's own exit status.

## Environment (recorded, because the results are conditional on it)

| | |
| --- | --- |
| Loader | `/usr/lib/x86_64-linux-gnu/libvulkan.so.1.4.341` (`libvulkan1 1.4.341.0-1`) |
| Layers in the chain | `VK_LAYER_LAPSPHERE_frames` (this), `VK_LAYER_MESA_device_select`, `VK_LAYER_MANGOHUD_overlay_x86_64` (`MANGOHUD=1` is set on this host) |
| ICDs | `RADV RENOIR` (AMD iGPU), `NVIDIA GeForce RTX 3070 Laptop GPU`, `llvmpipe` |
| Application | `vkcube` from `vulkan-tools 1.4.341.0+dfsg1-1` |
| Session | `xfwl4` (Xfce Wayland) + Xwayland; `vkcube` was run with `--wsi xlib` and with its default Wayland WSI |
| Layer source of truth for the ABI | `Vulkan-Headers` `vulkan-sdk-1.4.341.0` `include/vulkan/vk_layer.h`; behaviour from `Vulkan-Loader` same tag `loader/loader.c` |

Two of these matter for reading the results: the GPU indices `vkcube` reports
are not stable between runs (physical-device sorting), and MangoHud is in the
chain below the layer — so every run here is also an unplanned coexistence test.

## Commands

```sh
cd spike/vk-layer

./build.sh                      # build, with the build id compiled in
./install.sh plain              # implicit-layer manifest (no legacy functions map)
./install.sh functions          # the control manifest (with the functions map)
./install.sh none               # remove the manifest again

./run.sh 60                     # one instrumented run + mapped .so + layer log
LAPSPHERE_FRAMES=1 vkcube --gpu_number 0 --wsi xlib --c 900      # manual

./verify.sh [gpu_index]         # the acceptance harness (22 checks, ~35 s)
VERIFY_LONG=1 ./verify.sh 0     # + the ~50 s ring-wraparound run

./target/release/frames_reader --list
./target/release/frames_reader --dump /tmp/frames.csv
./target/release/frames_reader --watch --hz 60 --seconds 10 --cpu
```

`build.sh` prints the id it compiled in and the SHA-256 of the artifact, for
example:

```
build_id=0b427ef7e79b/718336e1a120
so_sha256=41a441bbfd7b4f3b2d34f8b6a0349daa6e0b984e98a94708a392dcf4db2ff1b2
```

`build_id` is `<git HEAD short>/<sha256 of src/{lib,shm,vkraw}.rs>[+dirty<N>]`.
The `dirty` suffix is deliberate: a companion result must never be quoted
without saying whether the tree it came from was clean.

## What the output proves

The layer's own log (`$XDG_RUNTIME_DIR/lapsphere/spike.log`) for the recorded
`vkcube --c 900` run, verbatim and abridged:

```
negotiate: loader offered v2, answering v2, build 0b427ef7e79b/718336e1a120
  mtime 2026-10-14T08:57:29 so=.../target/release/liblapsphere_frames_layer.so
create_instance: chain=0x7fff1242c800 link=0x7fff1242c770
  next_gipa=0x7895c7c47560 (libVkLayer_MESA_device_select.so offset+0x2560)
  next_gpdpa=0x7895d3195080 link.pNext=0x7fff1242c758 enabled=true
gipa[0](instance=..., "vkCreateInstance") -> ours (name string in libvulkan.so.1.4.341)
gipa[1..63](instance=..., "vkDestroyInstance" | "vkEnumeratePhysicalDevices" | ...) -> ...
create_device: chain=... link=... next_gipa=... next_gdpa=0x7895b905dae0
  (libMangoHud.so offset+0x3eae0) instance=0x635cff068650
create_device: device=0x635cff51b990 next_present=0x7895b90580d0
  (libMangoHud.so offset+0x390d0) next_queue=... next_swapchain=...
get_device_queue: queue 0x635cff73b560 -> device 0x635cff51b990
create_swapchain: device 0x635cff51b990 -> swapchain 109251075235840
destroy_device: ... QueuePresentKHR=900 ...
destroy_instance: ... QueuePresentKHR=900 DestroyInstance=1
writer_final: intervals=899 dropped=0 presents=900
```

Read it as: the loader calls **our** `vkGetInstanceProcAddr` 64 times while
building the instance dispatch (that is the thing round 1 concluded never
happened), then our `vkCreateDevice`, then our `vkGetDeviceQueue`, then our
`vkCreateSwapchainKHR`, and finally our `vkQueuePresentKHR` once per frame.

## The acceptance harness

`./verify.sh 0` prints one PASS/FAIL line per check with the value it read. The
recorded run (`VERIFY_LONG=1`, 2026-10-14T10:57:41Z, code commit
`0b427ef7e79b47ec0970da963966476ca5112fc5`, 0 dirty paths under
`spike/vk-layer`, build id `0b427ef7e79b/718336e1a120`):

| Check | Result |
| --- | --- |
| `build` | PASS — `build_id=0b427ef7e79b/718336e1a120` (clean tree), exit 0 |
| `C1.app_exit0` | PASS — `vkcube` exit 0 |
| `C1.present_intercepted` | PASS — `QueuePresentKHR=900` (log counter) |
| `C1.segment_during_run` | PASS — 1 segment file while the app ran |
| `C2.frame_count_known_to_reader` | PASS — reader read `frame_count=45` |
| `C2.intervals_are_presents_minus_one` | PASS — `writer_final: intervals=899 presents=900` |
| `C2.frame_count_increases` | PASS — 45 → 286 across two samples 3 s apart |
| `C2.segment_gone_after_exit` | PASS — 0 left (unlinked at `vkDestroyInstance`) |
| `C2.intervals_recorded` | PASS — 285 intervals dumped by the reader |
| `C2.intervals_plausible` | PASS — `n=285 min=0.688ms p50=8.334ms p99=8.646ms max=11.771ms fps_p50=120.0` |
| `C2.reader_is_a_separate_process` | PASS — second process, same segment |
| `C3.app_exit0_unset` | PASS — exit 0 with `LAPSPHERE_FRAMES` unset |
| `C3.no_segment` | PASS — 0 segments during and after |
| `C3.no_layer_log` | PASS — the layer never ran |
| `C3.loader_did_not_activate` | PASS — the loader log shows no enabled lapsphere layer |
| `C3b.app_exit0_disable_env` | PASS — exit 0 with `LAPSPHERE_FRAMES_DISABLE=1` |
| `C3b.no_segment` | PASS — 0 segments |
| `C4.app_exit0_functions_manifest` | PASS — exit 0 with the legacy `functions` map installed |
| `C4.present_intercepted_too` | PASS — `QueuePresentKHR=900` there as well |
| `C5.ring_full` | PASS — 4096 intervals returned once the ring wrapped |
| `C5.no_zero_slots` | PASS — 0 zero-valued slots |
| `C5.still_plausible` | PASS — p50 8.334 ms after wraparound (5965 frames recorded) |

Raw material, regenerated by every harness run and **not** committed:
`results/verify.txt`, `results/*.app.out`, `results/*.layer.log`,
`results/*.reader.list`, `results/*.frames.csv`.

## Other runs (same build)

| Configuration | Result |
| --- | --- |
| `vkcube --gpu_number 1 --wsi xlib --c 300` (AMD iGPU) | exit 0, `QueuePresentKHR=300`, 1 swapchain |
| `vkcube --gpu_number 0 --c 300` (default Wayland WSI, AMD iGPU) | exit 0, `QueuePresentKHR=300`, 2 swapchains |
| `vkcube --gpu_number 1 --c 300` (Wayland WSI, NVIDIA) | exit 0, `QueuePresentKHR=300`, 2 swapchains |
| `vkcube --wsi xlib` for 50 s, no `--c` | 5965 frames recorded, reader returned the full 4096-slot ring with 0 zero slots |
| `vkcube --c 5200` | `QueuePresentKHR=5200`; the periodic log lines reach `present: 4800`, i.e. past the ring capacity |

The two-swapchain Wayland runs are the reason `vkCreateSwapchainKHR` resets the
present origin: the gap across a swapchain recreation is not a frame time, and
without the reset the first interval after it would appear as a spike.

## Two conventions a later stage must not silently change

* **The timestamp is taken at entry to `vkQueuePresentKHR`**, so an interval is
  present-to-present as the application issues it. Round 1 timed after the call
  down returned; the numbers from the two conventions are not comparable.
* **`frame_count` counts intervals, i.e. presents − 1.** The first present of a
  cadence establishes the origin and writes no interval; `last_present_ns` is
  what proves the writer is alive at that moment. So a finished `vkcube --c 900`
  run reads `intervals=899` for `presents=900` — measured, not derived: the
  layer prints `writer_final: intervals=… dropped=… presents=…` immediately
  before it unlinks the segment (the only moment at which it can still be read),
  and check `C2.intervals_are_presents_minus_one` asserts the relation. The
  reader's `--list` prints the header's `frame_count`, which is the same number.

## Reproducing the round-1 failure, if it is ever in doubt

Install the round-1 manifest shape (`./install.sh functions`) is *not* the same
thing — that map is harmless. The failure is reproducible only from the round-1
code, on branch `feat/mini-panel` (PR #111), which is untouched by this branch:
its `lib.rs` never consumes the link and resolves "next" from the terminator.
The two-line difference is documented in this crate's `src/lib.rs` header, with
the loader line numbers that make the requirement explicit.

## Not tested

See `STATUS.md` § "What is NOT proven". In short: no overhead number exists yet,
no MangoHud agreement, no DXVK/Proton/Wine, no OpenGL, no 32-bit, no
anti-cheat, no GPU-bound title, no two-swapchain-per-process case, and no
Wayland presentation-timing claim. The harness touches only `spike/vk-layer/`
and `$XDG_RUNTIME_DIR/lapsphere/`; it does not start, stop or configure any
LapSphere service.
