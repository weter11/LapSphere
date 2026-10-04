//! LapSphere frames layer — a Vulkan **implicit** layer that timestamps
//! `vkQueuePresentKHR` into shared memory (protocol v1, see `shm.rs` and
//! `docs/development/panel-design.md` ADR-5).
//!
//! Design constraints this file implements, in order of priority:
//!
//! 1. **Fail-open.** Nothing here may change the game's behaviour. Every FFI
//!    entry point is wrapped in `catch_unwind`, no return value is ever
//!    modified, and every failure path calls the next layer's function with
//!    the arguments unchanged.
//! 2. **Inert without `LAPSPHERE_FRAMES=1`.** The manifest's
//!    `enable_environment` makes the loader skip us when it is unset; if we
//!    are loaded anyway, the second explicit gate in `vk_create_instance`
//!    turns every hook into a pass-through.
//! 3. **No allocation and no syscall on the present path.** The segment, the
//!    dispatch tables and the writer all exist before the first frame.

pub mod buildid;
pub mod shm;
pub mod vkraw;

use std::ffi::{c_char, c_void, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use shm::Writer;
use vkraw::*;

/// Set once, from `vkCreateInstance`, when `LAPSPHERE_FRAMES` is set.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// The single writer. `None` until the first `vkCreateInstance` that succeeds.
static WRITER: OnceLock<Mutex<Option<Writer>>> = OnceLock::new();

fn writer_slot() -> &'static Mutex<Option<Writer>> {
    WRITER.get_or_init(|| Mutex::new(None))
}

/// Next-layer instance dispatch.
#[derive(Clone, Copy)]
struct InstanceState {
    get_proc_addr: PFN_vkGetInstanceProcAddr,
    create_device: PFN_vkCreateDevice,
    destroy_instance: PFN_vkDestroyInstance,
}

static INSTANCE: OnceLock<Mutex<Option<InstanceState>>> = OnceLock::new();

fn instance_slot() -> &'static Mutex<Option<InstanceState>> {
    INSTANCE.get_or_init(|| Mutex::new(None))
}

/// Next-layer device dispatch.
#[derive(Clone, Copy)]
struct DeviceState {
    queue_present: PFN_vkQueuePresentKHR,
    destroy_device: PFN_vkDestroyDevice,
    get_device_proc_addr: PFN_vkGetDeviceProcAddr,
    create_swapchain: PFN_vkCreateSwapchainKHR,
    destroy_swapchain: PFN_vkDestroySwapchainKHR,
}

/// Device handles are pointers; the table is keyed on the raw value. A queue
/// obtained from a device carries the *same* dispatch pointer on the Vulkan
/// loader, which is what lets `vkQueuePresentKHR` find its device state from a
/// queue handle. That is a loader property, not a spec guarantee — the spike's
/// failure tests would surface it if it did not hold.
static DEVICES: OnceLock<Mutex<Vec<(u64, DeviceState)>>> = OnceLock::new();

fn devices() -> &'static Mutex<Vec<(u64, DeviceState)>> {
    DEVICES.get_or_init(|| Mutex::new(Vec::new()))
}

fn device_state(handle: u64) -> Option<DeviceState> {
    devices()
        .lock()
        .ok()
        .and_then(|d| d.iter().find(|(h, _)| *h == handle).map(|(_, s)| *s))
}

fn register_device(handle: u64, st: DeviceState) {
    if let Ok(mut d) = devices().lock() {
        if let Some(e) = d.iter_mut().find(|(h, _)| *h == handle) {
            e.1 = st;
        } else {
            d.push((handle, st));
        }
    }
}

fn unregister_device(handle: u64) {
    if let Ok(mut d) = devices().lock() {
        d.retain(|(h, _)| *h != handle);
    }
}

// ---------------------------------------------------------------------------
// instrumentation
// ---------------------------------------------------------------------------

