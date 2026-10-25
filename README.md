# LapSphere

A hardware control and monitoring application for Uniwill and Clevo laptops on Linux.

A root daemon owns the hardware and an egui GUI talks to it over D-Bus. The GUI never writes to hardware itself; per-user settings, profiles and panel layout live under `~/.config/lapsphere/`.

---

## Features

### Hardware monitoring

- **CPU** — per-core frequency, load and temperature; package temperature; power from RAPL, `amdgpu` or `zenpower` when present; governor, boost, SMT, AMD/Intel P-State status and EPP
- **Memory** — used, available and total; type and speed come from `dmidecode` when it is installed
- **GPU** — core and memory clocks, load, power, voltage, temperature, hotspot and VRAM temperatures, VRAM usage, runtime power state and per-process GPU memory. NVIDIA values come from NVML, the RM ioctl path and `libnvidia-api.so.1`; the daemon does not query a suspended dGPU
- **Battery** — charge, voltage, current, status, power (voltage × current) and charge thresholds; health and capacity are under Settings → Hardware info
- **WiFi** — SSID, signal, channel, width and frequency, PHY rates, throughput, temperature, driver and firmware versions
- **Storage** — read/write speed, IOPS and temperature per block device, plus mount usage
- **Fans** — speed, temperature and control mode for system fans (via `tuxedo_io`) and NVIDIA GPU fans
- **Gamepads** — connection type, battery level and power status; devices with a stable ID are remembered in `settings.json`

### CPU tuning

- Scaling governor and frequency min/max limits
- CPU boost and SMT toggles
- AMD P-State (active / passive / guided) and Intel P-State (active / passive)
- Energy Performance Preference (EPP)
- Uniwill TDP control (PL1 / PL2 / PL4) within the ranges the firmware reports
- Vendor performance profile — Uniwill: `power_save` / `enthusiast`, plus `overboost` when the firmware reports it; Clevo: `quiet` / `power_saving` / `performance` / `entertainment`

### GPU tuning (NVIDIA)

- Locked core clock range, core offset, memory offset and power limit
- **Dynamic offset** — only in Advanced mode with manual clocks enabled. On every `gpu_overclock` poll the daemon computes an offset from live temperature, frequency and power (drain and power control zones, optional critical-temperature range), rounds it to a configurable step and writes it only when it changed. The offset is cleared when the GPU leaves P0 or the daemon may not query it
- NVIDIA fan control (manual per fan or auto)
- PRIME profile switching (on-demand / nvidia / intel) through `optimus-manager` when it exists, otherwise `prime-select`

### Keyboard backlight

- Single-zone, 3-zone, 4-zone and per-key RGB keyboards, detected from the `tuxedo_keyboard` LED sysfs (brightness-only keyboards are detected but expose only brightness)
- Modes: Static color, Multiple zones, Per-key, Breathe, Cycle, Dance, Flash, Random Color, Tempo, Wave
- Live preview without saving to the profile

### Fan control

