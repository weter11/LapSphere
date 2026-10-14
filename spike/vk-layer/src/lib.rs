//! The layer: negotiation, the instance/device dispatch chains, and the
//! `vkQueuePresentKHR` hook.
//!
//! # What round 1 (PR #111) got wrong, and why this file looks like this
//!
//! The round-1 spike proved the segment was created and that `vkCreateInstance`
//! ran, and then reported `gipa=0 create_device=0` and concluded "the loader
//! never routes anything through the layer after instance creation". Reading
//! the loader's own source (`Vulkan-Loader` `vulkan-sdk-1.4.341.0`,
//! `loader/loader.c:4987-5301` and `:5403-5576`) shows three separate things:
//!
//! 1. **The counters were sampled in the wrong place.** The round-1 counter line
//!    was printed at the end of `vkCreateInstance`. The loader calls the layer's
//!    `vkGetInstanceProcAddr` at `loader.c:5283`
//!    (`loader_init_instance_core_dispatch_table`), i.e. *after* `vkCreateInstance`
//!    returns, and again at `:5504` (`nextGIPA(inst->instance, "vkCreateDevice")`)
//!    inside device creation. `gipa=0` was therefore not evidence about the
//!    loader at all — it was a measurement printed too early, from a point where
//!    0 is the only possible value. Counters here are printed at
//!    `vkDestroyDevice`, at `vkDestroyInstance`, and every 600 presents.
//! 2. **The chain link was not consumed.** The loader builds exactly **one**
//!    `VkLayerInstanceCreateInfo` node (`function == VK_LAYER_LINK_INFO`,
//!    `loader.c:5004-5009`) and threads the layers through it as a LIFO chain:
//!    for each layer it does `layer_instance_link_info[i].pNext = chain_info.u.pLayerInfo;`
//!    then `chain_info.u.pLayerInfo = &layer_instance_link_info[i];` (`:5113-5128`).
//!    The head is therefore *this* layer's link, and **the layer must consume it**
//!    (`chain_info->u.pLayerInfo = chain_info->u.pLayerInfo->pNext`) before calling
//!    down. Round 1 did not, so the next layer (`VK_LAYER_MESA_device_select`) saw
//!    its own link as "next" and called itself with a real instance handle: a
//!    user-space spin with no syscalls — exactly what the round-1 STATUS recorded,
//!    and then blamed on MESA. `vkCreateDevice`'s `VkLayerDeviceCreateInfo` chain
//!    works the same way (`:5480-5483`).
//! 3. **The "next" gipa must be the link's, not the loader terminator's.**
//!    Round 1 walked the link chain to its last entry (`libvulkan.so`) and used
//!    *that* gipa as "next". The terminator returns bottom-of-chain (ICD-level)
//!    functions, so everything routed through the layer afterwards bypassed the
//!    layer below it and the app hung on the first entry point that needs the
//!    loader's own trampoline.
//!
//! The manifest `functions` map is *not* the blocker: `loader.c:5043-5110` only
//! consults it when negotiation did not supply `vkGetInstanceProcAddr`, and the
//! two deprecation warnings it prints are cosmetic. `manifests/` carries both
//! variants and `TESTING.md` records the A/B result rather than the guess.
//!
//! # Fail-open
//!
//! Nothing here may change the game's behaviour. Every entry point is wrapped in
//! `catch_unwind`, no argument is ever modified, and every failure path calls the
//! next layer with the arguments unchanged. The exception is stated where it
//! happens: a missing chain link means there is no next function to call at all,
//! and the call then fails visibly instead of silently misrouting a game.
//!
//! # Handles
//!
//! Handles are stored as `usize`, not as `*mut c_void`: the state lives in
//! `static`s behind a `Mutex`, and a raw pointer would make those statics
//! neither `Send` nor `Sync`. The conversion happens at the boundary only.

pub mod buildid;
pub mod shm;
pub mod vkraw;

use std::ffi::c_char;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use buildid::{BUILD_ID, BUILD_MTIME};
use shm::Writer;
use vkraw::*;

/// `"vkCreateDevice\0"` etc. as a static pointer; no allocation on any path.
macro_rules! cname {
    ($s:literal) => {
        concat!($s, "\0").as_ptr() as *const c_char
    };
}

/// Read a `&str` from a C string without panicking on non-UTF-8.
unsafe fn cstr<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        return "<null>";
    }
    std::ffi::CStr::from_ptr(p).to_str().unwrap_or("<non-utf8>")
}

/// Reinterpret one entry-point pointer as another. A pure ABI cast, in one
/// place, so a call site cannot mistype it.
#[inline]
unsafe fn cast<T: Copy, U>(f: T) -> U {
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<U>());
    std::mem::transmute_copy::<T, U>(&f)
}

