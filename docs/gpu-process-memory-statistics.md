# GPU process and VRAM statistics

GPU statistics carry a backwards-compatible (serde-defaulted) PCI identity, process snapshot, and optional VRAM memory snapshot through the existing GetGpuInfo JSON/cache path. No new polling job or D-Bus method is added, and there is no on-demand full-query method: every consumer reads the `hardware_monitor` tick.

## Status fields

The old single `status` string mixed two namespaces. It is replaced by two distinct, uncached values:

- `runtime_status` — the Linux PCI runtime power-management word, read verbatim from `/sys/bus/pci/devices/<bdf>/power/runtime_status` ("active", "suspended", ...). A cheap sysfs read that never wakes the GPU. Discrete GPUs only: an integrated GPU never runtime-suspends, so the word would be constant noise.
- `performance_state` — the NVML performance state ("P0".."P15") from the most recent live query. `None` means "not queried", never "same as last time": the quiet range (P3 and deeper) and the suspended state publish blanks rather than a stale number.

The GUI renders the two as one line (e.g. `P0 · active`) but they remain separate values on the wire.

## Polling tiers (RTD3-aware)

Any NVML call resets the kernel's 20 s autosuspend timer, so the single poll block decides per tick what a GPU is allowed to receive:

| Observation | Tier | What is queried |
| --- | --- | --- |
| sysfs says `suspended` | Suspended | nothing (a probe would wake the GPU) |
| first sight, a suspend→active wake, or last known P0..P2 | Live | full NVML + NVAPI pass, every tick |
| last known P3 and deeper | Quiet | sysfs only; all dynamic fields published blank |

The quiet tier allows one bounded re-probe after `QUIET_REPROBE_SECS` (45 s) so a GPU that ramps from a light P8 back to P0 is noticed without a wake. The cadence is deliberately longer than the kernel's autosuspend delay plus the driver's release latency (measured: this dGPU re-suspends 27 s after the last NVML touch), so an unused GPU is already suspended when the cadence fires and the tier check skips the probe instead of waking it.

## Cached values

Only two figures may be served from a previous sample, because they are cheap and static: VRAM total and VRAM available (with their own sample timestamp). Everything else — status, clocks, load, power, voltage, hotspot, memory temperature — is either live or absent. Static device metadata (name, VRAM type/vendor/bus, clock ranges, supported p-states) is cached as capability data, not as telemetry.

## Meaning and limitations

- GPU device holders are processes with open per-device NVIDIA or DRM card/render handles. They are candidates for investigation, NOT evidence of current rendering/compute work or proof of preventing runtime suspend. Driver/kernel/display state can also block sleep with no listed process.
- Holders that never block dGPU runtime suspend are filtered out: the X server, `nvidia-persistenced`, and the daemon's own device handle (matched by PID). The list is shown for discrete GPUs only, on the Hardware info tab.
- Discovery resolves PCI devices to DRM nodes and NVIDIA Device Minor from procfs. Global /dev/nvidiactl, UVM and capability nodes are deliberately not attributed to every GPU.
- Only PID, comm name and matching device paths are exposed, never command lines, environment or file contents. Processes and duplicate handles are deduplicated and ordered by PID. Process exit races are tolerated. Permission errors are marked incomplete, not presented as an authoritative empty list.
- The root daemon normally sees more processes than an unprivileged test. Procfs restrictions, containers and process races can still limit visibility. A complete flag means the scan encountered no access error, not an atomic or universally exhaustive inventory.
- Proc scans are cached for five seconds per PCI identity. They read links/metadata, never open GPU device nodes or invoke NVML. The GUI displays snapshot age.
- NVIDIA free/used/total VRAM (MiB) is sampled only inside the existing already-authorized live pass. Suspended and quiet paths do not gain a query. Free is not computed as total minus used because reserved memory exists. Non-NVIDIA free VRAM is currently unavailable rather than estimated from shared RAM.
- Integrated GPUs publish no status, no VRAM figures and no holder list: they never runtime-suspend, so all three are constant noise there.

## Direct-driver VRAM identity (type / vendor / bus width)

VRAM type, vendor and bus width come from the driver's RM control interface (`NV2080_CTRL_CMD_FB_GET_INFO` over `NV_ESC_RM_CONTROL`), not from NVML (which exposes no memory-type API). The escape numbers and the registration direction are verified against the installed driver:

- `NV_ESC_RM_ALLOC` = `0x2B` (a client and then device/subdevice objects allocate with status `NV_OK`); `0x23` is rejected with `EINVAL`.
- `NV_ESC_RM_CONTROL` = `0x2A`; issuing a control with `0x2B` lands in the allocator.
- `NV_ESC_REGISTER_FD` = `201` and it is issued *on the device fd* passing the *control fd*. Any other combination returns `EINVAL`.
- Failure statuses are decoded from the driver's own `common/inc/nvstatuscodes.h`.

On driver 610.57.04 the FB-info query itself returns `NV_ERR_INVALID_ADDRESS` for its nested list pointer, so type and vendor stay unknown there; the bus width falls back to NVML (`memoryBusWidth`), which also restores the bandwidth figure. The failure is reported once per daemon run, not once per poll.

## Verification

- Tier decisions, the quiet/suspended payload shape (no stale telemetry) and the holder filter have pure unit tests.
- HIL tests (run explicitly, ignored by default) exercise the real driver: a live pass followed by quiet ticks that publish blanks while the GPU still reaches `runtime_status` "suspended"; a P0 load run asserting every call returns live clocks/load/power/hotspot/voltage; a suspended run asserting the call does not wake the GPU; and a raw probe of the direct-ioctl VRAM path.
- Headless egui test renders legacy and populated payloads, asserts the two status values stay separate on the wire, and that an integrated GPU renders neither a status nor a VRAM row.
- The full deployed daemon-to-GUI check requires installing the CI package.
