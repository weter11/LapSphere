//! Minimal raw Vulkan declarations for the layer.
//!
//! Deliberately hand-written instead of pulled from `ash`: a layer needs
//! exactly eleven entry points, the *loader* structs
//! (`VkLayerInstanceCreateInfo`, `VkNegotiateLayerInterface` — which `ash`
//! does not bind at all), and the first two fields of two `p_next` chains.
//! Binding a whole 500k-line header crate for that, and keeping it in
//! step with its own API churn, is more risk than the crate removes.
//!
//! Everything here is `#[repr(C)]` and matches the ABI in
//! `/usr/include/vulkan/vulkan_core.h` and `vk_layer.h` on this host. The
//! layouts are checked at compile time where the compiler can check them
//! (offsets of `s_type` and `p_next`), and the one thing a static assertion
//! cannot check — that the loader agrees — is exercised by the spike's
//! test 1: if these declarations were wrong, `vkCreateInstance` would not
//! find the chain and the segment would never appear.

#![allow(non_camel_case_types)]

use std::ffi::{c_char, c_void};

/// `VK_DEFINE_HANDLE` — a dispatchable object is a pointer.
macro_rules! handle {
    ($name:ident) => {
        #[repr(transparent)]
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub struct $name(pub *mut c_void);

        impl $name {
            #[inline]
            pub const fn null() -> Self {
                $name(std::ptr::null_mut())
            }
            #[inline]
            pub fn as_raw(&self) -> *mut c_void {
                self.0
            }
            #[inline]
            pub fn from_raw(p: *mut c_void) -> Self {
                $name(p)
            }
        }
    };
}

handle!(Instance);
handle!(PhysicalDevice);
handle!(Device);
handle!(Queue);
handle!(SwapchainKHR);

/// `VkResult`. A newtype over `i32` rather than an `enum`, because the layer
/// must forward a value it did not produce and a C enum with an
/// unspecified-invalid-value range is a trap.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VkResult(pub i32);

pub const SUCCESS: VkResult = VkResult(0);
pub const ERROR_OUT_OF_HOST_MEMORY: VkResult = VkResult(-1);
pub const ERROR_INITIALIZATION_FAILED: VkResult = VkResult(-3);
pub const ERROR_EXTENSION_NOT_PRESENT: VkResult = VkResult(-7);

impl VkResult {
    #[inline]
    pub fn is_error(self) -> bool {
        self.0 < 0
    }
}

