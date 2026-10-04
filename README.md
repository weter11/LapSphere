# LapSphere

A hardware control and monitoring application for Uniwill/Clevo laptops on Linux.

LapSphere provides a modern GUI for tuning CPU and GPU performance, configuring fan curves, controlling keyboard backlighting, managing battery charge thresholds, and viewing live system statistics — all through a root daemon with a per-user GUI frontend.

---

## Features

### Hardware Monitoring (Statistics)
- **CPU** — per-core frequency, load, temperature, power draw (RAPL, amdgpu, zenpower), governor, boost state, AMD/Intel P-State, EPP
- **Memory** — used/available/total, type and speed (via dmidecode)
- **GPU** — frequency, memory frequency, temperature, hotspot/VRAM temps (via NVAPI), load, power, voltage, clock offsets, runtime power state, VRAM usage and per-process GPU memory. The daemon does not wake a suspended dGPU
- **Battery** — charge %, voltage, current, status, power (computed as voltage × current), charge start/end thresholds; health and capacity are shown under Settings → Hardware info
- **WiFi** — SSID, signal level, channel, PHY rate, actual throughput, temperature
- **Storage** — read/write speed, IOPS, temperature, mount usage
- **Fans** — speed, temperature per fan, control mode
- **Gamepads** — connection type, battery level, power status; gamepads with a stable identifier are remembered across sessions (`remembered_gamepads` in `settings.json`)

### CPU Tuning
- Scaling governor selection
- Frequency min/max limits
- CPU boost / Turbo toggle
- SMT / Hyperthreading toggle
- AMD P-State mode (active / passive / guided)
- Intel P-State mode (active / passive)
- Energy Performance Preference (EPP)
- Uniwill TDP control (PL1 / PL2 / PL4) with per-device ranges
- TDP performance profile — Uniwill: `power_save` / `enthusiast` (+ `overboost` when the firmware reports support); Clevo: `quiet` / `power_saving` / `performance` / `entertainment`

### GPU Tuning (NVIDIA)
- GPU core locked clock range
- Core clock offset (static, applied immediately via NVML)
- Memory clock offset
- Power limit
- **Advanced dynamic overclocking** — requires Advanced mode with manual clock control enabled; on each `gpu_overclock` poll the daemon recalculates the offset from live temperature, frequency, and power draw (configurable drain/power/critical-temp control zones, smart rounding) and writes it only when it changes. Skipped while the dGPU is suspended
- NVIDIA fan speed control (manual per-fan or auto)
- PRIME profile switching (on-demand / nvidia / intel) via optimus-manager or prime-select

### Keyboard Backlight
- Supports single-zone RGB, 3-zone RGB, 4-zone RGB, per-key RGB, white-only keyboards
- Modes: Static color, multiple zones, per-key painting, Breathe, Cycle, Dance, Flash, Random Color, Tempo, Wave
- Live preview without saving

### Fan Control
- Custom fan curves per fan with a graphical editor (2–16 points)
- Drag points or edit numerically
- Curves are applied by the daemon through `/dev/tuxedo_io`; disabling custom curves restores automatic fan control

### Battery Charge Control
- Charge type (Standard / Custom) and charge start/end thresholds via the `charge_type` and `charge_control_{start,end}_threshold` sysfs files of `BAT0`/`BAT1`
- Available threshold values are read from `charge_control_*_available_thresholds` when present, with built-in fallback lists
- Clevo legacy and CC4 flexicharger interfaces are implemented by the vendored kernel module sources in `drivers_src/`, not by the daemon

### Profile System
- Multiple named profiles (create, edit, delete; the built-in Standard profile can be reset but not deleted), each storing CPU, GPU, keyboard, screen brightness, and fan settings
- Instant apply on switch (hardware settings sent to daemon via DBus); the current profile is also applied at GUI startup
- Profiles persist to `~/.config/lapsphere/profiles.json`

