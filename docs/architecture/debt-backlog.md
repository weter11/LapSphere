# Evidence-backed debt backlog

This is a read-only design-review backlog. It ranks candidates by confidence
in the documented evidence, not by the amount of code they might remove. The
items below use only findings already recorded in the architecture and
subsystem documents; source references are included to verify the cited
behavior. None of these items is an implementation decision.

## 1. Decide the fate of the disconnected `nvidia/` directory

- **What:** Delete the top-level `nvidia/` directory, or consolidate its
  needed capabilities into the daemon's in-tree NVIDIA implementation.
- **Why:** The NVIDIA subsystem records both a duplicate undocumented sensor
  and metadata implementation (ranked finding 1) and that the directory is
  not build-connected (ranked finding 2). The module map says it is not a
  workspace member, has no repository dependencies, is not dead or unwanted,
  and remains an undecided integration gap. The daemon already exposes the
  same hotspot, VRAM-temperature, voltage, and VRAM-metadata paths.
  `[verified]` (`docs/subsystems/nvidia-crate.md`, “Ranked findings” 1–2;
  `docs/architecture/modules.md`, `nvidia/`; source:
  `nvidia/driver.rs`, `nvidia/nvapi.rs`, `nvidia/nvidia.rs`,
  `daemon/src/hardware_detection.rs`, `Cargo.toml`)
- **Risk/blockers:** The maintainer must decide whether the broader LACT-style
  controller surface (fan curves, process utilization, clocks, and rich
  metadata) is a product requirement. If it is, define the replacement or
  consolidation boundary, supply the absent framework/bindings/schema, and
  compare behavior before deleting anything. Do not treat this as a simple
  import.
- **Counter-argument:** The directory is explicitly documented as not dead or
  unwanted. It may be the intended source for future NVIDIA control
  integration, and deleting it would discard functionality that is broader
  than the daemon's current monitor/control path.

## 2. Remove the GUI's no-op `gpu_overclock` refresh registration

- **What:** Remove or consolidate the GUI registration and interval-update
  path for `gpu_overclock` unless a real GUI refresh callback is decided.
- **Why:** The GUI findings rank “unregistered/no-op refresh coverage” and
  state that `gpu_overclock` is registered but has no matching refresh branch,
  so its scheduled callback performs no D-Bus fetch. The source confirms the
  registration at `gui/src/app.rs:376`, while the refresh dispatch handles
  CPU, GPU, memory, fans, battery, Wi-Fi, gamepads, storage, mount, webcam,
  and logs, then falls through at `gui/src/app.rs:360`; tuning still updates
  the same interval (`gui/src/pages/tuning.rs`).
  `[verified]` (`docs/subsystems/gui-state-sync.md`, ranked finding 5;
  source: `gui/src/app.rs:285-360, 366-378`)
- **Risk/blockers:** Decide whether GPU overclock statistics should be
  periodically fetched, and if so identify the intended payload and state
  update before removing the registration. Check that settings persistence and
  tuning controls no longer depend on the component ID.
- **Counter-argument:** The registration may be deliberate scaffolding for a
  future refresh operation, while the daemon's GPU overclock polling is a
  separate, functioning path. Removing it could make a future implementation
  less discoverable or silently remove a planned cadence hook.

## 3. Consolidate or bound persistent remembered-gamepad state

- **What:** Simplify `remembered_gamepads` so disconnected records and
  duplicate UIDs do not accumulate indefinitely; alternatively consolidate it
  with the current gamepad snapshot representation.
- **Why:** The GUI invariant says remembered gamepads are exact-UID merged,
  append-only, and never removed (invariants item 15). The subsystem finding
  records that duplicate remembered UIDs and UID churn are retained and
  written back to disk, with `.find()` updating only the first duplicate.
  Hardware detection separately documents that daemon UIDs can change across
  scans because identity is not stable across all input nodes or calls.
  `[verified]` (`docs/architecture/invariants.md`, item 15;
  `docs/subsystems/gui-state-sync.md`, “Merge semantics” and ranked finding 1;
  source: `gui/src/app.rs:543-580`)
- **Risk/blockers:** Decide whether remembered devices are intended to be a
  user-managed history or only a current-device list. Any identity
  reconciliation policy must be chosen with the daemon's documented UID
  limitations, and settings compatibility/migration must be specified before
  changing the persisted `AppConfig` shape.
