# Local patches of the removed `drivers_src/` tree

`drivers_src/` was a vendored copy of the [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers)
kernel modules. It was removed from this repository because nothing here built, packaged or loaded it.
This file preserves the changes that existed only in that copy.

Everything below is a record of what was in the tree and what was measured about it. Statements about
intent are limited to what the commits and the code say; where the record is silent, this file says so.

## The tree that was removed

| Property | Value |
|---|---|
| Upstream project | `tuxedocomputers/tuxedo-drivers`, GPL-2.0-or-later |
| Snapshot version | **v4.18.2** |
| Files | 55 |
| Size | 422,867 bytes (about 16 % of the tracked bytes) |
| History range | `05a0918` (2026-01-19, "Add files via upload") … `e997653` (2026-02-07) |
| Commits touching it | 4: `05a0918`, `5294cf3`, `73b0b5d`, `e997653` |

Version identification was by content, not by a version string: every file was sha256-compared against
the upstream release tarballs. **51 of 55 files were byte-identical to v4.18.2**, and the four files
that differed are exactly the four carrying the local edits listed here. `MODULE_VERSION` is `"0.3.9"`
in both the snapshot and current upstream, so it does not distinguish them.

Nothing in the repository compiled this tree: there is no `build.rs`, no `Makefile`, no `dkms.conf`,
and neither `debian/rules` nor `PKGBUILD` referenced it.

## Patch 1 — `W_CL_KBD_RGB` ioctl (`0x67`)

