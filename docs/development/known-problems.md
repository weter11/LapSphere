# Known problems and investigation candidates

This list records current-source observations, not fixes. `[verified]` means
directly traced in the scoped daemon files; `[assumed]` means an inferred
candidate.

## General findings

- Scheduler callbacks run synchronously and can overlap DBus hardware calls;
  there is no process-wide hardware-operation lock. `[verified]`
- Shutdown restores CPU frequency limits but does not explicitly stop or join
  the scheduler, DBus task, or GUI waiter. `[verified]`
- The scheduler command channel is unbounded. `[verified]`
- `MANUAL_GPU_OFFSETS` has no visible pruning operation. `[verified]`
- The log buffer is explicitly capped at 2,000 entries. `[verified]`

## Ranked leak/lifecycle-bug candidates

1. Missing scheduler shutdown/join and retained job closures. `[assumed]`
2. Unguarded concurrent `GetGpuInfoFull` blocking tasks. `[assumed]`
3. GPU-index offset entries retained across topology changes. `[assumed]`
4. Backlog growth in the unbounded scheduler command channel. `[assumed]`
5. One temporary shutdown thread per non-systemd DBus shutdown request.
   `[assumed]`

These candidates require runtime observation or broader source inspection to
confirm; this pass intentionally does not change implementation behavior.
