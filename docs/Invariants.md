# Invariants

> STATUS: PARTIALLY SEEDED — one entry verified from source, rest not yet populated
> Agents: an entry without a "Verified" note is not confirmed — do not treat it as authoritative without checking. A STATUS of NOT YET POPULATED elsewhere in this scaffold is not an error.

## Format for each entry
Rule — Enforced where — Backed by test? (link it) — Verified [date/commit] or Assumed

## Seeded entries

### 1. Gamepad UID identity is per-physical-device, not per-sysfs-node
**Rule:** A physical input device must resolve to exactly one `uid` regardless of which sysfs `inputN` sub-node is being enumerated; enumeration must be idempotent across sibling nodes belonging to the same device.
**Enforced where:** `daemon/src/hardware_detection.rs::get_gamepad_info()` — uid resolved per-`inputN` via `ID_SERIAL_SHORT` > `ID_SERIAL` > `ID_PATH` > raw sysfs path fallback, then conditionally overridden by `device/uniq` if present.
**Backed by test?** No test found as of this writing.
**Status:** Verified 2026-08-29 by direct source read — likely root cause of the "duplicate connected gamepads" bug, since sibling `inputN` nodes for one physical controller can have inconsistent udev property availability, yielding two different `uid`s for the same device.

## Maintenance
Add an invariant only once it's traced in source, not from a doc or a guess. Every invariant added here should be backed by a test/assertion in the same change that adds it.