- **Counter-argument:** Keeping disconnected devices is potentially
  intentional UX: it preserves user selections and allows a device to
  reappear without losing configuration. A cleanup rule could erase a
  legitimate device or worsen the daemon/GUI identity mismatch.

## 4. Simplify scheduler ownership by wiring explicit shutdown

- **What:** Consolidate scheduler lifetime handling around one explicit
  shutdown-and-join path, rather than retaining callbacks and relying on
  process termination.
- **Why:** The lifecycle invariant says graceful scheduler shutdown is not
  wired (invariants item 13). The lifecycle findings record that jobs retain
  callbacks until removal or process exit and rank missing scheduler
  shutdown/join first. `main` starts the scheduler and does not send
  `SchedulerCommand::Shutdown` or join it, while the scheduler already has a
  `Shutdown` command that breaks its loop.
  `[verified]` (`docs/architecture/invariants.md`, item 13;
  `docs/subsystems/daemon-lifecycle.md`, “Shutdown and cleanup” and ranked
  finding 1; source: `daemon/src/main.rs:362-406`,
  `daemon/src/polling_scheduler.rs:144-147`)
- **Risk/blockers:** Define shutdown ordering for D-Bus, GUI waiting, hardware
  restoration, and scheduler callbacks. Confirm that callbacks may be
  dropped safely after in-flight hardware operations and decide how repeated
  shutdown requests are handled.
- **Counter-argument:** The daemon is normally terminated as a process, and
  the existing shutdown command may be sufficient for future callers. Adding
  joins and ordering can make shutdown hang on a blocked hardware operation,
  complicating a path that currently restores CPU limits promptly.

## 5. Reconsider duplicate cadence and queue machinery

- **What:** Evaluate whether GUI refresh scheduling, daemon polling scheduling,
  and their unbounded command queues can be consolidated or bounded around a
  single ownership model.
- **Why:** The GUI invariant records separate GUI and daemon polling clocks
  (item 17), while the GUI findings record overlapping refresh tasks and an
  unbounded D-Bus command backlog. The daemon findings independently record an
  unbounded scheduler command queue. These are documented structural
  duplication and backlog risks, though memory growth requires sustained
  pressure and is not itself proven.
  `[verified]` (`docs/architecture/invariants.md`, item 17;
  `docs/subsystems/gui-state-sync.md`, ranked findings 3–4;
  `docs/subsystems/daemon-lifecycle.md`, ranked findings 2–4)
- **Risk/blockers:** Decide whether independent cadences are a deliberate
  privilege/process-boundary requirement. Measure acceptable refresh latency,
  define backpressure/coalescing semantics, and preserve the daemon's
  scheduler ownership invariant before removing or merging either scheduler.
- **Counter-argument:** The GUI and daemon have different responsibilities and
  lifetimes; separate clocks let the GUI adapt to user preferences without
  changing privileged hardware polling. Consolidating them could increase
  coupling across the D-Bus boundary and make outages less manageable.

## 6. Prune index-keyed GPU offset state if topology churn is accepted

- **What:** Simplify `MANUAL_GPU_OFFSETS` lifecycle by pruning entries that no
  longer correspond to an observed GPU, or replace the index-keyed map with a
  decided stable identity.
- **Why:** The lifecycle document records the map as index-keyed with no
  visible pruning and ranks unpruned GPU offset state as a candidate. Hardware
  detection documents that NVIDIA indexes can change and that index reuse can
  misattribute state. The source confirms the process-global map declaration
  (`daemon/src/main.rs:82-83`) and its index-based accesses in
  `daemon/src/hardware_detection.rs`.
  `[verified]` (`docs/subsystems/daemon-lifecycle.md`, ranked finding 3;
  `docs/subsystems/hardware-detection.md`, known duplication and identity
  risks; source: `daemon/src/main.rs:82-83`,
  `daemon/src/hardware_detection.rs:1538,2888`)
- **Risk/blockers:** Establish whether offsets are intentionally retained
  across temporary device disappearance, and choose a physical identity
  strategy compatible with the documented NVIDIA enumeration behavior.
  Confirm reset semantics before pruning any user-applied settings.
- **Counter-argument:** The documented stale-key growth and accumulation are
  only candidates (`[assumed]`), not a demonstrated production failure.
  Pruning could lose offsets during transient GPU suspension or reconnect and
  change established overclock behavior.
