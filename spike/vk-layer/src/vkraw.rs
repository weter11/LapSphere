//! Raw Vulkan + loader declarations, hand-written.
//!
//! `ash` was tried in the round-1 spike and dropped: it does not bind the
//! loader structures (`VkNegotiateLayerInterface`, `VkLayerInstanceCreateInfo`)
//! and its API churn was a larger risk than ~200 lines of `#[repr(C)]`.
//!
//! The layouts below follow **Vulkan-Headers `vulkan-sdk-1.4.341.0`**
//! `include/vulkan/vk_layer.h` (the header matching the loader installed on
//! this host, `/usr/lib/x86_64-linux-gnu/libvulkan.so.1.4.341`), and the
//! static assertions at the bottom of this file turn a layout mistake into a
//! compile error instead of a segfault inside a game process.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::ffi::{c_char, c_void};

pub type VkResult = i32;
pub const VK_SUCCESS: VkResult = 0;
pub const VK_ERROR_INITIALIZATION_FAILED: VkResult = -3;

/// A dispatchable handle. The layer never dereferences these.
pub type VkInstance = *mut c_void;
pub type VkDevice = *mut c_void;
pub type VkPhysicalDevice = *mut c_void;
pub type VkQueue = *mut c_void;

/// `VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO` / `..._DEVICE_CREATE_INFO`.
/// These are loader structure types; their values are reserved and stable.
pub const VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO: i32 = 47;
pub const VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO: i32 = 48;

/// `VkLayerFunction` (vk_layer.h:65).
pub const VK_LAYER_LINK_INFO: i32 = 0;
pub const VK_LOADER_DATA_CALLBACK: i32 = 1;
pub const VK_LOADER_LAYER_CREATE_DEVICE_CALLBACK: i32 = 2;
pub const VK_LOADER_FEATURES: i32 = 3;

/// `LAYER_NEGOTIATE_INTERFACE_STRUCT`.
pub const LAYER_NEGOTIATE_INTERFACE_STRUCT: i32 = 1;
/// `CURRENT_LOADER_LAYER_INTERFACE_VERSION`.
pub const CURRENT_LOADER_LAYER_INTERFACE_VERSION: u32 = 2;

/// A nullable function pointer, as C spells `PFN_vkVoidFunction`.
/// `Option<fn>` is FFI-safe (the null-pointer niche) and makes the loader's
/// "this entry point does not exist" answer impossible to ignore.
pub type PFN_vkVoidFunction = Option<unsafe extern "system" fn()>;

pub type PFN_vkGetInstanceProcAddr =
    Option<unsafe extern "system" fn(VkInstance, *const c_char) -> PFN_vkVoidFunction>;
pub type PFN_vkGetDeviceProcAddr =
    Option<unsafe extern "system" fn(VkDevice, *const c_char) -> PFN_vkVoidFunction>;
pub type PFN_GetPhysicalDeviceProcAddr =
    Option<unsafe extern "system" fn(VkInstance, *const c_char) -> PFN_vkVoidFunction>;

pub type PFN_vkNegotiateLoaderLayerInterfaceVersion =
    Option<unsafe extern "system" fn(*mut VkNegotiateLayerInterface) -> VkResult>;

// ---------------------------------------------------------------------------
// negotiation
// ---------------------------------------------------------------------------

/// `VkNegotiateLayerInterface` (vk_layer.h:42). 48 bytes on LP64.
#[repr(C)]
pub struct VkNegotiateLayerInterface {
    pub s_type: i32,
    pub p_next: *mut c_void,
    pub loader_layer_interface_version: u32,
    pub pfn_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_get_device_proc_addr: PFN_vkGetDeviceProcAddr,
    pub pfn_get_physical_device_proc_addr: PFN_GetPhysicalDeviceProcAddr,
}

// ---------------------------------------------------------------------------
// instance chain
// ---------------------------------------------------------------------------

/// `VkLayerInstanceLink` (vk_layer.h:72).
///
/// The *first* member is `pNext` — round 1 read it as a struct of two function
/// pointers and called through address `0x3`. `pfnNextGetPhysicalDeviceProcAddr`
/// replaced the old `pfnNextGetDeviceProcAddr` slot when the loader added the
/// v2 interface; both are pointer-sized, but the *name* matters here because
/// this is the member the loader fills with `loader_gpdpa_instance_terminator`.
#[repr(C)]
pub struct VkLayerInstanceLink {
    pub p_next: *mut VkLayerInstanceLink,
    pub pfn_next_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_physical_device_proc_addr: PFN_GetPhysicalDeviceProcAddr,
}

/// The `u` union of `VkLayerInstanceCreateInfo`; 16 bytes (the two-pointer
/// `layerDevice` arm is the widest).
#[repr(C)]
pub union VkLayerInstanceCreateInfoU {
    pub p_layer_info: *mut VkLayerInstanceLink,
    pub pfn_set_instance_loader_data: *const c_void,
    pub layer_device: [*const c_void; 2],
    pub loader_features: u32,
}

/// `VkLayerInstanceCreateInfo` (vk_layer.h:103).
#[repr(C)]
pub struct VkLayerInstanceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub function: i32,
    pub u: VkLayerInstanceCreateInfoU,
}

// ---------------------------------------------------------------------------
// device chain
// ---------------------------------------------------------------------------

/// `VkLayerDeviceLink` (vk_layer.h:118).
#[repr(C)]
pub struct VkLayerDeviceLink {
    pub p_next: *mut VkLayerDeviceLink,
    pub pfn_next_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_device_proc_addr: PFN_vkGetDeviceProcAddr,
}

