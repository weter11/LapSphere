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

### 12. DBus calls and polling may overlap hardware I/O

**Rule:** No source-confirmed invariant prevents a DBus operation from running
concurrently with a scheduler callback touching the same hardware resource.
`[verified]`

**Implication:** “A DBus call and polling tick never touch Z at the same time”
is not confirmed; treating that serialization as guaranteed is `[assumed]`.

### 13. Graceful scheduler shutdown is not wired

**Rule:** Main restores CPU frequency limits but does not send the scheduler
`Shutdown` command or join its task. `[verified]`

**Implication:** Cleanup of scheduler-owned callbacks before process exit is
not guaranteed. `[assumed]`
