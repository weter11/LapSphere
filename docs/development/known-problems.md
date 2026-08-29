# Known Problems (Backlog)

Seeded from PR #92's hardware-detection deep-dive. Read
`docs/architecture/invariants.md` and
the hardware-detection source for full detail. This file contains
pointers and priorities, not the full analysis.

## Priority framing

GPUs, Wi-Fi chips, storage, and batteries are essentially never hot-swapped on
this hardware. Gamepads (USB/Bluetooth) are hot-swapped constantly. Therefore,
the identity-duplication pattern below is a live user-facing bug for gamepads;
the same root cause elsewhere is tracked as dormant until proven otherwise.
`[verified]`

## P0 — user-facing, active

### BUG-1: Duplicate connected gamepads

- **Where:** `daemon/src/hardware_detection.rs::get_gamepad_info()`
- **Invariant violated:** `docs/architecture/invariants.md` #1 — current-pass
  duplicate suppression is UID equality, not physical-device equality.
- **Mechanism:** Sibling `inputN`/event-child nodes can yield different derived
  UIDs because serial, path, sysfs fallback, or `device/uniq` availability
  differs. `seen_uids` only deduplicates within one call; there is no
  cross-call identity registry. `[verified]`
- **Status:** Diagnosed, not fixed. A real cross-pass identity registry or
  normalized identity key is needed; do not add another gamepad-only special
  case. `[assumed]`

## P2 — same root cause, dormant given current hardware-swap frequency

Tracked from the hardware-detection deep-dive’s “Places where separate
enumeration passes can disagree” section. Revisit when a generalized
identity-registry fix is designed for BUG-1.

- **GPU (NVIDIA):** Idle-metrics cache keyed by numeric index; index
  reassignment could attach stale metrics to a different GPU. `[verified]`
- **GPU (Intel/AMD DRM):** `cardN` iteration index is used as identity and can
  change if cards are added or removed. `[verified]`
- **Wi-Fi:** Identity is interface name only; rename or recreation splits rate
  history. `[verified]`
- **Storage:** Identity is `/dev/<block-name>`; name reuse after replacement
  can misattribute rate history. `[verified]`
- **Battery / hwmon association:** Several functions return the first match
  rather than reconciling all candidates; multiple sources can disagree across
  passes. `[verified]`

## Additional daemon lifecycle candidates

These are separate from the hardware-detection identity backlog and remain
investigation candidates from the daemon lifecycle deep-dive.

1. Missing scheduler shutdown/join and retained job closures. `[assumed]`
2. Unguarded concurrent `GetGpuInfoFull` blocking tasks. `[assumed]`
3. GPU-index offset entries retained across topology changes. `[assumed]`
4. Backlog growth in the unbounded scheduler command channel. `[assumed]`
5. One temporary shutdown thread per non-systemd D-Bus shutdown request.
   `[assumed]`

## Open architectural question (not a bug)

- **`nvidia/` orphan directory:** Provides GPU VRAM/hotspot temperature through
  an undocumented NVIDIA interface plus other driver features, but is not wired
  into the workspace build. Track the integration decision separately; see
  `docs/architecture/modules.md`. `[verified]`

## Maintenance

Add an entry when an invariant violation is confirmed but not yet fixed.
Remove or move it to done when fixed and its regression test lands. This file
tracks problems, not design discussion; record a chosen fix approach in an ADR.
