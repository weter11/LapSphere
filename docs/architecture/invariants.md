# Architecture invariants

Every claim is marked `[verified]` when traced in `daemon/src/hardware_detection.rs`, or `[assumed]` when inferred. No fixes are proposed here.

## Gamepads

### 1. Current-pass duplicate suppression is UID equality

**Rule:** A gamepad is emitted at most once per `get_gamepad_info()` invocation for each exact derived UID. `[verified]`

**Enforced where:** `get_gamepad_info()` stores derived values in a local `HashSet<String>` named `seen_uids`. `[verified]`

**Limitation:** This is not a physical-device invariant across input nodes or calls. The UID can be serial short, serial, path, sysfs fallback, or `device/uniq`, and the function has no cross-pass identity map. `[verified]`

### 2. Gamepad observations are always marked connected

**Rule:** Every emitted gamepad has `GamepadStatus::Connected`; absent devices are omitted rather than emitted as disconnected. `[verified]`

**Enforced where:** `get_gamepad_info()` constructs `GamepadInfo` only for entries observed during the current scan. `[verified]`

## GPUs

### 3. NVIDIA idle snapshots are index-scoped

**Rule:** Last-known NVIDIA metrics are associated with the numeric GPU index and expire after `IDLE_CACHE_TTL_SECS` (30 seconds). `[verified]`

**Enforced where:** `IDLE_METRICS_CACHE` is a `HashMap<u32, IdleCacheEntry>` and freshness compares elapsed time to the constant. `[verified]`

**Implication:** Stable physical identity across index changes is not guaranteed. `[verified]`

### 4. NVIDIA and non-NVIDIA GPU paths are mutually filtered by vendor

**Rule:** DRM entries with vendor `0x10de` are skipped because NVIDIA GPUs are expected from NVML; non-NVIDIA DRM entries are emitted as integrated GPUs. `[verified]`

**Enforced where:** `get_gpu_info()` checks the DRM vendor before constructing the record. `[verified]`

## Wi-Fi and storage

### 5. Wi-Fi rate history is keyed by interface name

**Rule:** Throughput deltas are calculated only against the prior sample stored under the same interface string. `[verified]`

**Enforced where:** `PREVIOUS_NET_STATS` is keyed by `String` and `read_wifi_rates()` inserts using the interface name. `[verified]`

### 6. Storage rate history is keyed by block-device name

**Rule:** Storage throughput deltas are calculated only against the prior sample stored under the same `/sys/block` entry name. `[verified]`

**Enforced where:** `PREVIOUS_STORAGE_STATS` is keyed by `String` and `get_storage_device_info()` passes `dev_name` to `calculate_storage_rates()`. `[verified]`

## Caches and fallback selection

### 7. System and memory metadata are process-local caches

**Rule:** System identity and memory type/speed metadata do not refresh after their first successful initialization or cache population within the process. `[verified]`

**Enforced where:** `get_system_info()` uses `CACHED_SYSTEM_INFO`; `get_memory_type_and_freq()` uses `CACHED_MEM_METADATA`. `[verified]`

### 8. First matching provider wins in several fallback scans

**Rule:** When multiple matching hwmon, power-supply, thermal, or directory entries exist, the function may return the first one encountered rather than reconciling all candidates. `[verified]`

**Enforced where:** `get_package_temp()`, `get_core_temp()`, `find_battery_for_input()`, `read_wifi_temperature()`, and `find_storage_temperature()` return from the first successful match. `[verified]`

## Verification of the seeded entry

The prior seeded claim that gamepad identity is guaranteed per physical device is **not confirmed as an invariant by current source**. `[verified]` The source attempts stable identity resolution and suppresses exact duplicate UIDs within one pass, but sibling event nodes can have different udev-property availability, the fallback uses `inputN` paths, `device/uniq` is conditional, and no cross-pass registry exists. `[verified]`

## Lifecycle and concurrency

### 9. The scheduler owns job execution

**Rule:** Poll callbacks execute in the scheduler loop, not in a new task per
tick; due jobs are rescheduled after execution, including on error. `[verified]`

**Enforced where:** `daemon/src/polling_scheduler.rs::run()` and
`execute_due_jobs()`. `[verified]`

### 10. Scheduler commands have one receiver

**Rule:** Add, update, remove, and shutdown commands are applied by the
scheduler task in channel receive order. `[verified]`

**Enforced where:** `PollingScheduler::run()` and `SchedulerHandle`. `[verified]`

### 11. Shared daemon state is mutex/atomic protected

**Rule:** Shared cache, settings, fan/GPU state, offsets, and logs use their
declared mutexes; the one-shot NVML request is atomic. `[verified]`

**Enforced where:** globals in `daemon/src/main.rs` and accesses in
`dbus_interface.rs`, `hardware_control.rs`, and polling callbacks. `[verified]`

### 12. D-Bus calls and polling may overlap hardware I/O

**Rule:** No source-confirmed invariant prevents a D-Bus operation from running
concurrently with a scheduler callback touching the same hardware resource.
`[verified]`

**Implication:** “A DBus call and polling tick never touch Z at the same time”
is not confirmed; treating that serialization as guaranteed is `[assumed]`.

### 13. Graceful scheduler shutdown is not wired

**Rule:** Main restores CPU frequency limits but does not send the scheduler
`Shutdown` command or join its task. `[verified]`

