# GPU process and VRAM statistics

GPU statistics carry a backwards-compatible (serde-defaulted) PCI identity, process snapshot, and optional VRAM memory snapshot through the existing GetGpuInfo JSON/cache path. No new polling job or D-Bus method is added.

## Meaning and limitations

- GPU device holders are processes with open per-device NVIDIA or DRM card/render handles. They are candidates for investigation, NOT evidence of current rendering/compute work or proof of preventing runtime suspend. Driver/kernel/display state can also block sleep with no listed process.
- Discovery resolves PCI devices to DRM nodes and NVIDIA Device Minor from procfs. Global /dev/nvidiactl, UVM and capability nodes are deliberately not attributed to every GPU.
- Only PID, comm name and matching device paths are exposed, never command lines, environment or file contents. Processes and duplicate handles are deduplicated and ordered by PID. Process exit races are tolerated. Permission errors are marked incomplete, not presented as an authoritative empty list.
- The root daemon normally sees more processes than an unprivileged test. Procfs restrictions, containers and process races can still limit visibility. A complete flag means the scan encountered no access error, not an atomic or universally exhaustive inventory.
- Proc scans are cached for five seconds per PCI identity. They read links/metadata, never open GPU device nodes or invoke NVML. The GUI displays snapshot age.
- NVIDIA free/used/total VRAM (MiB) is sampled only inside the existing already-authorized NVML full-pass path. Suspended and fresh idle-cache paths do not gain a query. Cached memory retains its original timestamp; unknown is not zero, and free is not computed as total minus used because reserved memory exists. Non-NVIDIA free VRAM is currently unavailable rather than estimated from shared RAM.
- Existing periodic full-pass TTL and explicit GetGpuInfoFull wake semantics are unchanged. This feature does not fix the historical polling/driver power issue or guarantee that the entire daemon never wakes the GPU.

## Verification

- Fixture scanner regression went RED (missing process) before implementation, then GREEN. Tests cover per-device matching, duplicate descriptors, unrelated/global descriptors, unavailable procfs and memory free/used distinction.
- Headless egui test renders unknown and populated payloads and verifies old JSON defaults and new JSON round-trip.
- Explicit live scanner test under strace: three scans, zero /dev/nvidia or /dev/dri opens, GPU suspended before and after. The unprivileged live result correctly reports incomplete visibility.
- Existing fan/keyboard regression fixes were separately verified on the installed daemon via ApplyProfile and sysfs/fan readbacks earlier in this session.
- New feature full deployed daemon-to-GUI verification requires installing the CI package. No claim of that deployment test is made from unit/headless tests.

References: https://docs.kernel.org/gpu/drm-usage-stats.html ; NVIDIA NVML memory information API https://docs.nvidia.com/deploy/nvml-api/group__nvmlDeviceQueries.html
