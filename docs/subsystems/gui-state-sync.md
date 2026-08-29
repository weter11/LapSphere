# GUI state synchronization

## Scope and evidence

This deep-dive covers `gui/src/app.rs`, `gui/src/dbus_client.rs`, and
`gui/src/polling_scheduler.rs`. Claims are marked `[verified]` when directly
traced in those files and `[assumed]` when they describe an inferred user-visible
effect. This is archaeology only; no fixes are proposed.

## Pull path and refresh cadence

`LapSphereApp::new` loads configuration from disk, creates a `DbusClient`, and
registers refresh components with a `RefreshCoordinator`. `[verified]` The
configured millisecond rates are used for CPU, GPU, memory, fans, battery,
Wi-Fi, gamepads, storage, mount, and GPU overclock. Webcam and daemon logs are
registered at five seconds. `[verified]` Mounts share the storage poll rate.
`[verified]`

Each component starts with `last_refresh = None`, so its first refresh is due
immediately. Thereafter the coordinator sleeps until the earliest due component,
invokes the callback, and marks that component refreshed. `[verified]` The
callback spawns a separate Tokio task for the D-Bus request, so requests from
different components can overlap even though the D-Bus worker processes its
command queue one command at a time. `[verified]` A callback is marked complete
for scheduling purposes before its asynchronous D-Bus request completes.
`[verified]`

Changing rates in the settings UI updates the GUI coordinator interval for the
matching component. `[verified]` Saving settings also fires and forgets the
serialized statistics settings to the daemon through `SyncDaemonPollSettings`;
failure does not prevent the GUI save. `[verified]` The GUI coordinator and
daemon scheduler are therefore separate cadence owners. `[verified]`

The GUI requests repaint every 500 ms and drains all currently queued hardware
updates on each UI pass. `[verified]` The update channel is bounded to 100
messages; a send can await while the UI is not draining it. `[verified]`

## Initial fetches and local cache

The GUI retains the latest successful values in `AppState`, including optional
single records (system, memory, CPU, battery, hardware interface, keyboard
capabilities, webcam, and tuning limits) and vectors (GPU, Wi-Fi, gamepads,
fans, storage, mounts, logs, thresholds, and TDP profiles). `[verified]`

At startup, system information, battery threshold lists, TDP profiles, hardware
interface information, and keyboard capabilities are fetched once in separate
tasks. `[verified]` The registered periodic components re-fetch their own
snapshots; the corresponding successful update replaces the prior field rather
than merging by key. `[verified]` Daemon logs are also replaced, subject to a
2,000-entry GUI cap and the `log_paused` guard. `[verified]` Tuning range/limit
responses replace the relevant optional value on success and retain an error
string on failure. `[verified]`

There is no general GUI-side freshness timestamp, invalidation marker, or
retry/error state attached to retained hardware values. `[verified]` Thus, when
a request fails, the prior successful value normally remains visible.
`[assumed]`

## D-Bus transport and lifecycle

`DbusClient::new` creates an unbounded command channel and spawns one worker; it
returns successfully without establishing the system-bus connection itself.
[verified] The worker repeatedly creates a system-bus connection and a shared
Control proxy. Connection/proxy setup failures wait two seconds before retrying.
`[verified]`

Each public client method enqueues a command and returns a oneshot receiver.
[verified] The worker sends either the decoded JSON result or an error through
that receiver. `[verified]` A call-level failure is logged; connection/service
loss, timeout, and several service-absence error forms cause the worker to
discard the proxy/connection, back off for two seconds, and reconnect. Ten
consecutive otherwise-unrecognized failures also force reconnection. `[verified]`

During a disconnect or daemon restart, requests already being processed receive
errors and queued requests wait for the worker's reconnect loop. `[verified]`
There is no explicit GUI state clear or “disconnected” update. `[verified]`
After the daemon returns, later successful polls repopulate/replace fields.
`[assumed]` The GUI can therefore display stale last-known data during the outage,
and components whose next poll has not yet arrived remain stale after recovery.
`[assumed]`

