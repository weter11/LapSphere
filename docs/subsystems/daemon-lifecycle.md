# Daemon lifecycle and concurrency

Scope: `daemon/src/main.rs`, `dbus_interface.rs`, `polling_scheduler.rs`,
`hardware_control.rs`, and `battery_control.rs`. Claims are marked
`[verified]` or `[assumed]`; no fixes are proposed.

## Startup sequence

1. Tokio creates the async runtime and `main` initializes logging. `[verified]`
2. Help and non-root CLI paths can return or exit before daemon startup.
   `[verified]`
3. Root startup optionally opens `/dev/tuxedo_io`, probes battery charge
   control, and creates a `PollingScheduler`. `[verified]`
4. A scheduler handle is stored once in the global `OnceCell`. `[verified]`
5. `refresh_hardware_cache()` runs synchronously before background tasks.
   `[verified]`
6. `tokio::spawn` starts the scheduler. The monitor, optional fan-control, and
   GPU-overclock jobs are then submitted through its handle. `[verified]`
7. A system DBus connection is created; a second `tokio::spawn` registers the
   object and requests `io.lapsphere.Control`. `[verified]`
8. With `--gui`, a child process is spawned and another Tokio task waits for
   it. `[verified]`

## Ownership of long-lived state

- The scheduler task owns its receiver and `BinaryHeap`; the global handle owns
  a sender clone. `[verified]`
- Hardware snapshots, settings, fan/GPU state, offsets, and logs are lazy
  process-global values protected by mutexes or atomics. `[verified]`
- The fan callback captures an `Arc` clone of the initialized `TuxedoIo`.
  `[verified]`
- The DBus connection is owned by `main` and a clone moved into the service
  task. `[verified]`
- `BatteryControl` is constructed per battery DBus operation and retains only a
  `PathBuf`; no persistent battery handle is retained. `[verified]`
- `hardware_control.rs` keeps one lazy process-lifetime NVML value. `[verified]`

## Scheduler and DBus concurrency

The scheduler executes due jobs synchronously in its own task. It removes all
due jobs, calls each callback, and re-inserts each with `next_run = now +
interval`, including after errors. Jobs do not spawn a task per poll tick.
`[verified]`

DBus methods are async and can run while the scheduler polls. Cache/state
overlap is coordinated by the relevant mutex or atomic, and cache reads clone
before serialization. `[verified]` There is no single lock around hardware
control, so a DBus mutation and a polling callback can touch the same kernel or
NVML resource concurrently. `[verified]`

The scheduler command channel is unbounded; commands are applied by its one
receiver in receive order. `[verified]`

## Shutdown and cleanup

`main` awaits Ctrl-C and restores CPU frequency limits if they were modified.
`[verified]` It does not send `SchedulerCommand::Shutdown`, join the scheduler
task, join the DBus task, or explicitly join the GUI waiter. `[verified]`
Remaining cleanup therefore relies on process termination. `[assumed]`

The DBus shutdown method starts a temporary thread that sleeps 200 ms and
raises SIGINT when the daemon is not systemd-managed; under systemd it returns
without signalling. `[verified]` Repeated accepted calls can create repeated
threads before they fire. `[assumed]`

## Findings and candidate leak patterns

- The log deque is capped at 2,000 entries and evicts from the front; it is not
  an unbounded cache. `[verified]`
- Hardware cache fields are replaced on refresh rather than appended to.
  `[verified]`
- `MANUAL_GPU_OFFSETS` is a `HashMap` keyed by GPU index with no visible
  pruning. `[verified]` Stale entries across changing hardware topology are a
  candidate, but accumulation is `[assumed]`.
- The unbounded command channel can grow if producers outpace the scheduler.
  `[verified]` A sustained backlog is `[assumed]`.
- Jobs retain their callbacks until removed or process exit. The fan callback
  retains its `TuxedoIo` `Arc`, and `main` has no removal/shutdown path.
  `[verified]` An FD/resource leak is `[assumed]`, not proven here.
- `GetGpuInfoFull` creates one blocking task per DBus call with no visible
  in-flight guard. `[verified]` Resource amplification under bursts is
  `[assumed]`.
- Battery objects are local and dropped at call return. This is allocation
  churn, not an identified leak. `[verified]`

## Ranked leak/lifecycle-bug candidates

1. **Missing scheduler shutdown/join:** no explicit stop or join exists.
   `[verified]` Retained callbacks/resources surviving until abrupt teardown
   are `[assumed]`.
2. **Concurrent full NVML refresh tasks:** each call uses `spawn_blocking`.
   `[verified]` Burst amplification is `[assumed]`.
3. **Unpruned GPU offset map:** insertion is keyed by device index.
   `[verified]` Stale-key growth is `[assumed]`.
4. **Unbounded scheduler command queue:** the channel has no capacity.
   `[verified]` Memory growth requires a producer backlog and is `[assumed]`.
5. **Shutdown thread spawning:** one temporary thread per accepted request.
   `[verified]` Brief accumulation from repeated requests is `[assumed]`.
