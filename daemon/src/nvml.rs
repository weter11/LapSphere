use anyhow::{anyhow, Result};
use libloading::{Library, Symbol};
use nvml_wrapper::sys::{nvmlDevice_t, nvmlReturn_t, NVML_SUCCESS};
use std::mem;

use crate::structs::nvmlClockOffset;

const NVML_LIB_PATH: &str = "/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1";

pub struct NvmlLib {
    _lib: Library,
    nvml_device_get_clock: Symbol<'static, unsafe extern "C" fn(nvmlDevice_t, u32, u32, *mut u32) -> nvmlReturn_t>,
    nvml_device_get_clock_offsets: Symbol<'static, unsafe extern "C" fn(nvmlDevice_t, *mut nvmlClockOffset) -> nvmlReturn_t>,
    nvml_device_set_clock_offsets: Symbol<'static, unsafe extern "C" fn(nvmlDevice_t, *const nvmlClockOffset) -> nvmlReturn_t>,
}

impl NvmlLib {
    pub fn new() -> Result<Self> {
        unsafe {
            let lib = Library::new(NVML_LIB_PATH)?;
            let nvml_device_get_clock = *lib.get(b"nvmlDeviceGetClock")?;
            let nvml_device_get_clock_offsets = *lib.get(b"nvmlDeviceGetClockOffsets")?;
            let nvml_device_set_clock_offsets = *lib.get(b"nvmlDeviceSetClockOffsets")?;

            Ok(Self {
                _lib: lib,
                nvml_device_get_clock,
                nvml_device_get_clock_offsets,
                nvml_device_set_clock_offsets,
            })
        }
    }

    pub unsafe fn get_clock_offsets(&self, device: nvmlDevice_t) -> Result<nvmlClockOffset> {
        let mut offset = mem::zeroed();
        let ret = (self.nvml_device_get_clock_offsets)(device, &mut offset);
        if ret != NVML_SUCCESS {
            return Err(anyhow!("nvmlDeviceGetClockOffsets failed: {}", ret));
        }
        Ok(offset)
    }

    pub unsafe fn set_clock_offsets(&self, device: nvmlDevice_t, offset: &nvmlClockOffset) -> Result<()> {
        let ret = (self.nvml_device_set_clock_offsets)(device, offset);
        if ret != NVML_SUCCESS {
            return Err(anyhow!("nvmlDeviceSetClockOffsets failed: {}", ret));
        }
        Ok(())
    }
}