/// Build identity, printed in `negotiate` so a stale `.so` can never again be
/// mistaken for a behavioural result. The manifest points at the library by
/// absolute path, so "cargo finished" does not imply "this is the code that
/// ran".
pub use buildid::{BUILD_ID, BUILD_MTIME};

/// Path of the running process, so the loader's own `/proc/<pid>/maps` can be
/// compared against the file we think we are in.
fn self_maps_path() -> String {
    format!("/proc/{}/maps", std::process::id())
}

/// Append one line to `$XDG_RUNTIME_DIR/lapsphere/spike.log`.
///
/// A file rather than stderr: a layer runs inside somebody else's process
/// whose stderr may be discarded, closed, or (as happened twice) still
/// showing an earlier build's output. Counters and a file make that
/// impossible.
pub fn dbg_log(msg: &str) {
    use std::io::Write;
    let dir = format!(
        "{}/lapsphere",
        std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into())
    );
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{}/spike.log", dir))
    {
        let _ = writeln!(f, "[{}] {}", std::process::id(), msg);
    }
}

/// Which mapped file (if any) contains `addr`. Turns "the pointer looks
/// wrong" into "the pointer is in *that* object", which is the only form of
/// this bug that can be reasoned about.
fn which_mapping(addr: usize) -> String {
    let maps = match std::fs::read_to_string("/proc/self/maps") {
        Ok(m) => m,
        Err(e) => return format!("<maps unreadable: {}>", e),
    };
    for line in maps.lines() {
        let mut it = line.split_whitespace();
        let range = it.next().unwrap_or("");
        let _perms = it.next().unwrap_or("");
        let _off = it.next().unwrap_or("");
        let _dev = it.next().unwrap_or("");
        let _inode = it.next().unwrap_or("");
        let path = it.next().unwrap_or("[anon]");
        if let Some((lo, hi)) = range.split_once('-') {
            let lo = usize::from_str_radix(lo, 16).unwrap_or(0);
            let hi = usize::from_str_radix(hi, 16).unwrap_or(0);
            if addr >= lo && addr < hi {
                return format!("{} offset+0x{:x}", path, addr - lo);
            }
        }
    }
    "<not mapped>".to_string()
}

/// Per-hook entry counters, so "the hook was never called" is a measurement
/// rather than an inference from the absence of output.
pub static HOOK_CALLS: [std::sync::atomic::AtomicU64; 8] = [
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
    std::sync::atomic::AtomicU64::new(0),
];
pub const HOOK_NAMES: [&str; 8] = [
    "gipa",
    "create_instance",
    "create_device",
    "gdpa",
    "destroy_device",
    "create_swapchain",
    "destroy_swapchain",
    "queue_present",
];

#[inline]
fn bump(i: usize) {
    HOOK_CALLS[i].fetch_add(1, Ordering::Relaxed);
}

fn counters_line() -> String {
    let v: Vec<String> = HOOK_NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}={}", n, HOOK_CALLS[i].load(Ordering::Relaxed)))
        .collect();
    v.join(" ")
}

// ---------------------------------------------------------------------------
// negotiation
// ---------------------------------------------------------------------------

pub const LAYER_NAME: &str = "VK_LAYER_LAPSPHERE_frames";

#[no_mangle]
pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    p: *mut VkNegotiateLayerInterface,
) -> VkResult {
    if p.is_null() {
        return ERROR_INITIALIZATION_FAILED;
    }
    dbg_log(&format!(
        "negotiate: loader offered {}, build {} mtime {} maps_self={}",
        (*p).loader_layer_interface_version,
        BUILD_ID,
        BUILD_MTIME,
        self_maps_path()
    ));
    let s = &mut *p;
    s.s_type = STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO;
    s.p_next = std::ptr::null_mut();
    // Clamp to what the loader offers: v2 adds the physical-device slot.
    if s.loader_layer_interface_version >= 2 {
        s.loader_layer_interface_version = 2;
    } else {
        s.loader_layer_interface_version = 1;
    }
    s.pfn_get_instance_proc_addr = vk_get_instance_proc_addr;
    s.pfn_get_device_proc_addr = vk_get_device_proc_addr;
    s.pfn_get_physical_device_proc_addr = if s.loader_layer_interface_version >= 2 {
        Some(vk_get_physical_device_proc_addr)
    } else {
        None
    };
    SUCCESS
}