#[repr(C)]
pub union VkLayerDeviceCreateInfoU {
    pub p_layer_info: *mut VkLayerDeviceLink,
    pub pfn_set_device_loader_data: *const c_void,
}

/// `VkLayerDeviceCreateInfo` (vk_layer.h:124).
#[repr(C)]
pub struct VkLayerDeviceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub function: i32,
    pub u: VkLayerDeviceCreateInfoU,
}

// ---------------------------------------------------------------------------
// the application-facing structures we must read
// ---------------------------------------------------------------------------

/// `VkInstanceCreateInfo` — only `p_next` is read, but the whole struct is
/// declared so an offset assertion can guard the one field we use.
#[repr(C)]
pub struct VkInstanceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub p_application_info: *const c_void,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}

/// `VkDeviceCreateInfo`.
#[repr(C)]
pub struct VkDeviceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub flags: u32,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const c_void,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    pub p_enabled_features: *const c_void,
}

/// `VkPresentInfoKHR`. Passed straight through untouched; declared so the
/// present hook can count swapchains without guessing at the ABI.
#[repr(C)]
pub struct VkPresentInfoKHR {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub wait_semaphore_count: u32,
    pub p_wait_semaphores: *const c_void,
    pub swapchain_count: u32,
    pub p_swapchains: *const u64,
    pub p_image_indices: *const u32,
    pub p_results: *mut VkResult,
}

// ---------------------------------------------------------------------------
// the entry points we implement
// ---------------------------------------------------------------------------

pub type PFN_vkCreateInstance = Option<
    unsafe extern "system" fn(
        *const VkInstanceCreateInfo,
        *const c_void,
        *mut VkInstance,
    ) -> VkResult,
>;
pub type PFN_vkDestroyInstance =
    Option<unsafe extern "system" fn(VkInstance, *const c_void)>;
pub type PFN_vkCreateDevice = Option<
    unsafe extern "system" fn(
        VkPhysicalDevice,
        *const VkDeviceCreateInfo,
        *const c_void,
        *mut VkDevice,
    ) -> VkResult,
>;
pub type PFN_vkDestroyDevice = Option<unsafe extern "system" fn(VkDevice, *const c_void)>;
pub type PFN_vkGetDeviceQueue =
    Option<unsafe extern "system" fn(VkDevice, u32, u32, *mut VkQueue)>;
pub type PFN_vkGetDeviceQueue2 =
    Option<unsafe extern "system" fn(VkDevice, *const c_void, *mut VkQueue)>;
pub type PFN_vkQueuePresentKHR =
    Option<unsafe extern "system" fn(VkQueue, *const VkPresentInfoKHR) -> VkResult>;
pub type PFN_vkCreateSwapchainKHR =
    Option<unsafe extern "system" fn(VkDevice, *const c_void, *const c_void, *mut u64) -> VkResult>;
pub type PFN_vkDestroySwapchainKHR =
    Option<unsafe extern "system" fn(VkDevice, u64, *const c_void)>;

// ---------------------------------------------------------------------------
// layout assertions
// ---------------------------------------------------------------------------

const _: () = {
    use std::mem::{align_of, size_of};

    // negotiation struct
    assert!(size_of::<VkNegotiateLayerInterface>() == 48);
    assert!(align_of::<VkNegotiateLayerInterface>() == 8);
    assert!(std::mem::offset_of!(VkNegotiateLayerInterface, loader_layer_interface_version) == 16);
    assert!(std::mem::offset_of!(VkNegotiateLayerInterface, pfn_get_instance_proc_addr) == 24);
    assert!(std::mem::offset_of!(VkNegotiateLayerInterface, pfn_get_device_proc_addr) == 32);
    assert!(std::mem::offset_of!(VkNegotiateLayerInterface, pfn_get_physical_device_proc_addr) == 40);

    // instance link + create info
    assert!(size_of::<VkLayerInstanceLink>() == 24);
    assert!(std::mem::offset_of!(VkLayerInstanceLink, pfn_next_get_instance_proc_addr) == 8);
    assert!(size_of::<VkLayerInstanceCreateInfo>() == 40);
    assert!(std::mem::offset_of!(VkLayerInstanceCreateInfo, s_type) == 0);
    assert!(std::mem::offset_of!(VkLayerInstanceCreateInfo, p_next) == 8);
    assert!(std::mem::offset_of!(VkLayerInstanceCreateInfo, function) == 16);
    assert!(std::mem::offset_of!(VkLayerInstanceCreateInfo, u) == 24);
    assert!(align_of::<VkLayerInstanceCreateInfoU>() == 8);

    // device link + create info
    assert!(size_of::<VkLayerDeviceLink>() == 24);
    assert!(size_of::<VkLayerDeviceCreateInfo>() == 32);
    assert!(std::mem::offset_of!(VkLayerDeviceCreateInfo, function) == 16);
    assert!(std::mem::offset_of!(VkLayerDeviceCreateInfo, u) == 24);

    // application-facing
    assert!(std::mem::offset_of!(VkInstanceCreateInfo, p_next) == 8);
    assert!(size_of::<VkInstanceCreateInfo>() == 64);
    assert!(std::mem::offset_of!(VkDeviceCreateInfo, p_next) == 8);
    assert!(size_of::<VkDeviceCreateInfo>() == 72);
    assert!(std::mem::offset_of!(VkPresentInfoKHR, swapchain_count) == 32);
    assert!(size_of::<VkPresentInfoKHR>() == 64);
};
