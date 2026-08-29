# Module and crate map

Every statement is marked `[verified]` when traced in source/manifests or `[assumed]` when inferred. “Dependencies in” means repository components that call or consume the module; “dependencies out” means direct imports or declared runtime boundaries.

## `daemon/` — `lapsphere-daemon`

- **Responsibility:** Run the privileged service, collect hardware state, maintain the shared cache, schedule polling, and implement hardware mutations. `[verified]` (`daemon/src/main.rs`; module declarations)
- **Owns:** DBus service implementation, detection/control orchestration, daemon settings, battery control, tuxedo ioctl wrapper, logging, and scheduler jobs. `[verified]` (`daemon/src/main.rs`; daemon source files)
- **Does not own:** GUI rendering or shared type definitions. `[verified]` (GUI module declarations; `common/src/lib.rs`)
- **Dependencies in:** GUI DBus requests; systemd/DBus service activation. `[verified]` (`gui/src/dbus_client.rs`; `data/io.lapsphere.Control.service`)
- **Dependencies out:** `lapsphere-common`, zbus, Tokio, NVML, libloading/NVIDIA API, Linux sysfs/procfs, `/dev/tuxedo_io`, and battery sysfs. `[verified]` (`daemon/Cargo.toml`; daemon source imports and paths)

### Daemon submodules

- **`dbus_interface.rs`** — publishes read and write operations as `io.lapsphere.Control`; it owns transport/error adaptation, not hardware implementation. `[verified]` (`daemon/src/dbus_interface.rs`)
- **`hardware_detection.rs`** — owns read-side hardware discovery and metric construction/caching; it does not define the GUI or DBus transport. `[verified]` (imports, functions, and module boundaries)
- **`hardware_control.rs`** — owns write-side CPU, GPU, fan, keyboard, webcam, profile, and PRIME operations; it does not own DBus method declarations. `[verified]` (`daemon/src/hardware_control.rs`; `daemon/src/dbus_interface.rs`)
- **`battery_control.rs`** — owns battery charge-control availability, threshold reads, and threshold writes; it does not own general battery metric collection. `[verified]` (`daemon/src/battery_control.rs`; `daemon/src/hardware_detection.rs`)
- **`tuxedo_io.rs`** — owns the daemon-side `/dev/tuxedo_io` ioctl client and Clevo/Uniwill interface selection; it does not own the kernel-driver implementation. `[verified]` (`daemon/src/tuxedo_io.rs`)
- **`polling_scheduler.rs`** — owns timed job registration, interval updates, execution, rescheduling, and shutdown; it does not own the jobs' hardware actions. `[verified]` (`daemon/src/polling_scheduler.rs`)
- **`daemon_settings.rs`** — owns daemon-side polling/settings synchronization structures and scheduler updates; it does not own GUI rendering. `[verified]` (`daemon/src/daemon_settings.rs`)

## `gui/` — `lapsphere`

- **Responsibility:** Provide the desktop UI, local configuration/profile experience, refresh coordination, and system tray. `[verified]` (`gui/src/main.rs`; `gui/src/app.rs`; GUI module declarations)
- **Owns:** `egui` views, UI state, theme, keyboard shortcuts, fan-curve widget, async DBus command dispatch, and client refresh scheduling. `[verified]` (`gui/src/main.rs`; `gui/src/app.rs`; `gui/src/dbus_client.rs`)
- **Does not own:** privileged hardware access, sysfs writes, ioctl calls, or daemon polling jobs. `[verified]` (GUI imports/module declarations; daemon source)
- **Dependencies in:** user interaction and desktop session. `[assumed]`
- **Dependencies out:** `lapsphere-common`, zbus, Tokio, eframe/egui, system statistics support, and optional platform/tray libraries. `[verified]` (`gui/Cargo.toml`; GUI source)

## `common/` — `lapsphere-common`

- **Responsibility:** Define serializable data contracts shared by the daemon and GUI. `[verified]` (`common/src/lib.rs`; `common/src/types.rs`)
- **Owns:** Hardware snapshot structs, profile/settings structs, enums, and serialization derives. `[verified]` (`common/src/types.rs`)
- **Does not own:** Runtime I/O, DBus connections, scheduling, hardware detection, or rendering. `[verified]` (`common/Cargo.toml`; `common/src/lib.rs`)
- **Dependencies in:** Daemon and GUI imports. `[verified]` (both crate manifests; source imports)
- **Dependencies out:** Serde and Serde JSON only. `[verified]` (`common/Cargo.toml`)

## `nvidia/`

- **Responsibility:** Provide GPU VRAM usage and hotspot temperature through an undocumented NVIDIA interface, a community-discovered method that is not part of NVIDIA's public API, plus additional NVIDIA driver features not currently used elsewhere. `[maintainer-confirmed]`
- **Status:** This is not dead or unwanted code. It is currently disconnected from the build: it is not a workspace member and has no `mod` references from `daemon/` or `gui/`. `[maintainer-confirmed]` (wiring facts remain `[verified]`)
- **Open question:** Whether and how to wire this into `daemon/`'s NVIDIA path remains undecided. `[maintainer-confirmed]`
- **Dependencies in:** No repository dependency was found. `[verified]` (workspace/module search)
- **Dependencies out:** External crates and a Linux NVIDIA userspace/device interface are referenced by the files. `[verified]` (`nvidia/*.rs`)

## `drivers_src/` — external driver boundary

- **Responsibility:** Provide the kernel-side `/dev/tuxedo_io` interface consumed by the daemon. `[verified]` (`drivers_src/tuxedo_io/tuxedo_io_ioctl.h`; `daemon/src/tuxedo_io.rs`)
- **Owns:** The external ioctl contract for platform checks, Clevo/Uniwill reads, and Clevo/Uniwill writes. `[verified]` (`drivers_src/tuxedo_io/tuxedo_io_ioctl.h`)
- **Does not own:** Daemon scheduling, DBus, GUI state, shared Rust types, or userspace policy. `[verified]` (repository module boundaries)
- **Dependencies in:** Daemon `TuxedoIo` calls through `/dev/tuxedo_io`. `[verified]` (`daemon/src/tuxedo_io.rs`)
- **Dependencies out:** Kernel/device support and the external tuxedo-drivers build environment. `[verified]` (`drivers_src/Kbuild`; driver interface header)
- **Boundary note:** This entry documents only the exposed device path and ioctl families; driver internals are intentionally out of scope. `[verified]` (task scope; header and daemon boundary)