**Implication:** Cleanup of scheduler-owned callbacks before process exit is
not guaranteed. `[assumed]`

## GUI state synchronization

### 14. Ordinary GUI hardware snapshots replace fields

**Rule:** Successful ordinary hardware responses replace their corresponding
`AppState` field; the GUI does not merge those vectors by device key.
`[verified]`

**Enforced where:** `gui/src/app.rs::handle_hardware_updates()` assigns the
`HardwareUpdate` payloads directly to the CPU, GPU, Wi-Fi, fan, storage, mount,
and related fields. `[verified]`

### 15. Remembered gamepads are UID-merged and append-only

**Rule:** The GUI matches remembered gamepads to a response by exact UID, marks
missing remembered entries disconnected, updates matching records, appends
unseen records, and does not remove old records. `[verified]`

**Enforced where:** `gui/src/app.rs::handle_hardware_updates()` processes
`HardwareUpdate::GamepadInfo`; the result is persisted through
`save_settings()`. `[verified]`

**Implication:** The GUI assumes daemon UIDs remain stable. Daemon UID churn is
amplified into persistent disconnected/new records; the GUI has no independent
physical-device identity reconciliation. `[verified]`

### 16. Failed refreshes preserve last-known GUI values

**Rule:** A failed D-Bus refresh produces logging/error handling but no generic
state invalidation; the prior successful hardware value remains in `AppState`.
`[verified]`

**Implication:** During a daemon outage or restart, displayed hardware state may
be stale until a later successful poll. `[assumed]`

### 17. GUI and daemon polling clocks are separate

**Rule:** GUI refresh intervals are owned by `RefreshCoordinator`; saving
settings separately sends poll-rate JSON to the daemon. `[verified]`

**Implication:** Matching configured rates does not establish synchronized
execution or a coordinated refresh boundary. `[assumed]`

## Shared daemon/GUI data contracts

### 18. Shared structs are JSON field-name contracts

**Rule:** Public structs in `common/src/types.rs` use Serde's default field
names and are exchanged as JSON strings over D-Bus and in GUI configuration
files. `[verified]`

**Enforced where:** `common/src/types.rs` derives `Serialize` and
`Deserialize`; daemon D-Bus methods serialize snapshots/settings and GUI
client methods deserialize them. `[verified]`

**Implication:** Renaming a field, changing its type, or changing an
`Option`/vector shape without coordinated daemon and GUI changes breaks the
transport or silently changes snapshot meaning. `[assumed]`

### 19. Hardware snapshot shapes are load-bearing

**Rule:** `SystemInfo`, `CpuInfo` (including `CpuCapabilities`, `CoreInfo`, and
`PowerSource`), `MemoryInfo`, `GpuInfo`, `BatteryInfo`, `FanInfo`, `WiFiInfo`,
`StorageDevice`, `MountInfo`, and `GamepadInfo` are shared snapshot contracts.
`[verified]` `GpuInfo` includes daemon-populated optional hotspot,
memory-temperature, voltage, VRAM metadata, capability/range, and
`nvml_index` fields consumed by GUI code. `[verified]`

### 20. Enum spellings and variants are wire values

**Rule:** Variants of `GpuType`, `GamepadStatus`, `ConnectionType`,
`PowerStatus`, `KeyboardType`, `KeyboardMode`, `FontSize`, and `Theme` are
serialized enum values; variant names and `KeyboardMode` payload shapes must
remain aligned. `[verified]`

**Enforced where:** daemon matches `KeyboardMode` variants for hardware
control, while GUI constructs and matches the same variants and deserializes
daemon responses. `[verified]`

**Implication:** Renaming/removing a variant can turn valid JSON into a
deserialization failure; an added variant can be incompatible with an older
peer. `[assumed]`

### 21. Settings are persistent compatibility contracts

**Rule:** `AppConfig` and nested `Profile`, CPU/GPU/keyboard/screen/fan
settings, `GpuAdvancedSettings`, `NvidiaFanSettings`, `FanCurve`, and
`BatterySettings` define the GUI disk format and daemon settings payloads.
`[verified]`

Fields marked `#[serde(default)]` or explicit default functions are
backward-compatibility points; fields without defaults are required during
deserialization. `[verified]` `StatisticsSections` also carries section names
and millisecond polling rates mirrored into daemon scheduler settings.
`[verified]`

### 22. Defaults do not establish validation

**Rule:** `Default` implementations provide initial values, but shared types
contain no cross-field validation or version tag. `[verified]`

**Implication:** Both peers must preserve conventions such as frequency,
temperature, and power units, tuple ordering, fan IDs, and section-name
strings; a type-compatible semantic change can fail silently. `[assumed]`

## Ranked shared-contract findings

1. **Wire-shape mismatch (high):** Serde field names, optionality, tuple shape,
   and enum representation are the daemon/GUI protocol. `[verified]`
2. **GPU contract coupling (high):** `GpuInfo` carries ordinary NVML values
   and optional NVIDIA extensions; dropping fields loses telemetry. `[verified]`
3. **Persistent settings compatibility (high):** `AppConfig` and nested
   settings are disk and D-Bus contracts, with only selected fields defaulted.
   `[verified]`
4. **Semantic drift risk (medium):** units, IDs, and string conventions are
   not encoded in types or versioned. `[verified]`
