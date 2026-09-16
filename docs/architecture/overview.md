# Architecture overview

## Scope and evidence

This is a high-level repository map, not a file-scoped call-chain analysis. Each statement is marked `[verified]` when it is directly traced to source or manifests, or `[assumed]` when it is an explicit inference.

## System purpose

- LapSphere is a hardware control and monitoring application for Uniwill/Clevo laptops on Linux. `[verified]` (`README.md`; daemon hardware modules)
- The repository builds a privileged daemon, a per-user GUI, and a shared Rust types crate. `[verified]` (`Cargo.toml`; crate manifests)
- The daemon is intended to run as root and the GUI as a regular user. `[verified]` (`README.md`; `data/io.lapsphere.Control.service`; `daemon/src/main.rs`)

## Major components

### `lapsphere-daemon`

- The daemon is the hardware-facing process and owns hardware polling, cached hardware data, and control operations. `[verified]` (`daemon/src/main.rs`; `daemon/src/hardware_detection.rs`; `daemon/src/hardware_control.rs`)
- Its source-level modules separate DBus exposure, detection, control, battery control, the tuxedo ioctl wrapper, settings, and polling scheduling. `[verified]` (`daemon/src/main.rs`)
- It gathers CPU, memory, GPU, battery, Wi-Fi, storage, mount, fan, and gamepad information into shared data types. `[verified]` (`daemon/src/hardware_detection.rs`; `common/src/types.rs`)
- It exposes the `io.lapsphere.Control` interface at `/io/lapsphere/Control` on the system bus. `[verified]` (`daemon/src/dbus_interface.rs`)

### `lapsphere` GUI

- The GUI is an `eframe`/`egui` desktop application with statistics, profiles, tuning, settings, widgets, keyboard shortcuts, and a system tray. `[verified]` (`gui/Cargo.toml`; `gui/src/main.rs`; `gui/src/app.rs`; `gui/src/pages/mod.rs`; `gui/src/widgets/mod.rs`)
- The GUI keeps application configuration and profile state, renders views, and requests hardware data or mutations through its DBus client. `[verified]` (`gui/src/app.rs`; `gui/src/dbus_client.rs`)
- The GUI does not contain the daemon's hardware access modules. `[verified]` (source module declarations in `gui/src/main.rs` and `daemon/src/main.rs`)

### `lapsphere-common`

- `common` is a workspace crate exporting serializable shared domain types such as hardware snapshots, profiles, settings, and UI configuration. `[verified]` (`common/src/lib.rs`; `common/src/types.rs`)
- Both the daemon and GUI depend on it. `[verified]` (both crate manifests)

### NVIDIA support

- The daemon's active NVIDIA path uses NVML for GPU metrics/control and direct NVIDIA device ioctls plus `libnvidia-api.so.1` access for additional GPU information. `[verified]` (`daemon/Cargo.toml`; `daemon/src/hardware_detection.rs`; `daemon/src/hardware_control.rs`)
- The top-level `nvidia/` directory contains Rust NVIDIA implementation files, but it is not a workspace member and no daemon module declaration references those files. `[verified]` (`Cargo.toml`; source/module search)
- Therefore, the maintained runtime boundary is the daemon's in-tree NVIDIA code; the role of the top-level `nvidia/` directory is not established by current workspace wiring. `[assumed]`

## Runtime boundaries and flow

1. The daemon initializes optional `/dev/tuxedo_io` access, performs an initial hardware poll, starts its polling scheduler, and registers monitor/control jobs. `[verified]` (`daemon/src/main.rs`)
2. The daemon publishes cached read results and control methods through system DBus. `[verified]` (`daemon/src/dbus_interface.rs`; `daemon/src/main.rs`)
3. The GUI creates an asynchronous DBus client, schedules component refreshes, and applies user actions through that client. `[verified]` (`gui/src/dbus_client.rs`; `gui/src/app.rs`)
4. Shared payloads cross the DBus boundary as JSON strings and are converted to `lapsphere-common` types by the client. `[verified]` (`daemon/src/dbus_interface.rs`; `gui/src/dbus_client.rs`)
5. The daemon's hardware boundary includes Linux sysfs/procfs, battery sysfs, NVML/NVAPI, NVIDIA device nodes, and `/dev/tuxedo_io`. `[verified]` (`daemon/src/hardware_detection.rs`; `daemon/src/hardware_control.rs`; `daemon/src/battery_control.rs`; `daemon/src/tuxedo_io.rs`)

## `drivers_src/` external boundary

`drivers_src/` is treated here as an out-of-tree kernel-driver boundary; its implementation is intentionally not described. `[verified]` (`drivers_src` layout; `drivers_src/tuxedo_io/tuxedo_io_ioctl.h`)

- The daemon opens `/dev/tuxedo_io` and sends ioctl requests using magic `0xEC` and Clevo/Uniwill read/write command families. `[verified]` (`daemon/src/tuxedo_io.rs`; `drivers_src/tuxedo_io/tuxedo_io_ioctl.h`)
- The exposed interface covers platform checks, interface/model identification, fan speed/temperature and automatic fan mode, Clevo webcam/flight/touchpad/performance/keyboard controls, Uniwill mode/fan/TDP/profile controls, and module version reporting. `[verified]` (`drivers_src/tuxedo_io/tuxedo_io_ioctl.h`; `daemon/src/tuxedo_io.rs`)
- The daemon treats `/dev/tuxedo_io` as optional and disables dependent features when it is absent or cannot be initialized. `[verified]` (`daemon/src/main.rs`; `daemon/src/tuxedo_io.rs`)

## Deep-dive priorities

1. **Hardware detection and cache freshness** — highest priority because it aggregates nearly all monitoring sources and contains GPU power-state/cache behavior. `[verified]` (`daemon/src/hardware_detection.rs`)
2. **Hardware control and profile application** — high priority because it writes CPU sysfs, battery settings, NVIDIA controls, keyboard state, fans, and profiles. `[verified]` (`daemon/src/hardware_control.rs`; `daemon/src/battery_control.rs`)
3. **DBus contract and client worker** — high priority because it is the sole GUI/daemon boundary and carries both JSON data and mutations. `[verified]` (`daemon/src/dbus_interface.rs`; `gui/src/dbus_client.rs`)
4. **Tuxedo ioctl compatibility** — next because fan, TDP, webcam, and keyboard behavior depends on platform-specific command mappings at the driver boundary. `[verified]` (`daemon/src/tuxedo_io.rs`; `drivers_src/tuxedo_io/tuxedo_io_ioctl.h`)
5. **Polling schedulers** — next because both daemon jobs and GUI refresh cadence are expected to influence responsiveness and hardware/power behavior. `[assumed]` (`daemon/src/polling_scheduler.rs`; `gui/src/polling_scheduler.rs`)