/// Turn one of our own entry points into the `PFN_vkVoidFunction` the loader
/// stores.
///
/// The argument is a raw pointer, never a value: a function **item** is
/// zero-sized, so `transmute_copy` from one sizes Dst > Src and panics — and
/// that panic lands inside a game process, where it aborts the game (measured
/// in this stage's first run with the generic version of this helper).
#[inline]
unsafe fn as_void(p: *const ()) -> PFN_vkVoidFunction {
    cast::<*const (), PFN_vkVoidFunction>(p)
}

/// A handle as a number, for logs and for the `usize` state maps.
#[inline]
fn h(p: *const std::ffi::c_void) -> usize {
    p as usize
}

fn now_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // CLOCK_MONOTONIC: vDSO, no syscall, unaffected by wall-clock steps.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    (ts.tv_sec as u64) * 1_000_000_000 + (ts.tv_nsec as u64)
}

// ---------------------------------------------------------------------------
// instrumentation
// ---------------------------------------------------------------------------

pub const LAYER_NAME: &str = "VK_LAYER_LAPSPHERE_frames";

/// Set from the environment in `vkCreateInstance`. The manifest's
/// `enable_environment` normally prevents the library from being loaded at all
/// without `LAPSPHERE_FRAMES=1`; this is the second gate, so a force-loaded
/// layer is a pass-through that creates no segment.
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Append one line to `$XDG_RUNTIME_DIR/lapsphere/spike.log`.
///
/// A file, not stderr: the layer runs inside somebody else's process, whose
/// stderr may be discarded, closed, or still showing an earlier build's output.
pub fn log_line(msg: &str) {
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
        let _ = writeln!(f, "[{} {}] {}", std::process::id(), now_ns(), msg);
    }
}

/// Per-hook entry counters, so "the hook was never called" is a measurement
/// rather than an inference from the absence of output.
pub static HOOK_CALLS: [AtomicU64; 12] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
pub const HOOK_NAMES: [&str; 12] = [
    "GetInstanceProcAddr",
    "CreateInstance",
    "CreateDevice",
    "GetDeviceProcAddr",
    "DestroyDevice",
    "CreateSwapchainKHR",
    "DestroySwapchainKHR",
    "QueuePresentKHR",
    "DestroyInstance",
    "GetPhysicalDeviceProcAddr",
    "GetDeviceQueue",
    "GetDeviceQueue2",
];
pub const IDX_GIPA: usize = 0;
pub const IDX_CREATE_INSTANCE: usize = 1;
pub const IDX_CREATE_DEVICE: usize = 2;
pub const IDX_GDPA: usize = 3;
pub const IDX_DESTROY_DEVICE: usize = 4;
pub const IDX_CREATE_SWAPCHAIN: usize = 5;
pub const IDX_DESTROY_SWAPCHAIN: usize = 6;
pub const IDX_PRESENT: usize = 7;
pub const IDX_DESTROY_INSTANCE: usize = 8;
pub const IDX_GPDPA: usize = 9;
pub const IDX_GET_DEVICE_QUEUE: usize = 10;
pub const IDX_GET_DEVICE_QUEUE2: usize = 11;

#[inline]
fn bump(i: usize) {
    HOOK_CALLS[i].fetch_add(1, Ordering::Relaxed);
}

pub fn counters_line() -> String {
    let v: Vec<String> = HOOK_NAMES
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{}={}", n, HOOK_CALLS[i].load(Ordering::Relaxed)))
        .collect();
    v.join(" ")
}

/// The first N names the loader asks for are logged in full, together with the
/// object each *name string* lives in — which identifies the caller (the
/// loader's own library, or a layer below us). This list is the evidence for
/// criterion 1: it shows the chain walk happening instead of inferring it from
/// the absence of output, which is what round 1 did.
static NAME_LOG_BUDGET: AtomicU64 = AtomicU64::new(0);
const NAME_LOG_LIMIT: u64 = 64;

