# Tuxedo ioctl client archaeology

## Scope and evidence

This document covers `daemon/src/tuxedo_io.rs` and the matching declarations
in `drivers_src/tuxedo_io/tuxedo_io_ioctl.h`. Claims are marked `[verified]`
when directly traced and `[assumed]` when inferred. No fixes are proposed.

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
| `set_fan_speed`, `set_fan_auto` | CL write `0x10/0x11`; UW write `0x10/0x11/0x14` | Manual speed and return-to-auto `[verified]` |
| TDP reads/writes | UW read `0x18..0x20`; UW write `0x15..0x17` | TDP0..2 current/min/max and setters `[verified]` |
| Performance profiles | UW read `0x21`, mode read `0x14`; CL write `0x15`, UW write `0x18` | Available/current profile and profile setter `[verified]` |
| Webcam | CL read `0x13`, write `0x12` | Webcam state `[verified]` |
| Clevo keyboard | CL write `0x67` | Packed RGB mode/color/brightness command `[verified]` |

The header also declares Clevo flight-mode/touchpad calls and additional
Uniwill mode/fan capability calls that this Rust client does not invoke.
`[verified]` The client therefore covers a subset of the documented external
contract, with extra keyboard handling at the documented `0x67` sequence.

## Error and state assumptions

All ioctl wrappers convert a negative libc result through `Errno` into
`anyhow`; call-specific validation rejects unsupported interfaces and invalid
fan/profile IDs. `[verified]` Detection intentionally treats probe errors as
negative evidence, while operational read/write errors propagate to callers.
`[verified]` Clevo manual control uses a process-global atomic to disable auto
mode once and re-enables it when `set_fan_auto()` succeeds. `[verified]`

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