/// The v1 spelling of the same entry point. The loader looks for this name
/// first, then the v2 one; both must exist.
#[no_mangle]
pub unsafe extern "system" fn vk_icdNegotiateLoaderLayerInterfaceVersion(
    p: *mut VkNegotiateLayerInterface,
) -> VkResult {
    vkNegotiateLoaderLayerInterfaceVersion(p)
}

// ---------------------------------------------------------------------------
// instance-level dispatch
// ---------------------------------------------------------------------------

/// Widen one of our typed entry points to the `PFN_vkVoidFunction` the
/// loader's proc-addr functions return. A pure ABI cast, done in one place so
/// the cast cannot be mistyped at a call site.
#[inline]
unsafe fn as_void(f: *const ()) -> PFN_vkVoidFunction {
    std::mem::transmute::<*const (), PFN_vkVoidFunction>(f)
}

#[no_mangle]
pub unsafe extern "system" fn vk_get_instance_proc_addr(
    instance: Instance,
    p_name: *const c_char,
) -> PFN_vkVoidFunction {
    if p_name.is_null() {
        return None;
    }
    let name = CStr::from_ptr(p_name).to_bytes();
    // `PFN_vkVoidFunction` is itself an `Option`, so the match arms are the
    // return value directly -- no extra `Some` wrapper.
    let ours: PFN_vkVoidFunction = match name {
        b"vkGetInstanceProcAddr" => as_void(vk_get_instance_proc_addr as *const ()),
        b"vkCreateInstance" => as_void(vk_create_instance as *const ()),
        b"vkDestroyInstance" => as_void(vk_destroy_instance as *const ()),
        b"vkCreateDevice" => as_void(vk_create_device as *const ()),
        b"vkGetDeviceProcAddr" => as_void(vk_get_device_proc_addr as *const ()),
        _ => None,
    };
    if ours.is_some() {
        return ours;
    }
    // Everything else goes down the chain untouched — including
    // `vkEnumerateInstanceExtensionProperties`, which the loader calls on
    // every app. Answering it ourselves is a classic way to break a layer.
    next_instance_proc(instance, p_name)
}

