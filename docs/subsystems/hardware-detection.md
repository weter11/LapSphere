# Hardware detection deep-dive

## Scope and evidence

This document covers `daemon/src/hardware_detection.rs` only and stays at subsystem level. Claims are marked `[verified]` when directly traced in that file and `[assumed]` when inferred from its behavior.

## Device classes and enumeration

| Class | Enumeration and identity | Lifecycle behavior |
|---|---|---|
| CPU | `/proc/cpuinfo`, CPU sysfs, and hwmon provide aggregate data and per-logical-core records. Core identity is the numeric logical-core index. `[verified]` | Each call reconstructs the snapshot; prior CPU counters are retained in `PREVIOUS_CPU_STATS` to calculate load. `[verified]` |
| Memory | `systemstat` provides current memory totals; `dmidecode -t memory` provides cached type/speed metadata. No device UID is created. `[verified]` | Values are re-read; metadata is process-cached after first lookup. `[verified]` |
| NVIDIA GPU | NVML enumerates GPUs; PCI bus IDs locate `/sys/bus/pci` runtime state and direct NVIDIA device access uses the NVML-derived minor/index. `[verified]` | GPUs are re-enumerated per call. An idle metrics cache is keyed by numeric GPU index and expires after 30 seconds; suspended/idle paths intentionally avoid some NVML calls. `[verified]` |
| Intel/AMD GPU | `/sys/class/drm/card0` through `card3` are scanned; NVIDIA vendors are skipped. Identity in the returned record is effectively the DRM card iteration index, while vendor/device IDs select naming. `[verified]` | A missing card is skipped on the next pass; no persistent reconnect map exists. `[verified]` |
| System fans | `/dev/tuxedo_io` is opened and its reported fan count is enumerated by numeric fan ID. `[verified]` | Availability and count are rechecked; returned records are rebuilt, with no persistent fan identity. `[verified]` |
| NVIDIA fans | Active NVIDIA devices are enumerated through NVML and each fan receives a synthetic ID `100 + gpu_index * 10 + fan_index`. `[verified]` | Suspended devices are skipped; active fan records are rebuilt. `[verified]` |
| Keyboard capability | HID sysfs is scanned for a fixed HID ID and tuxedo kernel-module names/capability files are inspected. `[verified]` | Capability detection is recomputed; no keyboard instance identity is retained. `[verified]` |
| Wi-Fi | `/sys/class/net` entries are filtered by `wireless` or `phy80211`; the network interface name is the returned identity. `[verified]` | Interfaces are re-enumerated each call. Throughput history is keyed by interface name in `PREVIOUS_NET_STATS`. `[verified]` |
| Gamepad | `/sys/class/input/input*` entries are scanned. A joystick udev marker or name heuristic classifies the device; UID priority is `ID_SERIAL_SHORT`, `ID_SERIAL`, `ID_PATH`, then the input sysfs path, overridden by non-zero `device/uniq`. `[verified]` | Each pass returns only currently observed devices with `Connected` status. `seen_uids` suppresses duplicates within one pass; there is no cross-pass connection registry. `[verified]` |
| Battery | The main battery detector selects `BAT0`, else `BAT1`, under `/sys/class/power_supply`. Gamepad battery data separately searches parent power-supply paths, then globally matches names containing `controller` or `gamepad`. `[verified]` | Current values are re-read; selection is first-match/fallback rather than persistent identity tracking. `[verified]` |
| Mounts | `systemstat.mounts()` is filtered to `/` and `/home`; mount point is the returned identity. `[verified]` | The list is rebuilt every call, so mount replacement or disappearance is reflected on the next pass. `[verified]` |
| Storage | `/sys/block` entries are scanned, excluding `loop*` and `ram*`; `/dev/<block-name>` is the returned identity and the block name keys rate history. `[verified]` | Devices are re-enumerated and rate history is retained by block name. Temperature is associated through several hwmon path/canonical-path searches. `[verified]` |

## Identity and UID strategy

### Gamepads

`get_gamepad_info()` is the only class with an explicit stable UID strategy. It reads udev data through event children, prioritizes serial-short, serial, and physical path values, normalizes MAC-like values, falls back to the `inputN` sysfs path, and finally prefers `device/uniq` when meaningful. `[verified]`