/// `VkStructureType`; only the two loader values are needed.
pub const STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO: i32 = 47;
pub const STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO: i32 = 48;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BaseInStructure {
    pub s_type: i32,
    pub p_next: *const BaseInStructure,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkAllocationCallbacks {
    pub p_user_data: *mut c_void,
    pub pfn_allocation: *mut c_void,
    pub pfn_reallocation: *mut c_void,
    pub pfn_free: *mut c_void,
    pub p_internal_allocation: *mut c_void,
}

/// `VkLayerInstanceLink` / `VkLayerDeviceLink` from `vk_layer.h`.
///
/// The chain link's *first* member is the next layer's
/// `vkGetInstanceProcAddr`, which is how a layer finds the layer below it.
/// There is no `VkLayerFunction` struct in the create-info chain -- that was
/// the mistake in the first version of this file; the chain carries an
/// **enum** plus a union, and reading it as a struct of two function pointers
/// yields address `0x3`, which is exactly the crash the spike produced before
/// this was corrected. Recorded because the failure mode (segfault at the
/// first call, at address 3) looks nothing like a struct-layout mistake.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkLayerInstanceLink {
    pub p_next: *mut VkLayerInstanceLink,
    pub pfn_next_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_physical_device_proc_addr: Option<PFN_vkGetPhysicalDeviceProcAddr>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkLayerDeviceLink {
    pub p_next: *mut VkLayerDeviceLink,
    pub pfn_next_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_next_get_device_proc_addr: PFN_vkGetDeviceProcAddr,
}

/// `VkLayerFunction_`: which union member is live. 0 = link info, which is the
/// only value a layer needs to care about; the loader may pass others through
/// and they must be forwarded untouched (they are, because this struct is only
/// ever read).
pub const VK_LAYER_LINK_INFO: i32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkLayerInstanceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub function: i32,
    pub p_layer_info: *mut VkLayerInstanceLink,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkLayerDeviceCreateInfo {
    pub s_type: i32,
    pub p_next: *const c_void,
    pub function: i32,
    pub p_layer_info: *mut VkLayerDeviceLink,
}

/// The instance/device create-info headers. Only `s_type`/`p_next` are read,
/// and they are the first two fields, so a shorter stand-in would be
/// ABI-identical — but declaring the real prefix keeps the static offset
/// assertions meaningful.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkInstanceCreateInfo {
    pub s_type: i32,
    pub p_next: *const BaseInStructure,
    pub flags: u32,
    pub p_application_info: *const c_void,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkDeviceCreateInfo {
    pub s_type: i32,
    pub p_next: *const BaseInStructure,
    pub flags: u32,
    pub queue_create_info_count: u32,
    pub p_queue_create_infos: *const c_void,
    pub enabled_layer_count: u32,
    pub pp_enabled_layer_names: *const *const c_char,
    pub enabled_extension_count: u32,
    pub pp_enabled_extension_names: *const *const c_char,
    pub p_enabled_features: *const c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkPresentInfoKHR {
    pub s_type: i32,
    pub p_next: *const BaseInStructure,
    pub swapchain_count: u32,
    pub p_swapchains: *const SwapchainKHR,
    pub p_image_indices: *const u32,
    pub p_results: *mut i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct VkSwapchainCreateInfoKHR {
    pub s_type: i32,
    pub p_next: *const BaseInStructure,
    pub flags: u32,
    pub surface: u64,
    pub min_image_count: u32,
    pub image_format: u32,
    pub image_color_space: u32,
    pub image_extent_width: u32,
    pub image_extent_height: u32,
    pub image_array_layers: u32,
    pub image_usage: u32,
    pub image_sharing_mode: u32,
    pub queue_family_index_count: u32,
    pub p_queue_family_indices: *const u32,
    pub old_swapchain: SwapchainKHR,
    pub p_clipped_rects: *const c_void,
    pub clipped_rect_count: u32,
    pub p_transform: u32,
    pub composite_alpha: u32,
    pub present_mode: u32,
    pub clipped: *mut i32,
    pub old_swapchain_value: u64,
}

pub type PFN_vkVoidFunction = Option<unsafe extern "system" fn()>;

pub type PFN_vkGetInstanceProcAddr =
    unsafe extern "system" fn(Instance, *const c_char) -> PFN_vkVoidFunction;
pub type PFN_vkGetDeviceProcAddr =
    unsafe extern "system" fn(Device, *const c_char) -> PFN_vkVoidFunction;
pub type PFN_vkGetPhysicalDeviceProcAddr =
    unsafe extern "system" fn(Instance, *const c_char) -> PFN_vkVoidFunction;

pub type PFN_vkCreateInstance = unsafe extern "system" fn(
    *const VkInstanceCreateInfo,
    *const VkAllocationCallbacks,
    *mut Instance,
) -> VkResult;
pub type PFN_vkDestroyInstance = unsafe extern "system" fn(Instance, *const VkAllocationCallbacks);
pub type PFN_vkCreateDevice = unsafe extern "system" fn(
    PhysicalDevice,
    *const VkDeviceCreateInfo,
    *const VkAllocationCallbacks,
    *mut Device,
) -> VkResult;
pub type PFN_vkDestroyDevice = unsafe extern "system" fn(Device, *const VkAllocationCallbacks);
pub type PFN_vkQueuePresentKHR =
    unsafe extern "system" fn(Queue, *const VkPresentInfoKHR) -> VkResult;
pub type PFN_vkCreateSwapchainKHR = unsafe extern "system" fn(
    Device,
    *const VkSwapchainCreateInfoKHR,
    *const VkAllocationCallbacks,
    *mut SwapchainKHR,
) -> VkResult;
pub type PFN_vkDestroySwapchainKHR =
    unsafe extern "system" fn(Device, SwapchainKHR, *const VkAllocationCallbacks);

/// The negotiation struct from `vk_layer.h`.
#[repr(C)]
pub struct VkNegotiateLayerInterface {
    pub s_type: i32,
    pub p_next: *mut c_void,
    pub loader_layer_interface_version: u32,
    pub pfn_get_instance_proc_addr: PFN_vkGetInstanceProcAddr,
    pub pfn_get_device_proc_addr: PFN_vkGetDeviceProcAddr,
    pub pfn_get_physical_device_proc_addr: Option<PFN_vkGetPhysicalDeviceProcAddr>,
}

// Static layout checks. `s_type` at 0 and a pointer at 8 are the only
// offsets the layer depends on; if bindgen ever reorders these, this fails
// at compile time rather than at run time inside a game.
const _: () = {
    use core::mem::{align_of, offset_of};
    let c = offset_of!(VkInstanceCreateInfo, p_next);
    let d = offset_of!(VkDeviceCreateInfo, p_next);
    let l = offset_of!(VkLayerInstanceCreateInfo, p_layer_info);
    // s_type(4) pad(4) p_next(8) => p_next at 8 in both create infos.
    // Loader create infos: s_type(4) pad(4) p_next(8) function(4) pad(4) =>
    // the link pointer at 24 (the `function` enum sits between p_next and
    // the union, which is exactly what the first version got wrong).
    // Negotiation struct: s_type(4) pad(4) p_next(8) version(4) pad(4) =>
    // first function pointer at 24. All asserted so a mistake cannot reach a
    // game process.
    let m = offset_of!(VkNegotiateLayerInterface, pfn_get_instance_proc_addr);
    assert!(
        c == 8 && d == 8 && l == 24 && m == 24,
        "unexpected Vulkan layout"
    );
    assert!(
        align_of::<VkAllocationCallbacks>() == align_of::<*mut c_void>(),
        "allocator callback alignment"
    );
};