/// Which mapped file (if any) contains `addr`. Turns "the pointer looks wrong"
/// into "the pointer is in *that* object", the only form of the bug that can be
/// reasoned about.
fn which_mapping(addr: usize) -> String {
    if addr == 0 {
        return "<null>".to_string();
    }
    let maps = match std::fs::read_to_string("/proc/self/maps") {
        Ok(m) => m,
        Err(e) => return format!("<maps unreadable: {}>", e),
    };
    for line in maps.lines() {
        let mut it = line.split_whitespace();
        let range = it.next().unwrap_or("");
        let _perms = it.next();
        let _off = it.next();
        let _dev = it.next();
        let _inode = it.next();
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

/// The `.so` this process actually mapped, from its own maps. The manifest
/// points at the library by absolute path, so "cargo finished" does not imply
/// "this is the code that ran" — round 1 twice mistook a stale build for a
/// behavioural result.
fn self_so_path() -> String {
    if let Ok(maps) = std::fs::read_to_string(format!("/proc/{}/maps", std::process::id())) {
        for line in maps.lines() {
            if line.contains("lapsphere_frames_layer") {
                if let Some(p) = line.split_whitespace().nth(5) {
                    return p.to_string();
                }
            }
        }
    }
    "<not mapped>".to_string()
}

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct InstanceState {
    handle: usize,
    next_gipa: PFN_vkGetInstanceProcAddr,
    next_gpdpa: PFN_GetPhysicalDeviceProcAddr,
    next_destroy_instance: PFN_vkDestroyInstance,
}

#[derive(Clone, Copy)]
struct DeviceState {
    handle: usize,
    next_gdpa: PFN_vkGetDeviceProcAddr,
    present: PFN_vkQueuePresentKHR,
    destroy_device: PFN_vkDestroyDevice,
    get_device_queue: PFN_vkGetDeviceQueue,
    get_device_queue2: PFN_vkGetDeviceQueue2,
    create_swapchain: PFN_vkCreateSwapchainKHR,
    destroy_swapchain: PFN_vkDestroySwapchainKHR,
}

static INSTANCES: OnceLock<Mutex<Vec<InstanceState>>> = OnceLock::new();
static DEVICES: OnceLock<Mutex<Vec<DeviceState>>> = OnceLock::new();
/// Queues learned from `vkGetDeviceQueue{,2}`: the present hook receives only a
/// queue handle and must find the device's next-layer functions from it.
static QUEUES: OnceLock<Mutex<Vec<(usize, usize)>>> = OnceLock::new();
/// Last present function seen at device creation: the fallback for a queue we
/// never saw, so an unmapped queue degrades to "still forwards" rather than to
/// a NULL call.
static FALLBACK_PRESENT: AtomicUsize = AtomicUsize::new(0);
static LAST_DEVICE: AtomicUsize = AtomicUsize::new(0);
/// The instance handle from the last successful `vkCreateInstance`; it is the
/// argument for chained `gipa` lookups during device creation, exactly as the
/// loader itself does it (`loader.c:5504`).
static LAST_INSTANCE: AtomicUsize = AtomicUsize::new(0);

static WRITER: OnceLock<Mutex<Option<Writer>>> = OnceLock::new();

fn writer_slot() -> &'static Mutex<Option<Writer>> {
    WRITER.get_or_init(|| Mutex::new(None))
}

fn instances() -> &'static Mutex<Vec<InstanceState>> {
    INSTANCES.get_or_init(|| Mutex::new(Vec::new()))
}

fn devices() -> &'static Mutex<Vec<DeviceState>> {
    DEVICES.get_or_init(|| Mutex::new(Vec::new()))
}

fn queues() -> &'static Mutex<Vec<(usize, usize)>> {
    QUEUES.get_or_init(|| Mutex::new(Vec::new()))
}

fn instance_state(handle: usize) -> Option<InstanceState> {
    instances()
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|s| s.handle == handle).copied())
}

fn device_state(handle: usize) -> Option<DeviceState> {
    devices()
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|s| s.handle == handle).copied())
}

/// The state to use when the loader hands us a handle we did not register.
/// Logged once; falling back keeps the layer in the chain, which is what
/// fail-open requires.
fn device_state_else_last(handle: usize, who: &str) -> Option<DeviceState> {
    if handle != 0 {
        if let Some(s) = device_state(handle) {
            return Some(s);
        }
    }
    static WARNED: AtomicBool = AtomicBool::new(false);
    if !WARNED.swap(true, Ordering::Relaxed) {
        log_line(&format!(
            "{}: device {:#x} is not in the registry ({} known, last {:#x}) -- using the last one",
            who,
            handle,
            devices().lock().map(|v| v.len()).unwrap_or(0),
            LAST_DEVICE.load(Ordering::Relaxed)
        ));
    }
    devices().lock().ok().and_then(|v| v.last().copied())
}

// ---------------------------------------------------------------------------
// chain-info walking
// ---------------------------------------------------------------------------

/// Find the loader's `VK_LAYER_LINK_INFO` node in an instance create chain.
///
/// The node is **not** first: the loader prepends three more
/// (`VK_LOADER_FEATURES`, `VK_LOADER_LAYER_CREATE_DEVICE_CALLBACK`,
/// `VK_LOADER_DATA_CALLBACK`, `loader.c:5222-5241`) above the link node it
/// created at `:5004`. A walk that stops at the first matching `sType` reads
/// `function = 3` and concludes there is no next layer.
unsafe fn find_instance_chain_info(mut node: *const std::ffi::c_void) -> *mut VkLayerInstanceCreateInfo {
    let mut depth = 0;
    while !node.is_null() && depth < 64 {
        let base = node as *const u8;
        let s_type = std::ptr::read_unaligned(base as *const i32);
        if s_type == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO {
            let function = std::ptr::read_unaligned(base.add(16) as *const i32);
            if function == VK_LAYER_LINK_INFO {
                return node as *mut VkLayerInstanceCreateInfo;
            }
        }
        node = std::ptr::read_unaligned(base.add(8) as *const *const std::ffi::c_void);
        depth += 1;
    }
    std::ptr::null_mut()
}

