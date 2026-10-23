# Architecture Decision Records

All entries are observations from current source, not proposed fixes. `[verified]`
means directly traced; `[assumed]` means inferred.

## ADR-001: Separate the privileged daemon from the per-user GUI

- **Status:** Accepted retrospectively `[verified]`
- **Decision:** Hardware access, polling, cache ownership, and mutations live in
  `lapsphere-daemon`, while `lapsphere` provides the desktop UI as a separate
  per-user process. The daemon runs as root and the GUI runs as the invoking
  regular user. `[verified]`
- **Reason:** The application needs privileged Linux hardware access without
  making the desktop UI itself privileged. This boundary also keeps rendering
  and user configuration out of the hardware-facing process. `[assumed]`
- **Rejected alternatives:** none documented in project history.
- **References:** [Architecture overview](overview.md), [module map](modules.md),
  and [daemon lifecycle](../subsystems/daemon-lifecycle.md).

## ADR-002: Use system D-Bus as the daemon/GUI IPC boundary

- **Status:** Accepted retrospectively `[verified]`
- **Decision:** The daemon publishes `io.lapsphere.Control` at
  `/io/lapsphere/Control` on the system bus, and the GUI communicates with the
  daemon through that interface. `[verified]`
- **Reason:** A system-bus service provides a process boundary between the
  privileged daemon and user GUI while supporting service activation and
  asynchronous requests. `[assumed]`
- **Rejected alternatives:** none documented in project history.
- **References:** [Architecture overview](overview.md), [daemon lifecycle](../subsystems/daemon-lifecycle.md),
  and [GUI state synchronization](../subsystems/gui-state-sync.md).

## ADR-003: Keep daemon/GUI domain contracts in a shared Rust crate

- **Status:** Accepted retrospectively `[verified]`
- **Decision:** `lapsphere-common` is a workspace crate containing the
  serializable hardware, profile, settings, and UI-facing domain types consumed
  by both the daemon and GUI. It contains no runtime I/O, scheduling, hardware
  access, or rendering. `[verified]`
- **Reason:** A single type and serialization definition keeps the two sides of
  the D-Bus boundary aligned while preserving their independent runtime
  responsibilities. `[assumed]`
- **Rejected alternatives:** none documented in project history.
- **References:** [Architecture overview](overview.md), [module map](modules.md),
  and [GUI state synchronization](../subsystems/gui-state-sync.md).

## ADR-004: Drop the LACT-derived NVIDIA tree and keep the sensors in the daemon

- **Status:** Accepted `[maintainer-confirmed]`
- **Decision:** The top-level NVIDIA directory that was adapted from LACT is
  removed from the repository. NVIDIA support stays entirely on the daemon's
  in-tree paths, which read hotspot and memory-sensor temperatures from NVAPI
  (`daemon/src/hardware_detection.rs`). `[verified]`
- **Reason:** The NVIDIA open kernel module now covers nearly all of the
  telemetry that directory duplicated. The part NVIDIA has not opened is the
  hotspot and memory-sensor temperatures, and the daemon already reads those
  itself. `[maintainer-confirmed]`
- **Rejected alternatives:** wiring the LACT controller into the workspace — it
  needs LACT's controller framework, generated bindings and `lact_schema`, and
  duplicates telemetry the daemon already publishes. `[verified]`
- **References:** [Architecture overview](overview.md), [module map](modules.md).

## Considered but excluded

- **One process-local polling scheduler:** excluded because its execution,
  rescheduling, and concurrency rules are already fully documented in
  [invariants](invariants.md) and [daemon lifecycle](../subsystems/daemon-lifecycle.md).
- **Tuxedo ioctl boundary:** excluded because the existing
  [Tuxedo I/O subsystem document](../subsystems/tuxedo-io.md) already describes
  that boundary rather than recording a separate project-shaping choice.
