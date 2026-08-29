# Architecture Decision Records

All entries are observations from current source, not proposed fixes. `[verified]`
means directly traced; `[assumed]` means inferred.

## ADR-001: One process-local polling scheduler

**Decision:** The daemon creates one `PollingScheduler`, registers the monitor,
optional fan, and GPU-overclock jobs, and exposes a cloneable sender through
`SCHEDULER_HANDLE`. `[verified]`

**Reason:** This centralizes periodic work and lets D-Bus update intervals
without directly mutating the job heap. `[verified]`

**Consequences:** Job callbacks execute in the scheduler task, while D-Bus
methods execute through Tokio and may overlap with callbacks. Shared state uses
the existing mutexes or atomics. `[verified]`

## ADR-002: Process-global locked state

**Decision:** Hardware snapshots, settings, fan/GPU state, offsets, and logs
use lazy process-global `Arc<Mutex<_>>` values; the one-shot NVML request uses
an `AtomicBool`. `[verified]`

**Reason:** Polling callbacks and D-Bus methods need the same state. `[verified]`

**Consequences:** Callers usually clone state while holding a lock, then do
hardware I/O after releasing it. No single lock serializes all hardware
operations. `[verified]`

## ADR-003: Explicit blocking pool for full GPU refresh

**Decision:** `GetGpuInfoFull` uses `tokio::task::spawn_blocking` for its full
NVML query; other D-Bus hardware methods call synchronous functions directly.
`[verified]`

**Reason:** The full query is documented as blocking and should not occupy the
async reactor. `[verified]`

**Consequences:** Concurrent full-refresh calls can create concurrent blocking
tasks; no in-flight guard is visible in the scoped source. `[verified]`