/// Same for the device chain (`VkLayerDeviceCreateInfo`, `loader.c:5506-5538`).
unsafe fn find_device_chain_info(mut node: *const std::ffi::c_void) -> *mut VkLayerDeviceCreateInfo {
    let mut depth = 0;
    while !node.is_null() && depth < 64 {
        let base = node as *const u8;
        let s_type = std::ptr::read_unaligned(base as *const i32);
        if s_type == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO {
            let function = std::ptr::read_unaligned(base.add(16) as *const i32);
            if function == VK_LAYER_LINK_INFO {
                return node as *mut VkLayerDeviceCreateInfo;
            }
        }
        node = std::ptr::read_unaligned(base.add(8) as *const *const std::ffi::c_void);
        depth += 1;
    }
    std::ptr::null_mut()
}

/// `u.pLayerInfo` sits at offset 24 in both create-info structures.
#[inline]
unsafe fn read_link_ptr<T>(chain: *mut std::ffi::c_void) -> *mut T {
    std::ptr::read_unaligned((chain as *const u8).add(24) as *const *mut T)
}

#[inline]
unsafe fn write_link_ptr<T>(chain: *mut std::ffi::c_void, link: *mut T) {
    std::ptr::write_unaligned((chain as *mut u8).add(24) as *mut *mut T, link);
}

// ---------------------------------------------------------------------------
// negotiation
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    p: *mut VkNegotiateLayerInterface,
) -> VkResult {
    if p.is_null() {
        return VK_ERROR_INITIALIZATION_FAILED;
    }
    let s = &mut *p;
    let offered = s.loader_layer_interface_version;
    // Clamp to what the loader offers: v2 adds the physical-device slot.
    s.loader_layer_interface_version = if offered >= CURRENT_LOADER_LAYER_INTERFACE_VERSION {
        CURRENT_LOADER_LAYER_INTERFACE_VERSION
    } else {
        1
    };
    // `sType`/`pNext` belong to the loader (`loader.c:4793-4795` sets them
    // before the call); a layer must not rewrite them.
    s.pfn_get_instance_proc_addr = Some(vk_get_instance_proc_addr);
    s.pfn_get_device_proc_addr = Some(vk_get_device_proc_addr);
    s.pfn_get_physical_device_proc_addr = if s.loader_layer_interface_version >= 2 {
        Some(vk_get_physical_device_proc_addr)
    } else {
        None
    };

    log_line(&format!(
        "negotiate: loader offered v{}, answering v{}, build {} mtime {} so={}",
        offered,
        s.loader_layer_interface_version,
        BUILD_ID,
        BUILD_MTIME,
        self_so_path()
    ));
    eprintln!(
        "[lapsphere-frames] negotiate offered={} answered={} build={}",
        offered, s.loader_layer_interface_version, BUILD_ID
    );
    VK_SUCCESS
}

/// The ICD spelling of the same entry point. The loader looks for the layer
/// name only (`loader.c:5047`); exporting both costs one symbol and removes a
/// whole class of "which name was it" doubt.
#[no_mangle]
pub unsafe extern "system" fn vk_icdNegotiateLoaderLayerInterfaceVersion(
    p: *mut VkNegotiateLayerInterface,
) -> VkResult {
    vkNegotiateLoaderLayerInterfaceVersion(p)
}

