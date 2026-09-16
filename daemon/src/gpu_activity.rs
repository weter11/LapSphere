//! Passive GPU open-device holder inventory. Never opens device files.
use lapsphere_common::types::*;
use std::path::Path;

fn scan(proc_root: &Path, nodes: &[String]) -> GpuProcessSnapshot {
    use std::{collections::BTreeSet, fs, io::ErrorKind};
    let mut result = GpuProcessSnapshot {
        sampled_at_unix_secs: now(),
        ..Default::default()
    };
    let Ok(procs) = fs::read_dir(proc_root) else {
        return result;
    };
    result.complete = true;
    for entry in procs {
        let Ok(entry) = entry else {
            result.complete = false;
            continue;
        };
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let fds = match fs::read_dir(entry.path().join("fd")) {
            Ok(fds) => fds,
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    result.complete = false;
                }
                continue;
            }
        };
        let mut matched = BTreeSet::new();
        for fd in fds {
            let Ok(fd) = fd else {
                result.complete = false;
                continue;
            };
            match fs::read_link(fd.path()) {
                Ok(target) => {
                    let target = target.to_string_lossy().into_owned();
                    if nodes.contains(&target) {
                        matched.insert(target);
                    }
                }
                Err(e) if e.kind() == ErrorKind::NotFound => {} // process/FD exit race
                Err(_) => result.complete = false,
            }
        }
        if !matched.is_empty() {
            let name = fs::read_to_string(entry.path().join("comm"))
                .unwrap_or_else(|_| "<exited or inaccessible>".into())
                .trim()
                .to_owned();
            result.processes.push(GpuProcess {
                pid,
                name,
                device_nodes: matched.into_iter().collect(),
            });
        }
    }
    result.processes.sort_by_key(|p| p.pid);
    result
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// Map PCI identity to card/render nodes and the NVIDIA per-device minor.
// Reading links and the driver information file does not open a GPU device.
fn device_nodes(pci: &str) -> Vec<String> {
    use std::fs;
    let mut nodes = Vec::new();
    if let Ok(entries) = fs::read_dir(format!("/sys/bus/pci/devices/{pci}/drm")) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if (name.starts_with("card") || name.starts_with("renderD")) && !name.contains('-') {
                nodes.push(format!("/dev/dri/{name}"));
            }
        }
    }
    if let Ok(info) = fs::read_to_string(format!("/proc/driver/nvidia/gpus/{pci}/information")) {
        for line in info.lines() {
            if let Some(minor) = line
                .strip_prefix("Device Minor:")
                .and_then(|s| s.trim().parse::<u32>().ok())
            {
                nodes.push(format!("/dev/nvidia{minor}"));
            }
        }
    }
    nodes
}

static PROCESSES: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, GpuProcessSnapshot)>>,
> = once_cell::sync::Lazy::new(Default::default);
static MEMORY: once_cell::sync::Lazy<
    std::sync::Mutex<std::collections::HashMap<String, GpuMemorySnapshot>>,
> = once_cell::sync::Lazy::new(Default::default);

pub fn record_memory(pci: &str, free: u64, used: u64, total: u64) {
    let mut cache = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(
        pci.into(),
        GpuMemorySnapshot {
            free_mib: free / 1048576,
            used_mib: used / 1048576,
            total_mib: total / 1048576,
            sampled_at_unix_secs: now(),
        },
    );
}

pub fn attach(gpus: &mut [GpuInfo]) {
    for gpu in gpus {
        let Some(pci) = gpu.pci_bus_id.as_deref() else {
            continue;
        };
        let mut cache = PROCESSES.lock().unwrap_or_else(|e| e.into_inner());
        let stale = cache
            .get(pci)
            .map_or(true, |(t, _)| t.elapsed().as_secs() >= 5);
        if stale {
            let nodes = device_nodes(pci);
            let snapshot = if nodes.is_empty() {
                GpuProcessSnapshot::default()
            } else {
                scan(Path::new("/proc"), &nodes)
            };
            cache.insert(pci.into(), (std::time::Instant::now(), snapshot));
        }
        gpu.process_snapshot = cache[pci].1.clone();
        gpu.vram_memory = MEMORY
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(pci)
            .cloned();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};
    #[test]
    fn maps_device_holders_deduplicates_and_ignores_global_control_node() {
        let root = std::env::temp_dir().join(format!("lp-gpu-fixture-{}", std::process::id()));
        let fd = root.join("123/fd");
        fs::create_dir_all(&fd).unwrap();
        fs::write(root.join("123/comm"), "browser\n").unwrap();
        symlink("/dev/nvidia0", fd.join("1")).unwrap();
        symlink("/dev/nvidia0", fd.join("2")).unwrap();
        symlink("/dev/nvidiactl", fd.join("3")).unwrap();
        let other = root.join("124/fd");
        fs::create_dir_all(&other).unwrap();
        fs::write(root.join("124/comm"), "other\n").unwrap();
        symlink("/dev/nvidia1", other.join("1")).unwrap();
        let got = scan(&root, &["/dev/nvidia0".into()]);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(got.processes.len(), 1);
        assert_eq!(got.processes[0].pid, 123);
        assert_eq!(got.processes[0].name, "browser");
        assert_eq!(got.processes[0].device_nodes, vec!["/dev/nvidia0"]);
        assert!(got.complete);
    }
    #[test]
    fn memory_snapshot_preserves_free_separately_from_used_and_age() {
        let pci = "0000:aa:00.0";
        record_memory(pci, 700 * 1048576, 200 * 1048576, 1024 * 1048576);
        let cache = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
        let memory = &cache[pci];
        assert_eq!(memory.free_mib, 700); // reserved VRAM is NOT free
        assert_eq!(memory.used_mib, 200);
        assert_eq!(memory.total_mib, 1024);
        assert!(memory.sampled_at_unix_secs <= now());
    }

    #[test]
    #[ignore = "read-only live /proc scan; run explicitly with strace"]
    fn live_passive_scan() {
        let nodes = device_nodes("0000:01:00.0");
        assert!(!nodes.is_empty());
        for _ in 0..3 {
            println!(
                "{}",
                serde_json::to_string(&scan(Path::new("/proc"), &nodes)).unwrap()
            );
        }
    }

    #[test]
    fn missing_proc_is_unknown_not_an_empty_success() {
        assert!(!scan(Path::new("/nonexistent-lp-proc"), &[]).complete);
    }
}