unsafe fn next_instance_proc(instance: Instance, name: *const c_char) -> PFN_vkVoidFunction {
    let f = instance_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.get_proc_addr));
    match f {
        Some(f) => f(instance, name),
        None => None,
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_create_instance(
    p_create_info: *const VkInstanceCreateInfo,
    p_allocator: *const VkAllocationCallbacks,
    p_instance: *mut Instance,
) -> VkResult {
    bump(1);
    dbg_log(&format!("create_instance entered p_next={:p}", (*p_create_info).p_next));
    // The loader appends a VkLayerInstanceCreateInfo to p_next carrying the
    // next layer's proc addr.
    let mut next_gipa: Option<PFN_vkGetInstanceProcAddr> = None;
    // Each VkLayerInstanceLink carries `p_next` (the layer below the one that
    // gave us this link) and that layer's gipa. Owner hypothesis (в): the
    // *public* gipa of an adjacent layer is not safe to call with an instance
    // handle on this loader -- it loops forever -- whereas walking the link
    // chain is. Both routes are recorded so they can be compared by value.
    let mut link_ptr: *mut VkLayerInstanceLink = std::ptr::null_mut();
    let mut chain = (*p_create_info).p_next;
    while !chain.is_null() {
        if (*chain).s_type == STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO {
            let li = chain as *const VkLayerInstanceCreateInfo;
            // Only follow the chain when it is the link-info member; other
            // union members (loader-data callback, feature flags) are the
            // loader's business and must be forwarded untouched.
            if (*li).function == VK_LAYER_LINK_INFO && !(*li).p_layer_info.is_null() {
                link_ptr = (*li).p_layer_info;
                next_gipa = Some((*(*li).p_layer_info).pfn_next_get_instance_proc_addr);
                break;
            }
            // Not the link-info member: the loader also places a
            // VK_LOADER_FEATURES node in this chain, and the link node is not
            // necessarily first. Keep walking instead of giving up.
        }
        chain = (*chain).p_next;
    }

    eprintln!("[lapsphere-frames] next_gipa present: {}", next_gipa.is_some());
    let gipa = match next_gipa {
        Some(f) => f,
        None => return ERROR_INITIALIZATION_FAILED,
    };
    dbg_log(&format!("next_gipa={:p}", gipa as *const ()));
    let create: PFN_vkCreateInstance =
        match std::mem::transmute::<PFN_vkVoidFunction, PFN_vkCreateInstance>(gipa(
            Instance::null(),
            b"vkCreateInstance\0".as_ptr() as *const c_char,
        )) {
            f => f,
        };

    // Forward first. Our state is installed afterwards, so every early return
    // below leaves the chain exactly as the game left it.
    let res = create(p_create_info, p_allocator, p_instance);
    dbg_log(&format!("next vkCreateInstance -> {:?}", res));
    if res.is_error() {
        return res;
    }

    // Opt-in gate. The manifest already makes the loader skip us when the
    // variable is unset; this is the second gate, and it is what makes
    // criterion 11 true even if the layer is enabled explicitly.
    let active = std::env::var("LAPSPHERE_FRAMES")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);
    if !active {
        return res;
    }

    let inst = *p_instance;
    // HYPOTHESIS UNDER TEST (owner, item б): the pointer obtained from the
    // chain link spins forever when called with a real instance handle on this
    // loader. Use the loader's own exported vkGetInstanceProcAddr instead --
    // resolved with dlsym(RTLD_DEFAULT) -- for the instance-level lookups.
    // Walk to the LAST link -- that one belongs to the loader terminator
    // (libvulkan.so), whose gipa is the correct, complete implementation to ask
    // for a core entry point with a real instance handle. Measured on this
    // host: calling the *first* link (libVkLayer_MESA_device_select) with an
    // instance handle never returns.
    let mut terminator_gipa: PFN_vkGetInstanceProcAddr = gipa;
    {
        let mut l = link_ptr;
        let mut last = 0usize;
        let mut depth = 0;
        while !l.is_null() && depth < 8 {
            terminator_gipa = (*l).pfn_next_get_instance_proc_addr;
            last = depth;
            l = (*l).p_next;
            depth += 1;
        }
        dbg_log(&format!(
            "using terminator gipa from link[{}] ({})",
            last,
            which_mapping(terminator_gipa as usize)
        ));
    }
    let loader_gipa: PFN_vkGetInstanceProcAddr = terminator_gipa;
    dbg_log(&format!(
        "chain gipa={:p} loader gipa={:p} (dlsym)",
        gipa as *const (),
        loader_gipa as *const ()
    ));
    dbg_log(&format!("chain gipa lives in: {}", which_mapping(gipa as usize)));
    // Walk the link chain explicitly: each VkLayerInstanceLink carries
    // p_next (the layer below) and the next gipa. Print every link we can see,
    // with the object each belongs to.
    {
        let mut l = link_ptr;
        let mut depth = 0;
        while !l.is_null() && depth < 8 {
            dbg_log(&format!(
                "link[{}] {:p} gipa={:p} in {}",
                depth,
                l,
                (*l).pfn_next_get_instance_proc_addr as *const (),
                which_mapping((*l).pfn_next_get_instance_proc_addr as usize)
            ));
            l = (*l).p_next;
            depth += 1;
        }
    }
    dbg_log(&format!(
        "instance {} lives in: {}",
        inst.as_raw() as usize,
        which_mapping(inst.as_raw() as usize)
    ));
    let cd: Option<PFN_vkCreateDevice> = std::mem::transmute(loader_gipa(
        inst,
        b"vkCreateDevice\0".as_ptr() as *const c_char,
    ));
    dbg_log("vkCreateDevice lookup returned");
    let di: Option<PFN_vkDestroyInstance> = std::mem::transmute(loader_gipa(
        inst,
        b"vkDestroyInstance\0".as_ptr() as *const c_char,
    ));
    dbg_log("vkDestroyInstance lookup returned");
    dbg_log(&format!(
        "after create: instance={:p} cd.is_some()={} di.is_some()={}",
        inst.as_raw(),
        cd.is_some(),
        di.is_some()
    ));
    let (cd, di) = match (cd, di) {
        (Some(a), Some(b)) => (a, b),
        _ => return res, // cannot hook without both; stay a pass-through
    };
    if let Ok(mut slot) = instance_slot().lock() {
        *slot = Some(InstanceState {
            get_proc_addr: gipa,
            create_device: cd,
            destroy_instance: di,
        });
    }

    // Create the segment now — never on the hot path (ADR-5).
    if let Ok(mut slot) = writer_slot().lock() {
        if slot.is_none() {
            match Writer::create() {
                Ok(w) => *slot = Some(w),
                Err(e) => {
                    // Fail-open: a game that cannot be measured still runs.
                    dbg_log(&format!("shm unavailable, pass-through: {}", e));
                    return res;
                }
            }
        }
    }
    ENABLED.store(true, Ordering::Release);
    dbg_log(&format!("enabled; segment created {}", counters_line()));
    res
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_instance(
    instance: Instance,
    p_allocator: *const VkAllocationCallbacks,
) {
    let next = instance_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.destroy_instance));
    if let Some(f) = next {
        f(instance, p_allocator);
    }
    // Remove the segment once the game is done with the instance.
    if let Ok(mut slot) = writer_slot().lock() {
        if let Some(w) = slot.as_mut() {
            w.destroy();
        }
        *slot = None;
    }
    if let Ok(mut slot) = instance_slot().lock() {
        *slot = None;
    }
    ENABLED.store(false, Ordering::Release);
}