// ---------------------------------------------------------------------------
// instance dispatch
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn vk_get_instance_proc_addr(
    instance: VkInstance,
    name: *const c_char,
) -> PFN_vkVoidFunction {
    bump(IDX_GIPA);
    let n = cstr(name);
    let budget = NAME_LOG_BUDGET.fetch_add(1, Ordering::Relaxed);
    if budget < NAME_LOG_LIMIT {
        let ret = match n {
            "vkGetInstanceProcAddr" => "ours",
            "vkCreateInstance" => "ours",
            "vkDestroyInstance" => "ours",
            "vkCreateDevice" => "ours",
            "vkGetDeviceProcAddr" => "ours",
            "vk_layerGetPhysicalDeviceProcAddr" => "ours",
            _ => "chained down",
        };
        log_line(&format!(
            "gipa[{}](instance={:#x} known={} , \"{}\") -> {} (name string in {})",
            budget,
            h(instance),
            instance_state(h(instance)).is_some(),
            n,
            ret,
            which_mapping(name as usize)
        ));
    }
    match n {
        "vkGetInstanceProcAddr" => {
            as_void(vk_get_instance_proc_addr as *const ())
        }
        "vkCreateInstance" => as_void(vk_create_instance as *const ()),
        "vkDestroyInstance" => as_void(vk_destroy_instance as *const ()),
        "vkGetDeviceProcAddr" => as_void(vk_get_device_proc_addr as *const ()),
        "vkCreateDevice" => as_void(vk_create_device as *const ()),
        "vk_layerGetPhysicalDeviceProcAddr" => as_void(vk_get_physical_device_proc_addr as *const ()),
        _ => match instance_state(h(instance)) {
            Some(st) => match st.next_gipa {
                Some(g) => g(instance, name),
                None => None,
            },
            // NULL instance: only global functions are legal, and the loader
            // itself returns NULL for everything else.
            None => None,
        },
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_get_physical_device_proc_addr(
    instance: VkInstance,
    name: *const c_char,
) -> PFN_vkVoidFunction {
    bump(IDX_GPDPA);
    if let Some(st) = instance_state(h(instance)) {
        if let Some(g) = st.next_gpdpa {
            return g(instance, name);
        }
        if let Some(g) = st.next_gipa {
            return g(instance, name);
        }
    }
    None
}

#[no_mangle]
pub unsafe extern "system" fn vk_create_instance(
    create_info: *const VkInstanceCreateInfo,
    allocator: *const std::ffi::c_void,
    instance_out: *mut VkInstance,
) -> VkResult {
    bump(IDX_CREATE_INSTANCE);
    let r = catch_unwind(AssertUnwindSafe(|| {
        if create_info.is_null() || instance_out.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let enabled = std::env::var("LAPSPHERE_FRAMES")
            .map(|v| v == "1")
            .unwrap_or(false);
        ENABLED.store(enabled, Ordering::Relaxed);

        let p_next = std::ptr::read_unaligned(std::ptr::addr_of!((*create_info).p_next));
        let chain = find_instance_chain_info(p_next);
        if chain.is_null() {
            log_line("create_instance: NO VK_LAYER_LINK_INFO node in the create chain");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let link = read_link_ptr::<VkLayerInstanceLink>(chain as *mut std::ffi::c_void);
        if link.is_null() {
            log_line("create_instance: link node is NULL");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let next_gipa = (*link).pfn_next_get_instance_proc_addr;
        let next_gpdpa = (*link).pfn_next_get_physical_device_proc_addr;
        // Consume our link: the next layer must see *its* node as the head.
        write_link_ptr(chain as *mut std::ffi::c_void, (*link).p_next);

        log_line(&format!(
            "create_instance: chain={:#x} link={:#x} next_gipa={:#x} ({}) next_gpdpa={:#x} link.pNext={:#x} enabled={}",
            chain as usize,
            link as usize,
            next_gipa.map(|f| f as *const () as usize).unwrap_or(0),
            which_mapping(next_gipa.map(|f| f as *const () as usize).unwrap_or(0)),
            next_gpdpa.map(|f| f as *const () as usize).unwrap_or(0),
            (*link).p_next as usize,
            enabled
        ));

        let fp_create: PFN_vkCreateInstance = match next_gipa {
            Some(g) => cast::<PFN_vkVoidFunction, PFN_vkCreateInstance>(g(
                std::ptr::null_mut(),
                cname!("vkCreateInstance"),
            )),
            None => None,
        };
        let fp_create = match fp_create {
            Some(f) => f,
            None => {
                log_line("create_instance: next layer has no vkCreateInstance");
                return VK_ERROR_INITIALIZATION_FAILED;
            }
        };
        let res = fp_create(create_info, allocator, instance_out);
        if res != VK_SUCCESS {
            log_line(&format!("create_instance: next layer returned {}", res));
            return res;
        }
        let instance = *instance_out;
        LAST_INSTANCE.store(h(instance), Ordering::Relaxed);
        let next_destroy: PFN_vkDestroyInstance = match next_gipa {
            Some(g) => cast::<PFN_vkVoidFunction, PFN_vkDestroyInstance>(g(
                instance,
                cname!("vkDestroyInstance"),
            )),
            None => None,
        };

        if let Ok(mut v) = instances().lock() {
            v.push(InstanceState {
                handle: h(instance),
                next_gipa,
                next_gpdpa,
                next_destroy_instance: next_destroy,
            });
        }

        if enabled {
            match Writer::create() {
                Ok(w) => {
                    if let Ok(mut slot) = writer_slot().lock() {
                        *slot = Some(w);
                    }
                    log_line(&format!(
                        "create_instance: instance={:#x} segment created for pid {}",
                        h(instance),
                        std::process::id()
                    ));
                }
                Err(e) => log_line(&format!("create_instance: Writer::create failed: {}", e)),
            }
        } else {
            log_line("create_instance: LAPSPHERE_FRAMES is not 1 -- pass-through, no segment");
        }
        res
    }));
    match r {
        Ok(res) => res,
        Err(_) => {
            log_line("PANIC in vk_create_instance (caught; fail-open)");
            VK_ERROR_INITIALIZATION_FAILED
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_instance(
    instance: VkInstance,
    allocator: *const std::ffi::c_void,
) {
    bump(IDX_DESTROY_INSTANCE);
    let next = instance_state(h(instance)).and_then(|s| s.next_destroy_instance);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        log_line(&format!("destroy_instance: {}", counters_line()));
        if let Ok(mut slot) = writer_slot().lock() {
            if let Some(w) = slot.as_mut() {
                // The final interval count, measured rather than derived: the
                // segment is unlinked a few lines below, so this is the only
                // point at which it can be read from the writer itself. It must
                // equal QueuePresentKHR − 1 (the first present of the cadence
                // writes no interval), and verify.sh asserts that.
                log_line(&format!(
                    "writer_final: intervals={} dropped={} presents={}",
                    w.frames(),
                    w.dropped(),
                    HOOK_CALLS[IDX_PRESENT].load(Ordering::Relaxed)
                ));
                w.destroy();
            }
            *slot = None;
        }
        if let Ok(mut v) = instances().lock() {
            v.retain(|s| s.handle != h(instance));
        }
    }));
    match next {
        Some(f) => f(instance, allocator),
        None => log_line("destroy_instance: no next vkDestroyInstance"),
    }
}

// ---------------------------------------------------------------------------
// device dispatch
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "system" fn vk_get_device_proc_addr(
    device: VkDevice,
    name: *const c_char,
) -> PFN_vkVoidFunction {
    bump(IDX_GDPA);
    let n = cstr(name);
    match n {
        "vkGetDeviceProcAddr" => as_void(vk_get_device_proc_addr as *const ()),
        "vkQueuePresentKHR" => as_void(vk_queue_present_khr as *const ()),
        "vkDestroyDevice" => as_void(vk_destroy_device as *const ()),
        "vkGetDeviceQueue" => as_void(vk_get_device_queue as *const ()),
        "vkGetDeviceQueue2" => as_void(vk_get_device_queue2 as *const ()),
        "vkCreateSwapchainKHR" => as_void(vk_create_swapchain_khr as *const ()),
        "vkDestroySwapchainKHR" => as_void(vk_destroy_swapchain_khr as *const ()),
        _ => match device_state_else_last(h(device), "gdpa") {
            Some(st) => match st.next_gdpa {
                Some(g) => g(device, name),
                None => None,
            },
            None => None,
        },
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_create_device(
    physical_device: VkPhysicalDevice,
    create_info: *const VkDeviceCreateInfo,
    allocator: *const std::ffi::c_void,
    device_out: *mut VkDevice,
) -> VkResult {
    bump(IDX_CREATE_DEVICE);
    let r = catch_unwind(AssertUnwindSafe(|| {
        if create_info.is_null() || device_out.is_null() {
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let p_next = std::ptr::read_unaligned(std::ptr::addr_of!((*create_info).p_next));
        let chain = find_device_chain_info(p_next);
        if chain.is_null() {
            log_line("create_device: NO VK_LAYER_LINK_INFO node in the device create chain");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let link = read_link_ptr::<VkLayerDeviceLink>(chain as *mut std::ffi::c_void);
        if link.is_null() {
            log_line("create_device: link node is NULL");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
        let next_gipa = (*link).pfn_next_get_instance_proc_addr;
        let next_gdpa = (*link).pfn_next_get_device_proc_addr;
        write_link_ptr(chain as *mut std::ffi::c_void, (*link).p_next);

        let instance = LAST_INSTANCE.load(Ordering::Relaxed);
        log_line(&format!(
            "create_device: chain={:#x} link={:#x} next_gipa={:#x} next_gdpa={:#x} ({}) instance={:#x}",
            chain as usize,
            link as usize,
            next_gipa.map(|f| f as *const () as usize).unwrap_or(0),
            next_gdpa.map(|f| f as *const () as usize).unwrap_or(0),
            which_mapping(next_gdpa.map(|f| f as *const () as usize).unwrap_or(0)),
            instance
        ));

        let fp_create: PFN_vkCreateDevice = match next_gipa {
            Some(g) => {
                let p = g(instance as VkInstance, cname!("vkCreateDevice"));
                let p = if p.is_some() {
                    p
                } else {
                    // Some layers answer a real instance handle only after
                    // their own instance dispatch is set up; the global form is
                    // the fallback the loader itself relies on.
                    g(std::ptr::null_mut(), cname!("vkCreateDevice"))
                };
                cast::<PFN_vkVoidFunction, PFN_vkCreateDevice>(p)
            }
            None => None,
        };
        let fp_create = match fp_create {
            Some(f) => f,
            None => {
                log_line("create_device: no next vkCreateDevice found");
                return VK_ERROR_INITIALIZATION_FAILED;
            }
        };
        let res = fp_create(physical_device, create_info, allocator, device_out);
        if res != VK_SUCCESS {
            log_line(&format!("create_device: next layer returned {}", res));
            return res;
        }
        let device = *device_out;
        let look = |name: *const c_char| -> PFN_vkVoidFunction {
            match next_gdpa {
                Some(g) => g(device, name),
                None => None,
            }
        };
        let st = DeviceState {
            handle: h(device),
            next_gdpa,
            present: cast::<PFN_vkVoidFunction, PFN_vkQueuePresentKHR>(look(cname!(
                "vkQueuePresentKHR"
            ))),
            destroy_device: cast::<PFN_vkVoidFunction, PFN_vkDestroyDevice>(look(cname!(
                "vkDestroyDevice"
            ))),
            get_device_queue: cast::<PFN_vkVoidFunction, PFN_vkGetDeviceQueue>(look(cname!(
                "vkGetDeviceQueue"
            ))),
            get_device_queue2: cast::<PFN_vkVoidFunction, PFN_vkGetDeviceQueue2>(look(cname!(
                "vkGetDeviceQueue2"
            ))),
            create_swapchain: cast::<PFN_vkVoidFunction, PFN_vkCreateSwapchainKHR>(look(cname!(
                "vkCreateSwapchainKHR"
            ))),
            destroy_swapchain: cast::<PFN_vkVoidFunction, PFN_vkDestroySwapchainKHR>(look(cname!(
                "vkDestroySwapchainKHR"
            ))),
        };
        log_line(&format!(
            "create_device: device={:#x} next_present={:#x} ({}) next_queue={:#x} next_swapchain={:#x}",
            h(device),
            st.present.map(|f| f as *const () as usize).unwrap_or(0),
            which_mapping(st.present.map(|f| f as *const () as usize).unwrap_or(0)),
            st.get_device_queue.map(|f| f as *const () as usize).unwrap_or(0),
            st.create_swapchain.map(|f| f as *const () as usize).unwrap_or(0)
        ));
        if let Some(p) = st.present {
            FALLBACK_PRESENT.store(p as usize, Ordering::Relaxed);
        }
        LAST_DEVICE.store(h(device), Ordering::Relaxed);
        if let Ok(mut v) = devices().lock() {
            v.retain(|s| s.handle != h(device));
            v.push(st);
        }
        res
    }));
    match r {
        Ok(res) => res,
        Err(_) => {
            log_line("PANIC in vk_create_device (caught; fail-open)");
            VK_ERROR_INITIALIZATION_FAILED
        }
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_device(
    device: VkDevice,
    allocator: *const std::ffi::c_void,
) {
    bump(IDX_DESTROY_DEVICE);
    let st = device_state_else_last(h(device), "destroy_device");
    let _ = catch_unwind(AssertUnwindSafe(|| {
        log_line(&format!("destroy_device: {}", counters_line()));
        if let Ok(mut v) = devices().lock() {
            v.retain(|s| s.handle != h(device));
        }
        if let Ok(mut q) = queues().lock() {
            q.retain(|(_, d)| *d != h(device));
        }
    }));
    match st.and_then(|s| s.destroy_device) {
        Some(f) => f(device, allocator),
        None => log_line("destroy_device: no next vkDestroyDevice"),
    }
}

// ---------------------------------------------------------------------------
// queue / swapchain / present
// ---------------------------------------------------------------------------

/// Record `queue -> device`, so the present hook can find its device.
unsafe fn note_queue(queue: VkQueue, device: VkDevice, who: &str) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if queue.is_null() {
            return;
        }
        if let Ok(mut v) = queues().lock() {
            v.retain(|(qh, _)| *qh != h(queue));
            v.push((h(queue), h(device)));
        }
        log_line(&format!(
            "{}: queue {:#x} -> device {:#x}",
            who,
            h(queue),
            h(device)
        ));
    }));
}

#[no_mangle]
pub unsafe extern "system" fn vk_get_device_queue(
    device: VkDevice,
    family_index: u32,
    queue_index: u32,
    queue_out: *mut VkQueue,
) {
    bump(IDX_GET_DEVICE_QUEUE);
    let st = device_state_else_last(h(device), "get_device_queue");
    match st.and_then(|s| s.get_device_queue) {
        Some(f) => f(device, family_index, queue_index, queue_out),
        None => log_line("get_device_queue: no next vkGetDeviceQueue"),
    }
    if !queue_out.is_null() {
        note_queue(*queue_out, device, "get_device_queue");
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_get_device_queue2(
    device: VkDevice,
    info: *const std::ffi::c_void,
    queue_out: *mut VkQueue,
) {
    bump(IDX_GET_DEVICE_QUEUE2);
    let st = device_state_else_last(h(device), "get_device_queue2");
    match st.and_then(|s| s.get_device_queue2) {
        Some(f) => f(device, info, queue_out),
        None => log_line("get_device_queue2: no next vkGetDeviceQueue2"),
    }
    if !queue_out.is_null() {
        note_queue(*queue_out, device, "get_device_queue2");
    }
}

#[no_mangle]
pub unsafe extern "system" fn vk_create_swapchain_khr(
    device: VkDevice,
    create_info: *const std::ffi::c_void,
    allocator: *const std::ffi::c_void,
    swapchain_out: *mut u64,
) -> VkResult {
    bump(IDX_CREATE_SWAPCHAIN);
    let st = device_state_else_last(h(device), "create_swapchain");
    let res = match st.and_then(|s| s.create_swapchain) {
        Some(f) => f(device, create_info, allocator, swapchain_out),
        None => {
            log_line("create_swapchain: no next vkCreateSwapchainKHR");
            return VK_ERROR_INITIALIZATION_FAILED;
        }
    };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if res == VK_SUCCESS {
            log_line(&format!(
                "create_swapchain: device {:#x} -> swapchain {} (a new cadence: present origin reset)",
                h(device),
                *swapchain_out
            ));
            if let Ok(mut slot) = writer_slot().lock() {
                if let Some(w) = slot.as_mut() {
                    // A new swapchain ends the old cadence: the gap across the
                    // recreation is not a frame time (ADR-5).
                    w.reset_origin();
                }
            }
        }
    }));
    res
}

#[no_mangle]
pub unsafe extern "system" fn vk_destroy_swapchain_khr(
    device: VkDevice,
    swapchain: u64,
    allocator: *const std::ffi::c_void,
) {
    bump(IDX_DESTROY_SWAPCHAIN);
    let st = device_state_else_last(h(device), "destroy_swapchain");
    match st.and_then(|s| s.destroy_swapchain) {
        Some(f) => f(device, swapchain, allocator),
        None => log_line("destroy_swapchain: no next vkDestroySwapchainKHR"),
    }
}

/// The next-layer present for a queue: by the queue table first, then the last
/// device, then the last present ever seen. Every step is a fallback that keeps
/// the call forwarding; a queue we cannot place at all is logged, not dropped.
unsafe fn next_present_for(queue: VkQueue) -> PFN_vkQueuePresentKHR {
    let dev = queues()
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|(q, _)| *q == h(queue)).map(|(_, d)| *d));
    if let Some(d) = dev {
        if let Some(s) = device_state(d) {
            if s.present.is_some() {
                return s.present;
            }
        }
    }
    if let Some(s) = device_state_else_last(0, "present").and_then(|s| s.present) {
        return Some(s);
    }
    cast::<usize, PFN_vkQueuePresentKHR>(FALLBACK_PRESENT.load(Ordering::Relaxed))
}

/// The hook this whole stage exists for.
///
/// Timing convention: the timestamp is taken **at entry**, so the interval
/// recorded is present-to-present as the application issues it — the boundary
/// `fps` is normally computed from. (Round 1 timed *after* the call down
/// returned, which is the same boundary shifted by one layer's work; the choice
/// is stated because the two conventions are not comparable.)
#[no_mangle]
pub unsafe extern "system" fn vk_queue_present_khr(
    queue: VkQueue,
    present_info: *const VkPresentInfoKHR,
) -> VkResult {
    bump(IDX_PRESENT);
    let t = now_ns();
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Ok(mut slot) = writer_slot().lock() {
            if let Some(w) = slot.as_mut() {
                w.record(t, t);
                let frames = w.frames();
                if frames > 0 && frames % 600 == 0 {
                    log_line(&format!("present: {} | {}", frames, counters_line()));
                }
            }
        }
    }));

    // Called down outside the closure: the layer must not hold its own lock
    // across the next layer's present.
    match next_present_for(queue) {
        Some(f) => f(queue, present_info),
        None => {
            static WARNED: AtomicBool = AtomicBool::new(false);
            if !WARNED.swap(true, Ordering::Relaxed) {
                log_line("present: NO next vkQueuePresentKHR -- cannot forward");
            }
            VK_ERROR_INITIALIZATION_FAILED
        }
    }
}

/// Read-only accessors used by `TESTING.md`'s reproduction recipe and by a test
/// that wants the counters without parsing the log.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn present_calls() -> u64 {
    HOOK_CALLS[IDX_PRESENT].load(Ordering::Relaxed)
}