### Other
- System tray (ksni) with Profiles submenu, Show Window, Statistics, and Quit
- Configurable per-section polling rates
- Log viewer with level filtering, search, pause, and copy; button to open the crash-reports folder
- Crash reports written to `~/.config/lapsphere/crash_<timestamp>.log`
- Auto-start: Settings → Enable autostart writes `~/.config/autostart/io.lapsphere.LapSphere.desktop` (`lapsphere --tray`); disabling it writes an override with `Hidden=true`. Packages also ship `/etc/xdg/autostart/io.lapsphere.LapSphere.desktop` with `--tray`
- Update checker — on GUI startup queries the GitHub releases API and shows a notice when the latest tag differs from the running version; Settings → About offers the changelog and "Open Download Page" (no automatic install)
- Keyboard shortcuts: `Ctrl+1`–`Ctrl+4` switch pages, `F1` opens the NVIDIA overclocking help
- Webcam toggle (Clevo interface only)

---

## Architecture

```
┌──────────────────────────────────┐
│         lapsphere (GUI)          │  runs as regular user
│  egui frontend, DBus client      │
└────────────┬─────────────────────┘
             │ DBus (io.lapsphere.Control)
             │ system bus
┌────────────▼─────────────────────┐
│      lapsphere-daemon            │  runs as root
│  Hardware polling, sysfs writes  │
│  NVML, tuxedo_io ioctl, NVAPI    │
└──────────────────────────────────┘
```

The daemon owns the system-bus name `io.lapsphere.Control` (object path `/io/lapsphere/Control`) and runs three polling jobs: `hardware_monitor`, `fan_control`, and `gpu_overclock` (defaults 1000 / 2000 / 1000 ms, read from `~/.config/lapsphere/settings.json` and updated by the GUI over DBus). The GUI communicates exclusively through DBus — it never writes to hardware directly. The daemon does not wake a suspended dGPU.

The bus policy (`data/io.lapsphere.Control.conf`) lets only root own the name and lets any user call it. The activation file (`data/io.lapsphere.Control.service`) starts `/usr/bin/lapsphere-daemon` as root.

**Single-instance GUI.** The GUI requests the session-bus name `io.lapsphere.Gui`; a second instance prints a message and exits.

**Daemon unit.** `data/lapsphere-daemon.service` is a `Type=dbus` unit (`BusName=io.lapsphere.Control`) that runs `lapsphere-daemon` without flags, with `Restart=on-failure`, `CPUQuota=50%`, `MemoryMax=768M`, and `TasksMax=256`. Packages install it to `/usr/lib/systemd/system/`; it is not enabled by the packaging scripts.

**Flags.**
- `--gui` — the daemon spawns the GUI as the user given by `SUDO_UID` or `PKEXEC_UID` (so start it with `sudo` or `pkexec`), forwarding any other arguments (e.g. `--tray`) to it. When that GUI process exits, the daemon shuts down.
- `--tray` — starts the window hidden in the tray. On the GUI it is a standalone flag; on the daemon it is only forwarded together with `--gui`.
- Without flags the daemon requires root; as a regular user it prints limited statistics and exits.

When the GUI closes it sets all fans to auto and asks the daemon to shut down; the request is ignored if the daemon is managed by systemd.

---

## Requirements

### Runtime
- Linux kernel with tuxedo-drivers loaded (provides `/dev/tuxedo_io` for fan/TDP/webcam control and keyboard LEDs on Clevo/Uniwill hardware); the daemon still starts without `/dev/tuxedo_io`
- `dbus`
- `lscpu` (util-linux, for CPU info)
- `dmidecode` (for memory type/speed detection)
- `iw`, `ethtool` (for WiFi info)
- `pciutils` (`lspci`, for WiFi controller info)
- NVIDIA driver + NVML (`libnvidia-ml.so`) for GPU tuning
- `libnvidia-api.so.1` for hotspot/VRAM temperatures and voltage (optional)
- `optimus-manager` or `prime-select` for PRIME profile switching (optional)

