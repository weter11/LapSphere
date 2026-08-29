# NVIDIA crate archaeology

## Scope and evidence

This is a read-only inventory of `nvidia/driver.rs`, `nvidia/nvapi.rs`, and
`nvidia/nvidia.rs`. Claims are `[verified]` when traced in those files or their
current daemon counterpart; `[assumed]` marks an interpretation. No wiring or
fixes are proposed.

## What the code implements

`driver.rs` is a low-level NVIDIA Resource Manager client. `[verified]` It
opens `/dev/nvidiactl` and `/dev/nvidia<minor>`, allocates client/device/
subdevice handles, and issues `NV_ESC_RM_ALLOC`, `NV_ESC_REGISTER_FD`, and
`NV_ESC_RM_CONTROL` ioctls. The exposed queries cover:

- GPU and VRAM thermal sensors, including sensor values indexed for hotspot
  and memory/VRAM;
- core voltage;
- ROP count/factor/operations and streaming-multiprocessor count;
- VRAM type, memory vendor, bus width, and L2 cache size.

`nvapi.rs` dynamically loads `libnvidia-api.so.1`, resolves
`nvapi_QueryInterface`, initializes/unloads NVAPI, enumerates physical GPUs,
matches a GPU by PCI bus ID, and calls two undocumented interfaces for
thermal sensors and voltage. `[verified]` The thermal record decodes index 9
as hotspot and index 15 as VRAM, dividing the fixed-point value by 256 and
rejecting values outside 0..255°C. `[verified]` It also probes a supported
thermal mask rather than assuming every bit is valid. `[verified]`

`nvidia.rs` is a substantially broader GPU-controller implementation, not
just a sensor shim. `[verified]` It combines NVML with the RM handle and
Vulkan/OpenCL helpers to provide device metadata, PCI/link information,
architecture, CUDA cores, VRAM metadata, power states, clocks and offsets,
power limits, fan readings/control (static and curve), throttle reasons,
process lists/utilization, and cleanup/reset behavior. `[verified]` The RM
path supplies the fields NVML does not expose reliably, especially VRAM
type/vendor/bus width, L2/ROP/SM metadata, hotspot/VRAM temperatures, and
voltage. `[verified]`

## Completeness and standalone status

The implementation is internally substantial but is not a standalone crate in
this repository. `[verified]` There is no `nvidia/Cargo.toml`, it is absent
from the workspace, and `nvidia.rs` imports `super` controller traits/types,
`crate` modules and globals, generated NVIDIA bindings, and `lact_schema`.
`[verified]` Consequently it cannot be compiled or consumed merely by adding
the directory; it requires the surrounding LACT-style controller framework,
bindings, and dependency set. `[assumed]` Within that expected host, the
controller has complete read/control paths with graceful optional RM-handle
fallbacks, but its success depends on NVIDIA device nodes, permissions,
driver ABI compatibility, and NVML availability. `[verified]`

## Does it duplicate daemon NVIDIA logic?

**Yes for the deciding sensor/metadata question; it does not reveal a missing
daemon capability.** `[verified]` The daemon already contains an inline RM
client for VRAM metadata and an inline dynamic NVAPI client using the same
library, query IDs, thermal layout (hotspot index 9 and VRAM index 15), voltage
layout, and `/dev/nvidiactl`/`/dev/nvidia<minor>` approach. The daemon's
`get_gpu_info()` path already publishes `hotspot_temperature`,
`memory_temperature`, and NVAPI voltage when the GPU is active. `[verified]`

The directory therefore duplicates the daemon's undocumented NVIDIA
extensions rather than extending them with a unique thermal/VRAM feature.
`[verified]` It *does* contain a much broader NVML-backed controller surface
(fan curves, process utilization, pstate/clock control, and rich LACT device
metadata) than the daemon's `GpuInfo` monitor/control path, but that is a
different integration model, not an additional sensor required by the open
VRAM/hotspot question. `[verified]` Wiring it in would be a framework
replacement or consolidation decision, not a simple import. `[assumed]`

## Ranked findings

1. **Duplicate undocumented extension (high):** daemon already implements the
   same NVAPI hotspot/VRAM/voltage and RM-backed VRAM metadata paths. `[verified]`
2. **Not build-connected (high):** the directory lacks a crate boundary and
   depends on absent host framework symbols, so it is not independently
   functional in this workspace. `[verified]`
3. **Broader but incompatible controller surface (medium):** the files offer
   richer NVML control/telemetry than daemon `GpuInfo`, but require the
   LACT controller interfaces and schemas. `[verified]`
4. **Driver/ABI sensitivity (medium):** undocumented query IDs, fixed C
   layouts, and direct RM ioctls can fail by driver/GPU/power state; callers
   generally degrade to optional fields or errors. `[verified]`