If the worker's command sender is closed, enqueueing silently discards the send
error while the caller still receives a receiver. `[verified]` The worker exits
when its command channel closes. `[verified]`

## Merge semantics, especially gamepads

For ordinary periodic vectors and optionals, `handle_hardware_updates` assigns
the response directly to the matching `AppState` field. `[verified]` This is
whole-snapshot replacement, not merge-by-key. `[verified]`

Gamepads have two GUI representations:

* `state.gamepad_info` is replaced wholesale with the latest daemon vector.
  `[verified]`
* `state.config.remembered_gamepads` is persistent local state. For each
  remembered record, the GUI searches the current response for an exact `uid`
  match. A match updates status and selected descriptive/power fields; a
  missing UID is marked disconnected; returned UIDs absent from remembered state
  are appended. `[verified]` The remembered vector is therefore merged by UID,
  while records are never removed. `[verified]` Changes are persisted to
  settings. `[verified]`

This GUI merge assumes daemon UIDs are stable identifiers for the same physical
gamepad across scans and reconnects. `[verified]` It does not independently
derive or reconcile identity. `[verified]` Consequently it inherits the daemon
identity problem documented in `hardware-detection.md` and invariants item 1:
when the daemon emits a changed UID, the GUI marks the old record disconnected
and appends the new record. `[verified]` That creates GUI-visible duplication
and persistent stale records, even if the daemon's current response itself has
no duplicate UID. `[assumed]`

The GUI also has an independent failure mode: because it never deduplicates
`remembered_gamepads` and never garbage-collects disconnected entries, any
pre-existing duplicate remembered UIDs or any UID churn is retained and written
back to disk. `[verified]` Within one response, `.find()` updates only the
first matching remembered record, while the append check tests whether any
matching record exists. `[verified]` Thus the GUI adds persistence/staleness
amplification, but not a separate physical identity algorithm; its primary
identity error is inherited from daemon UID instability. `[assumed]`

## Confirmed GUI-side invariants

1. Successful ordinary hardware snapshots replace their corresponding GUI
   fields; they are not merged by device key. `[verified]`
2. The GUI's remembered gamepad list is merged by exact UID, marks absent
   records disconnected, appends unseen records, and does not remove records.
   `[verified]`
3. A failed D-Bus refresh does not clear the last successful value. `[verified]`
4. Polling starts with an immediate due state per registered component, and rate
   changes affect the GUI coordinator only through its registered component ID.
   `[verified]`
5. GUI repaint cadence (500 ms), GUI polling cadence, and daemon polling cadence
   are distinct mechanisms. `[verified]`

## Ranked GUI-side state-sync risks

1. **Persistent gamepad UID churn amplification:** daemon UID changes become a
   disconnected old record plus a newly appended record and survive in settings.
   `[verified]` The visible duplication/staleness consequence is `[assumed]`.
2. **Silent stale state during D-Bus outages:** no invalidation or freshness
   metadata distinguishes retained values from live observations. `[verified]`
3. **Overlapping refresh requests:** the coordinator launches each request in a
   new task and marks it refreshed immediately, so slow calls can overlap and
   older responses can arrive after newer ones. `[verified]` The possibility of
   out-of-order state regression is `[assumed]`.
4. **Unbounded command backlog:** the D-Bus command queue is unbounded, while
   refresh tasks and UI actions can continue producing commands during a daemon
   outage. `[verified]` Memory growth requires sustained backlog pressure.
   `[assumed]`
5. **Unregistered/no-op refresh coverage:** `gpu_overclock` is registered but
   has no matching refresh callback branch, so its scheduled callback performs
   no D-Bus fetch. `[verified]`
6. **Recovery latency and partial recovery:** after reconnect, fields recover only
   when their individual next poll succeeds; there is no coordinated full refresh.
   `[assumed]`
