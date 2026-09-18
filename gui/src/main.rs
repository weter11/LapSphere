mod app;
mod dbus_client;
mod theme;
mod pages;
mod keyboard_shortcuts;
mod widgets;
mod polling_scheduler;
mod system_tray;

use app::LapSphereApp;
use chrono::Local;
use std::fs;
use std::panic;

fn setup_panic_hook() {
    panic::set_hook(Box::new(|panic_info| {
        let timestamp = Local::now().format("%Y%m%d_%H%M%S");
        let crash_dir = app::get_crash_dir();
        let _ = fs::create_dir_all(&crash_dir);

        let file_path = format!("{}/crash_{}.log", crash_dir, timestamp);

        let mut message = String::new();
        if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            message = s.to_string();
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            message = s.clone();
        }

        let location = panic_info.location()
            .map(|l| format!(" at {}:{}", l.file(), l.line()))
            .unwrap_or_default();

        let backtrace = format!("{:?}", std::backtrace::Backtrace::capture());

        let report = format!(
            "LapSphere GUI Crash Report\n\
             ==========================\n\
             Time: {}\n\
             Panic: {}{}\n\n\
             Backtrace:\n\
             {}",
            Local::now().format("%Y-%m-%d %H:%M:%S"),
            message,
            location,
            backtrace
        );

        let _ = fs::write(file_path, report);

        #[cfg(target_os = "linux")]
        eprintln!("Application panicked! Crash report saved to ~/.config/lapsphere");
        #[cfg(target_os = "windows")]
        eprintln!("Application panicked! Crash report saved to %APPDATA%\\lapsphere");
    }));
}

