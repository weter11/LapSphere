# Tuxedo ioctl client archaeology

## Scope and evidence

This document covers `daemon/src/tuxedo_io.rs`. The request numbers in
[Request-number reference](#request-number-reference) are this repository's
in-repo ABI reference for the device; the driver sources are no longer vendored
here and come from the `tuxedo-drivers` kernel modules installed separately.
Claims are marked `[verified]` when directly traced and `[assumed]` when
inferred. No fixes are proposed.

## Device lifecycle and interface selection

`TuxedoIo::new()` opens `/dev/tuxedo_io` once with read/write access, detects
the platform interface, detects the fan count, and stores the `File`,
`HardwareInterface`, and count in the object. `[verified]` Every later call
reuses that file descriptor; the device is not opened per ioctl. `[verified]`
`is_available()` only checks path existence and does not prove that opening or
any ioctl will succeed. `[verified]`

Detection first issues general hardware checks `0x05` (Clevo) and `0x06`
(Uniwill), then probes Clevo read sequence `0x10`, then Uniwill read sequence
`0x10`. `[verified]` A successful result of exactly `1` selects the
corresponding interface; otherwise a successful probe selects it, and failure
of all probes returns `HardwareInterface::None` rather than failing
construction. `[verified]` Open failure or fan-count errors during
construction are propagated. `[verified]`

The client manually builds Linux ioctl numbers using an 8-byte pointer size
on 64-bit systems. `[verified]` This matches the header's `char*`/`int32_t*`
argument declarations as represented by `_IOR`/`_IOW`; the Uniwill auto-fan
call is `_IO` and passes an integer argument directly. `[verified]`

## Calls and mapping to `modules.md`/driver contract

The magic values match the header exactly: general `0xEC`, Clevo read
`0xED`, Clevo write `0xEE`, Uniwill read `0xEF`, and Uniwill write `0xF0`.
`[verified]`

| Client operation | ioctl family and sequences | Contract meaning |
|---|---|---|
| Interface/fan detection | general `0x05/0x06`; CL read `0x10..`; UW read `0x10..0x11` | Platform checks and fan probes `[verified]` |
| `get_fan_speed`, `get_fan_temperature` | CL read `0x10..0x12`; UW read `0x10..0x13` | Clevo packed fan info; Uniwill fan speed/temp `[verified]` |
| `set_fan_speed`, `set_fan_auto` | CL write `0x10/0x11`; UW write `0x10/0x11/0x12/0x14` | Manual speed (Uniwill: clears the `0x40` ownership bit through `0x12` first) and return-to-auto `[verified]` |
| TDP reads/writes | UW read `0x18..0x20`; UW write `0x15..0x17` | TDP0..2 current/min/max and setters `[verified]` |
| Performance profiles | UW read `0x21`, mode read `0x14`; CL write `0x15`, UW write `0x18` | Available/current profile and profile setter `[verified]` |
| Webcam | CL read `0x13`, write `0x12` | Webcam state `[verified]` |
| Clevo keyboard | CL write `0x67` | Packed RGB mode/color/brightness command `[verified]` |

## Request-number reference

The table below is the ABI reference kept in this repository: one row per request the client actually
issues, with the call site that builds it. Families are `0xEC` general, `0xED` Clevo read, `0xEE`
Clevo write, `0xEF` Uniwill read, `0xF0` Uniwill write (`daemon/src/tuxedo_io.rs:11-14`); each request
is built by `ior`/`iow` with an 8-byte pointer size (`:164-171`).

| Request | Daemon call site | Purpose |
|---|---|---|
| `_IOR(0xEC, 0x05)` / `_IOR(0xEC, 0x06)` | `:277-278` | Clevo / Uniwill platform check |
| `_IOR(0xED, 0x10)` | `:309` | Clevo fan-info probe (interface detection) |
| `_IOR(0xED, 0x10 + fan)` | `:342`, `:382`, `:454`, `:703` | Clevo fan count, fan speed, read-back, fan temperature |
| `_IOR(0xED, 0x13)` | `:805` | Clevo webcam state |
| `_IOW(0xEE, 0x10)` | `:536` | Clevo fan speed write |
| `_IOW(0xEE, 0x11)` | `:432`, `:592` | Clevo fan auto (before a manual write, and `set_fan_auto`) |
| `_IOW(0xEE, 0x12)` | `:819` | Clevo webcam write |
| `_IOW(0xEE, 0x15)` | `:774` | Clevo performance profile |
| `_IOW(0xEE, 0x67)` | `:831` | Clevo keyboard RGB / effect value — **local patch, not in upstream tuxedo-drivers nor in the maintainer's fork; the daemon ignores the result and repeats the write through sysfs** (see [`docs/archive/drivers_src-local-patches.md`](../archive/drivers_src-local-patches.md)) |
| `_IOR(0xEF, 0x10)` / `_IOR(0xEF, 0x11)` | `:359-363`, `:394` | Uniwill fan count probe and fan speed |
| `_IOR(0xEF, 0x12 + fan)` | `:720` | Uniwill fan temperature |
| `_IOR(0xEF, 0x14)` | `:205`, `:669` | Uniwill mode read (fan-ownership bit, performance profile) |
| `_IOW(0xF0, 0x12)` | `:211` | Uniwill mode write (clears the `0x40` fan-ownership bit) |
| `_IOW(0xF0, 0x10 + fan)` | `:573` | Uniwill fan speed write |
| `_IO(0xF0, 0x14)` | `:609` | Uniwill fan auto (no data argument) |
| `_IOR(0xEF, 0x18 + i)` / `_IOR(0xEF, 0x1b + i)` / `_IOR(0xEF, 0x1e + i)` | `:627`, `:638`, `:649` | Uniwill TDP current / min / max for TDP index `i` |
| `_IOW(0xF0, 0x15 + i)` | `:660` | Uniwill TDP write for TDP index `i` |
| `_IOR(0xEF, 0x21)` | `:746` | Uniwill available performance profiles |
| `_IOW(0xF0, 0x18)` | `:787` | Uniwill performance profile write |

The daemon does not issue the header's Clevo flight-mode/touchpad calls or its additional Uniwill
mode/fan capability calls. `[verified]` (call sites in `daemon/src/tuxedo_io.rs` against the archived
patch record); the client therefore covers a subset of the driver's published interface, plus the
local `0x67` keyboard request described above.

## Error and state assumptions

All ioctl wrappers convert a negative libc result through `Errno` into
`anyhow`; call-specific validation rejects unsupported interfaces and invalid
fan/profile IDs. `[verified]` Detection intentionally treats probe errors as
negative evidence, while operational read/write errors propagate to callers.
`[verified]` Clevo manual control uses a process-global atomic to disable auto
mode once and re-enables it when `set_fan_auto()` succeeds. `[verified]`

On a board with universal EC fan control the driver's `uw_set_fan()` reads
`0x0751` and returns success without touching the EC while bit `0x40` is set,
so a manual duty is dropped silently. The bit is sticky: `uw_init_fan()` clears
it, but only after that same check has already passed. `set_fan_speed()`
therefore clears it first with a `0x14`/`0x12` read-modify-write that keeps the
profile bits (`0xa0`/`0x10`), and a failed read or clear is propagated instead
of being reported as a successful write. `uw_set_fan_auto()` clears the bit on
its own. `[verified]`

If `/dev/tuxedo_io` is missing, `is_available()` returns false, but
`TuxedoIo::new()` returns the underlying open error; there is no internal
mock, retry, or deferred open. `[verified]` An existing device node with an
unsupported platform remains constructible with `None` and zero fans.
`[verified]`

## Ranked findings

1. **Open-once assumption (high):** the object owns one long-lived descriptor;
   device replacement or driver unload is surfaced only as later ioctl errors.
   `[verified]`
2. **Availability is only a path check (high):** callers must distinguish
   `is_available()` from successful open/interface detection. `[verified]`
3. **Contract subset (medium):** the client implements fan, TDP, profile,
   webcam, and keyboard operations but not every header-declared mode or
   capability ioctl. `[verified]`
4. **Architecture-specific encoding (medium):** request numbers hard-code an
   8-byte pointer size, matching the intended 64-bit Linux deployment.
   `[verified]`
