pub mod nvidia {
    #[allow(non_camel_case_types)]
    pub type NvAPI_Status = i32;
    pub type NvPhysicalGpuHandle = *mut std::ffi::c_void;
    pub const NVAPI_MAX_PHYSICAL_GPUS: u32 = 64;
    pub const NVAPI_SHORT_STRING_MAX: u32 = 64;
}