#[cfg(target_os = "linux")]
fn check_single_instance_linux(rt: &tokio::runtime::Runtime) -> Option<zbus::Connection> {
    rt.block_on(async {
        match zbus::Connection::session().await {
            Ok(conn) => {
                // Use the explicit DBus proxy to request name and check the reply
                let dbus = match zbus::fdo::DBusProxy::new(&conn).await {
                    Ok(proxy) => proxy,
                    Err(e) => {
                        log::error!("Failed to create DBus proxy: {}", e);
                        return Some(conn);
                    }
                };

                let reply = dbus.request_name(
                    "io.lapsphere.Gui".try_into().unwrap(),
                    zbus::fdo::RequestNameFlags::DoNotQueue.into()
                ).await;

                match reply {
                    Ok(zbus::fdo::RequestNameReply::PrimaryOwner) => Some(conn),
                    Ok(_) => {
                        eprintln!("Another instance of LapSphere GUI is already running.");
                        None
                    }
                    Err(e) => {
                        log::error!("DBus error requesting name: {}", e);
                        Some(conn)
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to connect to session bus for single instance check: {}", e);
                None
            }
        }
    })
}

#[cfg(target_os = "windows")]
fn check_single_instance_windows() -> Option<isize> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexA;
    use std::ptr::null;

    let name = b"Global\\io.lapsphere.Gui\0";
    unsafe {
        let handle: HANDLE = CreateMutexA(null(), 1, name.as_ptr());
        if handle == 0 {
            return Some(0);
        }
        let err = windows_sys::Win32::Foundation::GetLastError();
        if err == ERROR_ALREADY_EXISTS {
            eprintln!("Another instance of LapSphere GUI is already running.");
            return None;
        }
        Some(handle)
    }
}

// Replace glibc malloc with mimalloc: glibc's per-thread arena pool retains
// freed-but-unused memory (up to 64 MB per arena, one per malloc-thread) and
// never returns it to the OS. Under the GUI's ~8k alloc/s D-Bus polling load
// that arena ratchet accumulated ~8 GB of resident-but-free heap over hours
// (see heaptrack probe: real Rust heap stayed at 27 MB peak / 22 MB leaked
// while RSS grew to 3.6 GB). mimalloc bounds arenas and reuses freed blocks.
//
// NOTE: mimalloc is NOT retention-free out of the box. It reserves 1 GiB
// arenas, commits them eagerly on Linux (`arena_eager_commit = 2`) and lets
// transparent huge pages back them, so the reserved region stays resident in
// `[anon:mimalloc]` even with a ~27 MB live heap (measured: 57.6 MB of mimalloc
// RSS of which 51.2 MB was AnonHugePages). `apply_mimalloc_tuning()` sets the
// three knobs that bound it — in-process, at startup, so every launch path
// (menu, autostart, terminal, tray) is covered and nothing depends on a
// `.desktop` Exec line, an env wrapper or a packaging sed. Measured with the
// installed v3.3.2 build, 5 min per arm, `--tray`, same D-Bus workload:
// mimalloc-map RSS 57.6 MB -> 12.8 MB and VmRSS 160.8 MB -> 115.9 MB.
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// Bound mimalloc's arena footprint from inside the process.
///
/// The mimalloc Rust crate (0.1.52) binds no option API — its `extended`
/// feature only adds `version()`, `usable_size()` and `stats_json()` — but
/// `mi_option_set`/`mi_option_get` are exported from the static mimalloc
/// library that `libmimalloc-sys` links, so a plain `extern "C"` reaches them
/// without enabling anything or adding a dependency.
mod mimalloc_tuning {
    use std::os::raw::{c_int, c_long};

    // Values of `mi_option_t` for mimalloc v3 (libmimalloc-sys 0.1.49 vendors
    // v3; `MI_MALLOC_VERSION 30302` == 3.3.2). The C ABI takes the enum's
    // numeric value, so they are pinned here next to the version they were
    // read from — re-check them when bumping mimalloc.
    const OPTION_ARENA_EAGER_COMMIT: c_int = 4;
    const OPTION_PURGE_DELAY: c_int = 15;
    const OPTION_ALLOW_THP: c_int = 43;

    extern "C" {
        fn mi_option_set(option: c_int, value: c_long);
        fn mi_option_get(option: c_int) -> c_long;
    }

    /// Apply the arena-bounding options. Safe to call at any point in `main`:
    /// options are read live through `_mi_option_get_fast`, and `mi_option_set`
    /// marks them initialized, so these values win over the environment.
    pub fn apply() {
        unsafe {
            // 0 = don't commit reserved arena space up front (default 2).
            mi_option_set(OPTION_ARENA_EAGER_COMMIT, 0);
            // 0 = don't let THP back the arenas (stops ~2 MB-granular RSS).
            mi_option_set(OPTION_ALLOW_THP, 0);
            // 0 = decommit freed pages immediately; the default is 1000 ms and
            // -1 would disable purging altogether.
            mi_option_set(OPTION_PURGE_DELAY, 0);
        }
    }

    /// Read the effective values back (verifiable at runtime with
    /// `RUST_LOG=info lapsphere`).
    pub fn log_effective() {
        unsafe {
            log::info!(
                "mimalloc arena tuning: arena_eager_commit={} allow_thp={} purge_delay_ms={}",
                mi_option_get(OPTION_ARENA_EAGER_COMMIT),
                mi_option_get(OPTION_ALLOW_THP),
                mi_option_get(OPTION_PURGE_DELAY),
            );
        }
    }
}

/// Run the tuning before mimalloc's own process initialization.
///
/// Options are read live, so setting them in `main` already applies — except for
/// the two things mimalloc does exactly once at process init: it reads
/// `allow_thp` to decide whether THP may back its regions, and issues
/// `prctl(PR_SET_THP_DISABLE)` when THP is off. Those already-committed THP pages
/// are what a main()-time call cannot undo (measured: ~10 MB of AnonHugePages left
/// behind versus 0 with the same options supplied through `MIMALLOC_*`). An ELF
/// constructor runs before the first Rust allocation, so it reproduces the
/// environment-variable behaviour without depending on the environment.
#[cfg(target_os = "linux")]
mod early_tuning {
    use super::mimalloc_tuning;

    extern "C" fn init() {
        mimalloc_tuning::apply();
    }

    #[used]
    #[link_section = ".init_array"]
    static EARLY_INIT: extern "C" fn() = init;
}

fn main() -> Result<(), eframe::Error> {
    // Bound the allocator before anything heavy runs. On Linux this has already
    // happened in the ELF constructor below; the call is kept for other targets.
    mimalloc_tuning::apply();
    env_logger::init();
    mimalloc_tuning::log_effective();
    setup_panic_hook();

    let args: Vec<String> = std::env::args().collect();
    let start_in_tray_arg = args.contains(&"--tray".to_string());

    let config = app::load_config_from_disk().unwrap_or_default();
    let start_minimized = start_in_tray_arg || config.start_minimized;

    // Create and enter a Tokio runtime context.
    // This is required for `tokio::spawn` to work in the `DbusClient`.
    let rt = tokio::runtime::Runtime::new().expect("Unable to create a Tokio runtime");
    let _enter = rt.enter();

    #[cfg(target_os = "linux")]
    let _instance_guard = match check_single_instance_linux(&rt) {
        Some(conn) => conn,
        None => return Ok(()),
    };

    #[cfg(target_os = "windows")]
    let _instance_guard = match check_single_instance_windows() {
        Some(mutex) => mutex,
        None => return Ok(()),
    };
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([570.0, 620.0])
            .with_min_inner_size([440.0, 470.0])
            .with_icon(load_icon())
            .with_visible(!start_minimized),
        ..Default::default()
    };
    
    eframe::run_native(
        "LapSphere",
        options,
        Box::new(move |cc| Ok(Box::new(LapSphereApp::new(cc)))),
    )
}

fn load_icon() -> egui::IconData {
    let width = 32;
    let height = 32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    for y in 0..height {
        for x in 0..width {
            let idx = ((y * width + x) * 4) as usize;

            // Gaming-themed "L" icon: neon green on dark background
            let is_l_vertical = x >= 10 && x <= 14 && y >= 6 && y <= 26;
            let is_l_horizontal = x >= 10 && x <= 22 && y >= 22 && y <= 26;

            if is_l_vertical || is_l_horizontal {
                rgba[idx] = 0;     // R
                rgba[idx + 1] = 255; // G
                rgba[idx + 2] = 0;   // B
                rgba[idx + 3] = 255; // A
            } else {
                rgba[idx] = 26;
                rgba[idx + 1] = 26;
                rgba[idx + 2] = 26;
                rgba[idx + 3] = 255;
            }
        }
    }

    egui::IconData {
        rgba,
        width,
        height,
    }
}