The algorithm deduplicates only the current result using exact string equality in `seen_uids`. `[verified]` It does not normalize the fallback sysfs path, reconcile UIDs from different event children, or retain an identity map across calls. `[verified]`

### Other classes

Other classes use names, indexes, paths, or mount points rather than a shared stable UID. `[verified]` No cross-class identity registry exists in this file. `[verified]`

## Connect, disconnect, and reconnect handling

- Enumeration functions are snapshot-oriented: entries absent from the current filesystem/API scan simply do not appear in the returned vector. `[verified]`
- Gamepads always emit `GamepadStatus::Connected` for observed records; this file does not emit a disconnected record or preserve prior records. `[verified]`
- CPU, Wi-Fi, storage, and rate calculations preserve only measurement history, not a lifecycle identity registry. `[verified]`
- GPU idle snapshots preserve last-known metrics by numeric index, not a stable physical-device identity. `[verified]`
- `[assumed]` Consumers therefore infer connect/disconnect/reconnect from differences between successive snapshots, and index reuse or changing fallback identity can look like replacement or duplication.

## Known duplication and identity risks

- A single gamepad can expose multiple `inputN` nodes/event children. Different children can have different udev data, causing one pass to derive serial/path/fallback values inconsistently. `[verified]`
- A gamepad with no stable udev fields falls back to `/sys/class/input/inputN`; `inputN` can change after reconnect, so the same physical device can appear as a new UID. `[verified]`
- `device/uniq` is applied only when readable and non-zero; availability differences between sibling nodes or passes can switch the selected UID. `[verified]`
- `seen_uids` is local to one invocation and cannot prevent duplicates caused by identity changes between invocations. `[verified]`
- DRM GPU records use bounded `cardN` iteration while NVIDIA records use NVML ordering/indexes; separate enumeration orders can change or disagree when devices appear/disappear. `[verified]`
- NVIDIA idle metrics are keyed by numeric index, so index reassignment can attach stale metrics to a different GPU. `[verified]`
- Wi-Fi identity is interface name only; interface renames or recreated interfaces split rate history and can resemble disconnect/reconnect. `[verified]`
- Storage identity is `/dev/<block-name>`/block name; device-name reuse after replacement can preserve or misattribute rate history. `[verified]`
- Battery selection is fixed-name preference (`BAT0` then `BAT1`), while gamepad power lookup is parent/global name matching; multiple matching power supplies can produce different associations. `[verified]`
- hwmon temperature association returns the first matching entry from directory iteration; multiple matching hwmon nodes can yield different sensors across passes. `[verified]`
- CPU temperature and power choose the first matching hwmon/power source or a priority fallback, so multiple providers can disagree without a device UID. `[verified]`

## Places where separate enumeration passes can disagree

1. Gamepad sibling `inputN` entries and their event-child udev records can expose different serial, path, or `uniq` values. `[verified]`
2. Gamepad udev classification and the name heuristic can disagree about whether an entry is a gamepad. `[verified]`
3. Gamepad exclusions are name-based and can remove an entry after udev classified it as a joystick. `[verified]`
4. Gamepad `inputN` numbering can change across reconnects when the sysfs-path fallback is used. `[verified]`
5. Gamepad parent traversal and global power-supply fallback can select different battery supplies. `[verified]`
6. NVIDIA NVML ordering/indexes can disagree with PCI/DRM ordering or change after device availability changes. `[verified]`
7. NVIDIA runtime-status scans can observe a different active/suspended set than the later per-device NVML/status scan. `[verified]`
8. Intel/AMD DRM `cardN` numbering can change when cards are added or removed. `[verified]`
9. System-fan numeric IDs depend on the driver-reported count and platform interface detected at that pass. `[verified]`
10. NVIDIA fan synthetic IDs depend on GPU and fan indexes, both of which can change with enumeration ordering. `[verified]`
11. Wi-Fi interface names can be renamed or recreated, splitting identity and byte-rate history. `[verified]`
12. Wi-Fi device hwmon association can differ between direct, canonical-path, and thermal-zone fallbacks. `[verified]`
13. Storage block names can be reused after replacement, while hwmon association can select a different matching sensor. `[verified]`
14. `/` and `/home` mount records can change filesystem/device identity while the returned key remains only the mount point. `[verified]`
15. CPU hwmon iteration can choose different providers for package/core temperature, and power-source priority can select different providers as availability changes. `[verified]`