- Per-fan curves with 2–16 points, edited graphically or entered numerically
- The daemon evaluates the curves and writes them through `/dev/tuxedo_io`; disabling custom curves sets the fans back to auto
- Requires `tuxedo-drivers` (see [Kernel driver](#kernel-driver))

### Battery charge control

- Charge type (Standard / Custom) and start/end thresholds through `charge_type` and `charge_control_{start,end}_threshold` on `BAT0` / `BAT1`
- Available values are read from `charge_control_*_available_thresholds` when present, otherwise from built-in fallback lists
- The attributes and the Clevo legacy / CC4 flexicharger logic live in the kernel module provided by `tuxedo-drivers` (installed separately); the daemon only reads and writes the sysfs files

### Profiles

- Named profiles holding CPU, GPU, keyboard, screen brightness and fan settings; the built-in Standard profile can be reset but not deleted
- Switching applies the profile immediately over D-Bus, and the active profile is applied again when the GUI starts
- Persisted to `~/.config/lapsphere/profiles.json`

### Panel mode (experimental)

The panel is a compact column of the same statistics the Statistics page shows, drawn by the GUI's window and reached with the `📈 Panel` button, `lapsphere --panel`, or `lapsphere --toggle-panel` on a running GUI. Its settings (row visibility, width, background opacity, stacking, hide from the taskbar and pager) are stored in `panel.json`; the hotkey and taskbar handling are X11 only. The frame rows read a shared-memory segment written by the Vulkan layer in `spike/vk-layer/`, which the packages neither build nor install, so those rows show `—` until it is built and enabled separately.

### Other

- System tray (ksni) with a Profiles submenu, Show Window, Statistics and Quit
- Configurable per-section polling rates
- Log viewer with level filters, search, pause, copy, and a button to open the crash-report folder
- Crash reports written to `~/.config/lapsphere/crash_<timestamp>.log`
- Autostart: Settings → Enable autostart writes `~/.config/autostart/io.lapsphere.LapSphere.desktop` with `lapsphere --tray`; disabling it writes the same file with `Hidden=true`
- Update check: on startup the GUI asks the GitHub releases API and shows a notice when the latest tag differs from the running version; Settings → About shows the changelog and an "Open Download Page" button (no automatic install)
- Keyboard shortcuts: `Ctrl+1`–`Ctrl+4` switch pages, `F1` toggles the NVIDIA overclocking help
- Webcam toggle (Clevo interface only)

---

## Architecture

```
┌──────────────────────────────────┐
│         lapsphere (GUI)          │  runs as a regular user
│  egui frontend, D-Bus client     │
└────────────┬─────────────────────┘
             │ D-Bus (io.lapsphere.Control)
             │ system bus
┌────────────▼─────────────────────┐
│      lapsphere-daemon            │  runs as root
│  hardware polling, sysfs writes  │
│  NVML, tuxedo_io ioctls, NVAPI   │
└──────────────────────────────────┘
```

The daemon owns the system-bus name `io.lapsphere.Control` (object path `/io/lapsphere/Control`, interface of the same name) and runs three polling jobs: `hardware_monitor`, `fan_control` and `gpu_overclock`. Their defaults are 1000 / 2000 / 1000 ms, read from `~/.config/lapsphere/settings.json`, and the GUI updates them over D-Bus. The GUI talks to the daemon only over D-Bus; apart from its own configuration, crash reports and autostart files it writes nothing.

The bus policy (`data/io.lapsphere.Control.conf`) lets only root own the name and lets any user call it. The activation file (`data/io.lapsphere.Control.service`) starts `/usr/bin/lapsphere-daemon` as root.

**Single instance.** The GUI requests the session-bus name `io.lapsphere.Gui`; a second instance prints a message and exits.

**Daemon unit.** `data/lapsphere-daemon.service` is a `Type=dbus` unit (`BusName=io.lapsphere.Control`) that runs `lapsphere-daemon` without flags, with `Restart=on-failure`, `CPUQuota=50%`, `MemoryMax=768M`, `TasksMax=256` and `MALLOC_ARENA_MAX=2`. Packages install it to `/usr/lib/systemd/system/` but do not enable it.

**Flags.**

- `--gui` — the daemon spawns the GUI as the user given by `SUDO_UID`, or `PKEXEC_UID` if that is not set (so start it with `sudo` or `pkexec`), forwarding every other argument to it. When that GUI process exits, the daemon shuts down.
- `--tray` — on the GUI, start with the window hidden. On the daemon it is only forwarded to the GUI; without `--gui` it merely suppresses the startup statistics print.
- `--panel` — on the GUI, start in panel mode.
- `--toggle-panel` — on the GUI, ask the already-running instance to show or hide its panel; exits non-zero if no GUI answers (Linux only).
- Without flags the daemon requires root; as a regular user it prints limited statistics and exits with status 1.

When the GUI exits it sets all fans to auto and asks the daemon to shut down; the daemon ignores that request when it is managed by systemd.

---

## Requirements

### Runtime

- Linux with a D-Bus system bus
- `tuxedo-drivers` for `/dev/tuxedo_io` — required for fan control, TDP profiles, the webcam toggle and the LED interface (see [Kernel driver](#kernel-driver))
- An X11 or Wayland session for the GUI
- Optional helpers, each used when present: `lscpu` (CPU topology), `dmidecode` (memory type/speed), `iw` and `ethtool` (WiFi link and driver info), `lspci` (WiFi controller)
- NVIDIA driver with `libnvidia-ml.so.1` for GPU tuning and NVIDIA statistics; `libnvidia-api.so.1` adds the NVAPI hotspot/VRAM temperature and voltage queries
- `optimus-manager` or `prime-select` for PRIME switching, `xdg-open` and `systemctl` for the crash-report folder button and the Settings restart button

Libraries are loaded at runtime: NVML through `libloading`, NVAPI by name (`libnvidia-api.so.1`). `/dev/tuxedo_io`, `/dev/nvidia*`, `/dev/dri/*` and sysfs (`/sys/class/...`, `/sys/devices/system/cpu/...`, `/sys/class/dmi/id/...`) are the device and kernel interfaces the code uses.

### Build

- Rust toolchain (edition 2021, workspace resolver 2)
- A C compiler, for mimalloc's vendored C library in the GUI
- `pkg-config` and OpenSSL development headers (`ureq` → `native-tls` → `openssl-sys`)

`cargo tree --target x86_64-unknown-linux-gnu` resolves no GTK, glib, gdk, atk or libdbus crate: D-Bus is `zbus`, and the Wayland/X11 bindings (`wayland-sys`, `x11-dl`, `xkbcommon-dl`) load their libraries with `dlopen`, so no Wayland, X11 or xkbcommon development package is needed to build. The only build script with a hard `pkg-config` requirement is `openssl-sys`; `libmimalloc-sys` uses the C compiler. `debian/control`, `PKGBUILD` and the CI workflow still install `libdbus-1-dev`, the GTK3/libadwaita/pango/cairo/gdk-pixbuf/atk/graphene packages and `libwayland-dev`/`libxkbcommon-dev`, but those are not required by the crate graph.

---

## Kernel driver

Fan control, TDP profiles, the webcam toggle and the keyboard LED sysfs interface come from TUXEDO Computers' [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers) kernel modules, which provide `/dev/tuxedo_io` and the `tuxedo_keyboard` LED class devices.

The module is provided by the `tuxedo-drivers` kernel modules (installed separately). This repository contains no `Makefile`, `dkms.conf` or packaging rule that builds or installs a module, and neither the Debian package nor the `PKGBUILD` ships one; the module therefore has to be installed separately, from TUXEDO's tuxedo-drivers, which the Debian package lists under `Recommends`.

The maintainer's fork ([weter11/tuxedo-drivers](https://github.com/weter11/tuxedo-drivers)) is not part of the upstream project. One change in it is relevant here: its commit `899801d3` lowers the fan-on floor by setting `FAN_ON_MIN_SPEED_PERCENT` to `0` in `src/tuxedo_io/tuxedo_io.c:50`, `src/tuxedo_nb05/tuxedo_nb05_fan_control.c:32` and `src/tuxedo_tuxi/tuxedo_tuxi_fan_control.c:32`. That change is not in upstream tuxedo-drivers and is not in this repository. See [`docs/archive/drivers_src-local-patches.md`](docs/archive/drivers_src-local-patches.md).

Without `/dev/tuxedo_io` the daemon logs `/dev/tuxedo_io not available - some features will be disabled` (debug level) and keeps running: monitoring and the sysfs-based CPU paths are unaffected, while fan control, TDP profiles and the webcam toggle return `tuxedo_io not available`.

---

## Building

```bash
cargo build --release --all
```

Cargo produces two binaries in `target/release/`:

- `lapsphere` — the GUI
- `lapsphere-daemon` — the daemon

CI builds a Debian package and an Arch package on every push to `main`/`lapsphere` and on pull requests; download them as the `lapsphere-ubuntu-24.04-deb` and `lapsphere-arch-pkg` artifacts of a workflow run.

---

## Installation (Debian/Ubuntu)

A `debian/` directory is included for building a `.deb`:

```bash
dpkg-buildpackage -us -uc -b
sudo dpkg -i ../lapsphere_*.deb
```

This installs:

- `/usr/bin/lapsphere` and `/usr/bin/lapsphere-daemon`
- D-Bus policy `/usr/share/dbus-1/system.d/io.lapsphere.Control.conf`
- D-Bus activation file `/usr/share/dbus-1/system-services/io.lapsphere.Control.service`
- systemd unit `/usr/lib/systemd/system/lapsphere-daemon.service`
- desktop entry and icon under `/usr/share/applications` and `/usr/share/icons/hicolor/scalable/apps`, and the autostart entry `/etc/xdg/autostart/io.lapsphere.LapSphere.desktop` (`Exec=lapsphere --tray`)

The package depends on `dbus`, `policykit-1`, `libxkbcommon-x11-0`, `dmidecode`, `pciutils`, `ethtool` and `iw`, and recommends `tuxedo-drivers`.

### Arch Linux

The root `PKGBUILD` builds the same two binaries and installs the same files:

```bash
makepkg -si
```

`tuxedo-drivers` is not part of this package and has to be installed separately here as well, exactly as described under [Kernel driver](#kernel-driver); without it `/dev/tuxedo_io` and the `tuxedo_keyboard` LED devices are missing.

---

## Running

### Recommended (through the daemon)

```bash
sudo lapsphere-daemon --gui
```

The daemon starts, owns `io.lapsphere.Control`, and launches the GUI as the invoking user. It exits when that GUI exits.

### Manual

```bash
# terminal 1 (root)
sudo lapsphere-daemon

# terminal 2 (user)
lapsphere
```

### Start hidden, or in the panel

```bash
lapsphere --tray      # window hidden, use the tray icon to show it
lapsphere --panel     # start in panel mode
lapsphere --toggle-panel   # toggle the panel of the running GUI
```

---

## Configuration

All user configuration lives in `~/.config/lapsphere/`:

| File | Contents |
|---|---|
| `settings.json` | Theme, font size, autostart, CPU scheduler name, statistics sections (visibility, order, per-section polling rates), tuning section order, battery settings, log limit and trace filter, remembered gamepads |
| `profiles.json` | All profiles and the active profile name; the previous file is kept as `profiles.json.bak` |
| `tray.json` | `start_minimized` and `tray_enabled` (minimize on close), plus a schema `version` |
| `panel.json` | Panel width, background opacity, stacking, hide-from-taskbar, last position, hotkey, frame window and refresh rate, per-row visibility, plus a schema `version` |
| `config.json` | Legacy single-file configuration, read only when `settings.json` or `profiles.json` is missing |
| `crash_<timestamp>.log` | GUI panic reports (backtrace and panic message) |

`settings.json` is written atomically; a corrupt `tray.json` or `panel.json` is moved aside to `tray.json.corrupt` / `panel.json.corrupt` instead of being deleted, and an existing pre-split backup is kept as `settings.json.pre-tray-split`.

The daemon reads only the poll rates from `$HOME/.config/lapsphere/settings.json` (root's `$HOME` under `sudo`) and receives live updates from the GUI over D-Bus; everything else in that directory belongs to the GUI.

---

## Project structure

```
Cargo.toml            workspace (members: common, daemon, gui)
common/               shared types (CpuInfo, GpuInfo, Profile, AppConfig, …)
daemon/               root daemon
  src/main.rs         CLI flags, startup, the three poll jobs, dynamic GPU offset
  src/dbus_interface.rs   exported D-Bus methods
  src/hardware_detection.rs  read paths and capability detection
  src/hardware_control.rs    write paths (sysfs, NVML, tuxedo_io)
  src/tuxedo_io.rs    ioctl wrapper for /dev/tuxedo_io
  src/battery_control.rs
  src/daemon_settings.rs
  src/gpu_activity.rs
  src/polling_scheduler.rs
gui/                  egui GUI
  src/main.rs         entry point, single-instance guard, --tray/--panel/--toggle-panel
  src/app.rs          state, config load/migration, update loop
  src/dbus_client.rs  D-Bus command dispatch
  src/system_tray.rs  ksni tray
  src/tray_config.rs  tray.json
  src/gamepad_registry.rs
  src/pages/          statistics, profiles, tuning, settings
  src/panel/          panel mode: config, layout, rows, rendering, X11 EWMH, frame reader
  src/widgets/        fan curve editor
data/                 D-Bus policy and activation file, systemd unit, desktop entry, icon
debian/               Debian packaging
spike/vk-layer/       standalone Vulkan layer that feeds the panel's frame rows
docs/                 architecture, subsystem and development notes
screenshots/          images
PKGBUILD              Arch Linux package recipe
```

---

## Hardware support

| Feature | Clevo | Uniwill |
|---|---|---|
| Fan control | yes, up to 3 fans | yes, 1–2 fans |
| Performance profile | `quiet` / `power_saving` / `performance` / `entertainment` | `power_save` / `enthusiast`, plus `overboost` when reported |
| TDP PL1 / PL2 / PL4 | — | yes |
| Keyboard backlight | LED sysfs, plus Clevo ioctl modes | single-zone `rgb:` LED |
| Webcam toggle | yes | — |
| Battery charge thresholds | if the battery exposes `charge_type` (the Clevo module adds it) | only if the battery exposes `charge_type` |

The vendor interface is probed once when `/dev/tuxedo_io` is opened; if neither Clevo nor Uniwill answers, vendor-specific features report themselves as unavailable. NVIDIA features need an NVIDIA GPU with NVML; AMD and Intel iGPUs are read-only (statistics only). Hotspot and memory-sensor temperatures are not exposed by the open kernel module, so the daemon reads them through NVAPI and the RM control API.

Hardware coverage is limited — the features above are gated on what the code probes for, not on a list of validated models — and reports from other models are welcome.

TODO(maintainer): list the laptop models and BIOS versions LapSphere has actually been run on.

---

## Caution for Uniwill

On Uniwill laptops the fan number is not a verified percentage. The fan curves produce 0–100, `hardware_control::set_fan_speed` clamps D-Bus requests to 100, and `TuxedoIo::set_fan_speed` (`daemon/src/tuxedo_io.rs`) passes the number to the `W_UW_FANSPEED` ioctl with a 200 ceiling. The kernel side, `uw_set_fan` in the tuxedo-drivers `tuxedo_io` module, treats it as a raw EC fan level and rejects values above `NB02_FAN_SPEED_MAX` (200); the read path returns the same raw level, which the GUI labels `%`. Nothing converts between that level, RPM and a percentage, and the 200 is the kernel's ceiling rather than 200 %. The unit is therefore unverified: commands of 0–100 reach the EC as levels 0–100, half of the 200 the kernel accepts. Treat the percentages shown for Uniwill fans as approximate, and prefer the automatic fan curve unless you can watch the fans.

---

## Reporting problems

A bug report is useful when it includes:

- the laptop model as the app reports it, from `/sys/class/dmi/id/product_name` (shown under Settings → Hardware info)
- the daemon log: `journalctl -u lapsphere-daemon` when the daemon runs as a systemd service, otherwise the terminal output or the GUI's View Logs tab
- the crash report from `~/.config/lapsphere/crash_<timestamp>.log`, if the GUI crashed
- the LapSphere version shown in Settings → About

---

## Documentation

- [`docs/architecture/`](docs/architecture/) — overview, modules, invariants, decisions
- [`docs/subsystems/`](docs/subsystems/) — daemon lifecycle, GUI state sync, hardware detection, `tuxedo_io`
- [`docs/development/known-problems.md`](docs/development/known-problems.md)
- [`docs/VRAM_DETECTION_TROUBLESHOOTING.md`](docs/VRAM_DETECTION_TROUBLESHOOTING.md) and [`docs/gpu-process-memory-statistics.md`](docs/gpu-process-memory-statistics.md)

---

## Credits

- The `tuxedo-drivers` kernel modules are TUXEDO Computers' [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers), GPL-2.0-or-later (SPDX headers in their source files). No part of that project is vendored in this repository.
- NVIDIA hotspot and memory-sensor temperatures follow the approach of [LACT](https://github.com/ilya-zlobintsev/LACT) by Ilya Zlobintsev (MIT): the open kernel module does not expose them, so the daemon reads them from NVAPI and the RM control API itself. No LACT code is vendored in this repository.

---

## Status

Version 0.1.0 (`Cargo.toml`). There are no git tags and no GitHub releases; the project is early-stage.

---

## Disclaimer

LapSphere is an independent project and is not affiliated with TUXEDO Computers, Clevo or Uniwill. It writes to hardware — fan curves, power limits, clock offsets, charge thresholds and keyboard settings — so a wrong value can affect stability, cooling or the battery. It comes with no warranty; see `LICENSE`.

## License

GPL-2.0 — see `LICENSE` and `debian/copyright`.