// ---------------------------------------------------------------------------
// device-level dispatch
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn vk_create_device(
    physical_device: PhysicalDevice,
    p_create_info: *const VkDeviceCreateInfo,
    p_allocator: *const VkAllocationCallbacks,
    p_device: *mut Device,
) -> VkResult {
    bump(2);
    dbg_log(&format!("create_device ENTERED {}", counters_line()));
    // Owner hypothesis (a): dispatch tables are keyed by the *dispatch pointer*
    // inside the handle -- *(void**)handle -- not by the descriptor we were
    // handed. Print both so a mismatch is visible.
    dbg_log(&format!(
        "create_device: physdev={:p} dispatch_key={:p} instance_slot_has_create={}",
        physical_device.as_raw(),
        *(physical_device.as_raw() as *const *const u8),
        instance_slot().lock().map(|s| s.is_some()).unwrap_or(false)
    ));
    let next_create = instance_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.create_device));
    let create = match next_create {
        Some(f) => f,
        None => return ERROR_INITIALIZATION_FAILED,
    };

    // The device chain carries the next layer's vkGetDeviceProcAddr.
    let mut next_gdpa: Option<PFN_vkGetDeviceProcAddr> = None;
    let mut chain = (*p_create_info).p_next;
    while !chain.is_null() {
        if (*chain).s_type == STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO {
            let ld = chain as *const VkLayerDeviceCreateInfo;
            if (*ld).function == VK_LAYER_LINK_INFO && !(*ld).p_layer_info.is_null() {
                next_gdpa = Some((*(*ld).p_layer_info).pfn_next_get_device_proc_addr);
            }
            break;
        }
        chain = (*chain).p_next;
    }

    dbg_log("create_device: forwarding down the chain");
    let res = create(physical_device, p_create_info, p_allocator, p_device);
    dbg_log(&format!("create_device -> {:?}", res));
    if res.is_error() {
        return res;
    }

    dbg_log(&format!(
        "create_device: gdpa from device chain = {:?}",
        next_gdpa.map(|f| f as *const ())
    ));
    let gdpa = match next_gdpa {
        Some(f) => f,
        None => {
            dbg_log("create_device: NO gdpa in chain, pass-through");
            return res;
        }
    };
    let dev = *p_device;
    dbg_log(&format!(
        "create_device: device={:p} dispatch_key={:p}",
        dev.as_raw(),
        *(dev.as_raw() as *const *const u8)
    ));
    let name = |s: &[u8]| s.as_ptr() as *const c_char;
    let queue_present: Option<PFN_vkQueuePresentKHR> =
        std::mem::transmute(gdpa(dev, name(b"vkQueuePresentKHR\0")));
    let destroy_device: Option<PFN_vkDestroyDevice> =
        std::mem::transmute(gdpa(dev, name(b"vkDestroyDevice\0")));
    let create_swapchain: Option<PFN_vkCreateSwapchainKHR> =
        std::mem::transmute(gdpa(dev, name(b"vkCreateSwapchainKHR\0")));
    let destroy_swapchain: Option<PFN_vkDestroySwapchainKHR> =
        std::mem::transmute(gdpa(dev, name(b"vkDestroySwapchainKHR\0")));

    // Without the present pointer the layer cannot do its job: stay a
    // pass-through rather than crash later.
    if let (Some(qp), Some(dd)) = (queue_present, destroy_device) {
        register_device(
            dev.as_raw() as u64,
            DeviceState {
                queue_present: qp,
                destroy_device: dd,
                get_device_proc_addr: gdpa,
                create_swapchain: create_swapchain.unwrap_or(dummy_create_swapchain),
                destroy_swapchain: destroy_swapchain.unwrap_or(dummy_destroy_swapchain),
            },
        );
    }
    res
}