### Build
- Rust toolchain (stable, tested with recent versions)
- C compiler (for the bundled mimalloc allocator)
- `pkg-config` and OpenSSL development headers (the update checker uses `ureq` with `native-tls`)

The Debian package recipe (`debian/control`) and the CI workflow additionally install `libdbus-1-dev`, GTK3/libadwaita, pango/cairo/gdk-pixbuf/atk/graphene, `libwayland-dev`, and `libxkbcommon-dev`. `Cargo.lock` contains no GTK or libdbus crates (DBus goes through `zbus`), and Wayland/X11 support in `eframe` is enabled through Rust crates.

---

## Building

```bash
cargo build --release --all
```

Binaries are placed in `target/release/`:
- `lapsphere` — GUI
- `lapsphere-daemon` — system daemon

### GitHub Actions artifacts

The CI workflow uploads two package artifacts per successful run:

- `lapsphere-ubuntu-24.04-deb` — a Debian package containing both binaries and their service, DBus, desktop, and icon files.
- `lapsphere-arch-pkg` — an Arch package containing the same application files.

`cargo test` runs in CI before the packages are built; the Debian package build itself skips tests (`debian/rules`).

The package artifacts are the supported CI outputs. A workflow run also has a source commit, so an artifact created by an older run is an older snapshot even when its package version remains `0.1.0`. The date shown in `debian/changelog` is package release metadata and does not identify when a CI artifact was built.

---

## Installation (Debian/Ubuntu)

A `debian/` directory is included for building a `.deb` package:

```bash
dpkg-buildpackage -us -uc -b
sudo dpkg -i ../lapsphere_*.deb
```

This installs:
- `/usr/bin/lapsphere` and `/usr/bin/lapsphere-daemon`
- DBus policy `/usr/share/dbus-1/system.d/io.lapsphere.Control.conf`
- DBus activation service `/usr/share/dbus-1/system-services/io.lapsphere.Control.service`
- systemd unit `/usr/lib/systemd/system/lapsphere-daemon.service`
- Desktop entry, autostart entry `/etc/xdg/autostart/io.lapsphere.LapSphere.desktop` (`Exec=lapsphere --tray`), and icon

The package depends on `dbus`, `policykit-1`, `libxkbcommon-x11-0`, `dmidecode`, `pciutils`, `ethtool`, and `iw`, and recommends `tuxedo-drivers`. The root `PKGBUILD` installs the same files for Arch Linux.

---

## Running

### Recommended (via daemon)

```bash
# Start daemon (requires root)
sudo lapsphere-daemon --gui
```

The `--gui` flag causes the daemon to launch the GUI as the original user (read from `SUDO_UID` or `PKEXEC_UID`). The daemon exits when that GUI exits.

### Manual

```bash
# Terminal 1 (root)
sudo lapsphere-daemon

# Terminal 2 (user)
lapsphere
```

### Start minimized to tray

```bash
lapsphere --tray
```

The window starts hidden and the tray icon is used to show it.

---

## Configuration

Settings are split into files under `~/.config/lapsphere/`, one per subsystem:

| File | Contents |
|---|---|
| `settings.json` | Theme, font size, start minimized, tray (minimize on close), autostart, CPU scheduler, statistics sections (polling rates, order, visibility), tuning section order, battery settings, log limit/filter, remembered gamepads |
| `profiles.json` | All tuning profiles and the active profile name |

Crash reports (`crash_<timestamp>.log`) are also written to this directory.

A legacy single-file format (`config.json`) is read on startup and used to fill in whichever of `settings.json` / `profiles.json` is missing; the new files are then written and `config.json` is left in place. If both new files exist, `config.json` is ignored.