Adds an ioctl that forwards a 32-bit argument to the Clevo ACPI method `CLEVO_CMD_SET_KB_RGB_LEDS`
(also `0x67`). Introduced by `73b0b5d` ("Implement GUI single instance, crash reporting, and Clevo/ITE
keyboard fixes").

Exact diff of `73b0b5d` against its parent, restricted to `drivers_src/`:

```diff
diff --git a/drivers_src/tuxedo_io/tuxedo_io.c b/drivers_src/tuxedo_io/tuxedo_io.c
index 2f94526..06644e2 100644
--- a/drivers_src/tuxedo_io/tuxedo_io.c
+++ b/drivers_src/tuxedo_io/tuxedo_io.c
@@ -382,6 +382,10 @@ static long clevo_ioctl_interface(struct file *file, unsigned int cmd, unsigned
 			clevo_arg = (CLEVO_CMD_OPT_SUB_SET_PERF_PROF << 0x18) | (argument & 0xff);
 			clevo_evaluate_method(CLEVO_CMD_OPT, clevo_arg, &result);
 			break;
+		case W_CL_KBD_RGB:
+			copy_result = copy_from_user(&argument, (int32_t *) arg, sizeof(argument));
+			clevo_evaluate_method(CLEVO_CMD_SET_KB_RGB_LEDS, argument, &result);
+			break;
 	}
 
 	return 0;
diff --git a/drivers_src/tuxedo_io/tuxedo_io_ioctl.h b/drivers_src/tuxedo_io/tuxedo_io_ioctl.h
index a4fa432..5dda79f 100644
--- a/drivers_src/tuxedo_io/tuxedo_io_ioctl.h
+++ b/drivers_src/tuxedo_io/tuxedo_io_ioctl.h
@@ -29,7 +29,7 @@
 #define MAGIC_READ_UW	IOCTL_MAGIC + 3
 #define MAGIC_WRITE_UW	IOCTL_MAGIC + 4
 
-#define MOD_API_MIN_VERSION "0.2.6" // IMPORTANT: Needs to be updated when a new ioctl is added
+#define MOD_API_MIN_VERSION "0.3.0" // IMPORTANT: Needs to be updated when a new ioctl is added
 
 // General
 #define R_MOD_VERSION		_IOR(IOCTL_MAGIC, 0x00, char*)
@@ -64,6 +64,7 @@
 #define W_CL_FLIGHTMODE_SW	_IOW(MAGIC_WRITE_CL, 0x13, int32_t*)
 #define W_CL_TOUCHPAD_SW	_IOW(MAGIC_WRITE_CL, 0x14, int32_t*)
 #define W_CL_PERF_PROFILE	_IOW(MAGIC_WRITE_CL, 0x15, int32_t*)
+#define W_CL_KBD_RGB		_IOW(MAGIC_WRITE_CL, 0x67, int32_t*)
 
 #ifdef DEBUG
 #define W_TF_BC			_IOW(MAGIC_WRITE_CL, 0x91, uint32_t*)
```

Status of this patch, as recorded at removal time:

- **Intent unknown.** The commit message says "Implement missing `W_CL_KBD_RGB` (0x67) ioctl in
  tuxedo_io driver" and nothing more. No issue, design note or comment explains why the command is
  exposed through `/dev/tuxedo_io` rather than left to the in-kernel `tuxedo_keyboard` LED class.
- **Not accepted upstream.** `W_CL_KBD_RGB` does not exist in `tuxedocomputers/tuxedo-drivers` at
  v4.25.0 or on `main` (checked in `src/tuxedo_io/tuxedo_io_ioctl.h` and `src/tuxedo_io/tuxedo_io.c`;
  GitHub code search over that repository returns 0 hits). The ACPI command itself is upstream
  (`src/clevo_interfaces.h`: `CLEVO_CMD_SET_KB_RGB_LEDS 0x67`), used internally by
  `clevo_keyboard.h` and `clevo_leds.h`; only the ioctl wrapper is local.
- **Not in the maintainer's fork.** See "Verification against the maintainer's fork" below.
- **The daemon ignores the result.** `daemon/src/tuxedo_io.rs:831` issues the request
  (`Self::iow(MAGIC_WRITE_CL, 0x67, …)`), and every caller discards the outcome:
  `daemon/src/hardware_control.rs:1747` (`if let Ok(_) = io.set_clevo_keyboard_color(…)`),
  `:1772` (`let _ = io.set_clevo_keyboard_brightness(…)`) and `:1801-1910`
  (`let _ = io.set_clevo_keyboard_mode(…)` in each effect branch). Each of those paths then writes
  the same state through the `tuxedo_keyboard` sysfs attributes. Upstream's `clevo_ioctl_interface()`
  has no `default:` case and still ends in `return 0;`, so the request reports success and does
  nothing when the module does not implement it.

The patch is against v4.18.2 and has not been rebased since; it does not apply to the fork's current
tree (v4.22.3).

## Patch 2 — `FAN_ON_MIN_SPEED_PERCENT` 25 → 0

Three one-line edits, each lowering the driver's minimum "fan on" speed floor from 25 % to 0.

`tuxedo_io.c` (the same file also carries Patch 1 and the description change below; only the
`FAN_ON_MIN_SPEED_PERCENT` hunk of its v4.18.2 diff is reproduced here):

```diff
--- a/drivers_src/tuxedo_io/tuxedo_io.c
+++ b/drivers_src/tuxedo_io/tuxedo_io.c
@@ -47,7 +47,7 @@
 
 #define NB01_FAN_SPEED_MAX 0xff
 #define NB02_FAN_SPEED_MAX 0xc8
-#define FAN_ON_MIN_SPEED_PERCENT 25
+#define FAN_ON_MIN_SPEED_PERCENT 0
 
 // Initialized in module init, global for ioctl interface
 static u32 id_check_clevo;
```

`tuxedo_nb05_fan_control.c`:

```diff
--- a/drivers_src/tuxedo_nb05/tuxedo_nb05_fan_control.c
+++ b/drivers_src/tuxedo_nb05/tuxedo_nb05_fan_control.c
@@ -29,7 +29,7 @@
 
 #define FAN_SET_RPM_MAX 54
 #define FAN_SET_DUTY_MAX 0xb8
-#define FAN_ON_MIN_SPEED_PERCENT 25
+#define FAN_ON_MIN_SPEED_PERCENT 0
 #define FAN_SET_RPM_HIGHTEMP 15
 #define FAN_SET_DUTY_HIGHTEMP ((FAN_SET_RPM_HIGHTEMP * FAN_SET_DUTY_MAX) / FAN_SET_RPM_MAX)
```

`tuxedo_tuxi_fan_control.c`:

```diff
--- a/drivers_src/tuxedo_tuxi/tuxedo_tuxi_fan_control.c
+++ b/drivers_src/tuxedo_tuxi/tuxedo_tuxi_fan_control.c
@@ -29,7 +29,7 @@
 #include "tuxi_acpi.h"
 
 #define FAN_SET_DUTY_MAX 255
-#define FAN_ON_MIN_SPEED_PERCENT 25
+#define FAN_ON_MIN_SPEED_PERCENT 0
 
 struct driver_data_t {
 	struct platform_device *pdev;
```

Status of this patch, as recorded at removal time:

- **Unrecorded in this repository.** It arrived inside `05a0918` ("Add files via upload"); no commit in
  LapSphere mentions it, and no commit message explains the value. It is not possible to tell from
  history whether it was an intentional local change or carried over from another tree.
- **Effect.** The three defines are read by the clamp blocks in the same files (for example
  `tuxedo_io.c` around lines 339-342, 533-536 and 800; `tuxedo_nb05_fan_control.c` at 98-101, 137-140,
  160-163, 206-209, 238-241; `tuxedo_tuxi_fan_control.c` at 115-118, 191-194). With the constant at 0
  those conditions are never true, so the clamps no longer raise a low commanded value; the blocks
  themselves remain compiled.
- **Present in the maintainer's fork.** See below.

## Other local edits (recorded for completeness)

| Edit | Where | Commit | Note |
|---|---|---|---|
| `MODULE_DESCRIPTION` "Hardware interface for TUXEDO laptops" → "…for Uniwill/Clevo laptops" | `drivers_src/tuxedo_io/tuxedo_io.c:37` | `5294cf3`, re-worded by `e997653` | Cosmetic; reaches `modinfo` only. The maintainer's fork keeps the upstream string. |
| `MOD_API_MIN_VERSION` `"0.2.6"` → `"0.3.0"` | `drivers_src/tuxedo_io/tuxedo_io_ioctl.h:32` | `73b0b5d` | The constant is defined but never referenced in the tree; `R_MOD_VERSION` returns `THIS_MODULE->version` (`MODULE_VERSION`, "0.3.9"). The bump changes no reported value. |

## Verification against the maintainer's fork

Repository: <https://github.com/weter11/tuxedo-drivers> — a fork of `tuxedocomputers/tuxedo-drivers`.
At the time of removal its default branch `main` was at `a9530cbc` (2026-08-23, "chore: bump debian
version to 4.22.3"), last pushed 2026-09-12, i.e. **v4.22.3**.

**Question 1 — does the fork contain the fan floor change? Yes.**

| File on the fork | Line | Value |
|---|---|---|
| [`src/tuxedo_io/tuxedo_io.c`](https://github.com/weter11/tuxedo-drivers/blob/a9530cbc5b83741f87d7b1d79c050cb4a3d7038c/src/tuxedo_io/tuxedo_io.c#L50) | 50 | `#define FAN_ON_MIN_SPEED_PERCENT 0` |
| [`src/tuxedo_nb05/tuxedo_nb05_fan_control.c`](https://github.com/weter11/tuxedo-drivers/blob/a9530cbc5b83741f87d7b1d79c050cb4a3d7038c/src/tuxedo_nb05/tuxedo_nb05_fan_control.c#L32) | 32 | `#define FAN_ON_MIN_SPEED_PERCENT 0` |
| [`src/tuxedo_tuxi/tuxedo_tuxi_fan_control.c`](https://github.com/weter11/tuxedo-drivers/blob/a9530cbc5b83741f87d7b1d79c050cb4a3d7038c/src/tuxedo_tuxi/tuxedo_tuxi_fan_control.c#L32) | 32 | `#define FAN_ON_MIN_SPEED_PERCENT 0` |

Introduced by commit [`899801d3`](https://github.com/weter11/tuxedo-drivers/commit/899801d3af990d3e0c15579efaf4bff32a89e77c)
("feat: Remove min fan speed and add GitHub Actions for deb builds", 2026-01-08), on branch
`feat/remove-min-fan-speed-and-add-ci-1335973093691059155`. That commit also comments out the clamp
blocks in the same three files; the vendored copy here kept them compiled, which is equivalent while
the constant is 0.

**Question 2 — does the fork contain `W_CL_KBD_RGB`? No.**

The string does not occur in `src/tuxedo_io/tuxedo_io_ioctl.h` or `src/tuxedo_io/tuxedo_io.c` at the
tips of the fork's branches (`main` and `copilot/fixclevo-acpi-failcache` = `a9530cbc`,
`feat/remove-min-fan-speed-and-add-ci-1335973093691059155` = `899801d3`,
`fix/clevo-acpi-failcache` = `448d78cf`); neither file contains `KBD` or `0x67` in that position.
GitHub code search restricted to the fork returns 0 results.

**What this verification did not cover.** The fork's full history was not scanned: a branch that once
carried the ioctl and was deleted would not be visible through the API calls made here. The search was
over branch tips by direct file fetch, not over all commits. No build of the fork was attempted.