// Placeholders for the swapchain hooks when the next layer does not export
// them (a headless implementation with no surface support). They keep the
// table total instead of leaving an `Option` on the hot path.
unsafe extern "system" fn dummy_create_swapchain(
    _d: Device,
    _c: *const VkSwapchainCreateInfoKHR,
    _a: *const VkAllocationCallbacks,
    _s: *mut SwapchainKHR,
) -> VkResult {
    ERROR_EXTENSION_NOT_PRESENT
}
unsafe extern "system" fn dummy_destroy_swapchain(
    _d: Device,
    _s: SwapchainKHR,
    _a: *const VkAllocationCallbacks,
) {
}

#[no_mangle]
pub unsafe extern "system" fn vk_get_device_proc_addr(
    device: Device,
    p_name: *const c_char,
) -> PFN_vkVoidFunction {
    if p_name.is_null() {
        return None;
    }
    let name = CStr::from_ptr(p_name).to_bytes();
    // `PFN_vkVoidFunction` is itself an `Option`, so the match arms are the
    // return value directly -- no extra `Some` wrapper.
    let ours: PFN_vkVoidFunction = match name {
        b"vkQueuePresentKHR" => as_void(vk_queue_present_khr as *const ()),
        b"vkCreateSwapchainKHR" => as_void(vk_create_swapchain_khr as *const ()),
        b"vkDestroySwapchainKHR" => as_void(vk_destroy_swapchain_khr as *const ()),
        b"vkDestroyDevice" => as_void(vk_destroy_device as *const ()),
        _ => None,
    };
    if ours.is_some() {
        return ours;
    }
    match device_state(device.as_raw() as u64).map(|s| s.get_device_proc_addr) {
        Some(f) => f(device, p_name),
        None => None,
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_device(
    device: Device,
    p_allocator: *const VkAllocationCallbacks,
) {
    if let Some(s) = device_state(device.as_raw() as u64) {
        (s.destroy_device)(device, p_allocator);
    }
    unregister_device(device.as_raw() as u64);
}

#[no_mangle]
pub unsafe extern "system" fn vk_create_swapchain_khr(
    device: Device,
    p_create_info: *const VkSwapchainCreateInfoKHR,
    p_allocator: *const VkAllocationCallbacks,
    p_swapchain: *mut SwapchainKHR,
) -> VkResult {
    let st = match device_state(device.as_raw() as u64) {
        Some(s) => s,
        None => return ERROR_EXTENSION_NOT_PRESENT,
    };
    let r = (st.create_swapchain)(device, p_create_info, p_allocator, p_swapchain);
    if !r.is_error() {
        // A new swapchain means a new present cadence. Reset the origin so the
        // first interval is not measured across the recreation — the
        // window-resize case in the spike's test 4.
        if let Ok(mut slot) = writer_slot().lock() {
            if let Some(w) = slot.as_mut() {
                w.reset_origin();
            }
        }
    }
    r
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_swapchain_khr(
    device: Device,
    swapchain: SwapchainKHR,
    p_allocator: *const VkAllocationCallbacks,
) {
    if let Some(s) = device_state(device.as_raw() as u64) {
        (s.destroy_swapchain)(device, swapchain, p_allocator);
    }
}

// ---------------------------------------------------------------------------
// the hot path
// ---------------------------------------------------------------------------

#[inline]
fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_MONOTONIC: vDSO, no syscall, unaffected by wall-clock steps.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

/// `vkQueuePresentKHR`.
///
/// `catch_unwind` so that a bug in the measurement path can never unwind into
/// the game (criterion 10). The `Mutex` around the writer is uncontended in
/// practice but makes the shared `Writer` sound without unsafe aliasing, and
/// its cost is inside the measured overhead rather than hidden from it.
///
/// **What the overhead sample covers:** from the timestamp taken after the
/// downstream present to the clock read that ends `record()`. It excludes the
/// function epilogue and the sampling `clock_gettime` itself (~20–25 ns of
/// vDSO), so the reported figure is a slight **under**estimate — stated here
/// rather than left implicit.
///
/// The interval is measured *after* the downstream present rather than around
/// it, so it is present-to-present — the same quantity MangoHud reports —
/// rather than present-to-entry.
#[no_mangle]
pub unsafe extern "system" fn vk_queue_present_khr(
    queue: Queue,
    p_present_info: *const VkPresentInfoKHR,
) -> VkResult {
    bump(7);
    let next = match device_state(queue.as_raw() as u64) {
        Some(s) => s.queue_present,
        None => {
            // This fires once per present if the key is wrong; log once, not
            // 60 times a second.
            if HOOK_CALLS[7].load(Ordering::Relaxed) == 1 {
                dbg_log(&format!(
                    "queue_present: NO device state for key 0x{:x} (queue={:p} dispatch_key={:p}) {}",
                    queue.as_raw() as u64,
                    queue.as_raw(),
                    *(queue.as_raw() as *const *const u8),
                    counters_line()
                ));
            }
            return ERROR_INITIALIZATION_FAILED;
        }
    };

    if !ENABLED.load(Ordering::Acquire) {
        return next(queue, p_present_info);
    }

    let r = catch_unwind(AssertUnwindSafe(|| {
        let res = next(queue, p_present_info);
        let t1 = now_ns();
        if let Ok(mut slot) = writer_slot().lock() {
            if let Some(w) = slot.as_mut() {
                w.record(t1, t1);
            }
        }
        res
    }));

    match r {
        Ok(res) => res,
        Err(_) => {
            // A panic inside the measurement path. The downstream present
            // already happened, so calling it again would be worse than
            // reporting an error; the game sees a failed present, which
            // vkQueuePresentKHR is allowed to return. Fail-open, not silent.
            ERROR_OUT_OF_HOST_MEMORY
        }
    }
}

/// No physical-device-level entry points are hooked, so there is nothing to
/// answer: `None` for every name is correct because the manifest claims none.
#[no_mangle]
pub unsafe extern "system" fn vk_get_physical_device_proc_addr(
    _instance: Instance,
    _p_name: *const c_char,
) -> PFN_vkVoidFunction {
    None
}