The daemon reads its polling rates from `$HOME/.config/lapsphere/settings.json` (under `sudo` this is root's `$HOME`) and receives updates from the GUI over DBus.

### Tray settings

`tray.json` is created on first run. Upgrading from a build that kept the tray
fields in `settings.json` copies those values into `tray.json` once, backs the
original file up to `settings.json.pre-tray-split`, and removes the tray keys
from it. To go back to an older build, restore the backup:

```bash
cp ~/.config/lapsphere/settings.json.pre-tray-split ~/.config/lapsphere/settings.json
```

See [`docs/development/config-files.md`](docs/development/config-files.md) for
the full file layout, the migration order, and the corruption-handling rules.

---

## Project Structure

```
Cargo.toml       Workspace (members: common, daemon, gui)
PKGBUILD         Arch Linux package recipe
common/          Shared types (CpuInfo, GpuInfo, Profile, AppConfig, …)
daemon/          System daemon
  com.tuxedo.Control.conf  Older DBus policy copy (not used by the packages)
  src/
    main.rs              Startup, CLI flags, polling jobs, GPU overclock loop
    dbus_interface.rs    Exposed DBus methods
    daemon_settings.rs   Poll-rate settings read from settings.json
    gpu_activity.rs      Passive inventory of processes holding GPU device nodes
    hardware_detection.rs  Read-only hardware queries
    hardware_control.rs    Write paths (sysfs, NVML, tuxedo_io)
    tuxedo_io.rs         ioctl wrapper for /dev/tuxedo_io
    battery_control.rs   Charge threshold sysfs control
    polling_scheduler.rs Tokio-based job scheduler
gui/             egui GUI
  src/
    main.rs              Entry point, single-instance check, --tray
    app.rs               App state, update loop, config load/migration, hardware channel
    dbus_client.rs       Async DBus command dispatch
    pages/               Statistics, Profiles, Tuning, Settings views
    widgets/             Fan curve editor
    theme.rs             Dark/light theme definitions
    polling_scheduler.rs Client-side refresh coordinator
    system_tray.rs       ksni system tray integration
    gamepad_registry.rs  Remembered gamepads
    keyboard_shortcuts.rs  Ctrl+1–4 / F1 shortcuts
nvidia/          NVIDIA driver/NVAPI sources (not a workspace member and not built; see docs/architecture/modules.md)
data/            DBus policy/activation files, systemd unit, desktop entry, icon
debian/          Debian packaging
docs/            Architecture, subsystem, and development documentation
drivers_src/     Vendored tuxedo-drivers kernel module sources (not built by cargo)
screenshots/     Screenshots
.github/workflows/rust.yml  CI (tests, Debian and Arch packages)
```

---

## Hardware Support

| Feature | Clevo | Uniwill |
|---|---|---|
| Fan control | ✅ | ✅ |
| TDP profiles | ✅ | ✅ |
| Uniwill TDP (PL1/PL2/PL4) | — | ✅ |
| Keyboard RGB | ✅ (3-zone, 1-zone) | ✅ (1-zone) |
| Webcam toggle | ✅ | — |
| Battery thresholds | ✅ (flexicharger) | via sysfs |

NVIDIA GPU features require a supported NVIDIA driver and card. AMD and Intel iGPUs are read-only (stats only).

---

## Documentation

- [`docs/architecture/`](docs/architecture/) — overview, modules, invariants, design decisions
- [`docs/subsystems/`](docs/subsystems/) — daemon lifecycle, GUI state sync, hardware detection, `nvidia/` crate, `tuxedo_io`
- [`docs/development/`](docs/development/) — known problems
- [`docs/VRAM_DETECTION_TROUBLESHOOTING.md`](docs/VRAM_DETECTION_TROUBLESHOOTING.md) and [`docs/gpu-process-memory-statistics.md`](docs/gpu-process-memory-statistics.md)

---

## Caution for Uniwill
Be carefully if your laptop is Uniwill to use it with, where speed limit in RPM instead of percent, that feature is hard to implement correctly, Tuxedo know speed limits on every there laptop, I'm lacking that info.

## License

GPL-2.0 — see `debian/copyright`.
