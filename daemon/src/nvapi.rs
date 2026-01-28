// Simple NVAPI wrapper for accessing undocumented thermal and voltage APIs
// Based on the nvapi.rs from the nvidia directory but simplified and standalone

#![allow(clippy::unreadable_literal)]

use anyhow::{bail, Context};
use std::{
    mem::{self, transmute},
    ptr,
};

const LIBRARY_NAME: &str = "libnvidia-api.so.1";
const QUERY_INTERFACE_FN: &[u8] = b"nvapi_QueryInterface\0";

const QUERY_NVAPI_INITIALIZE: u32 = 0x0150e828;
const QUERY_NVAPI_UNLOAD: u32 = 0xd22bdd7e;
const QUERY_NVAPI_ENUM_PHYSICAL_GPUS: u32 = 0xe5ac921f;
#[allow(dead_code)]
const QUERY_NVAPI_GET_BUS_ID: u32 = 0x1be0b8e5;
const QUERY_NVAPI_THERMALS: u32 = 0x65fe3aad; // Undocumented call
const QUERY_NVAPI_VOLTAGE: u32 = 0x465f9bcf; // Undocumented call

const NVAPI_MAX_PHYSICAL_GPUS: u32 = 64;

type NvApiStatus = i32;
type NvPhysicalGpuHandle = *mut std::ffi::c_void;

pub struct NvApi {
    lib: libloading::Library,
}

impl NvApi {
    pub fn new() -> anyhow::Result<Self> {
        let lib = unsafe {
            libloading::Library::new(LIBRARY_NAME).context("Could not load nvidia API library")
        }?;

        let handle = Self { lib };

        unsafe {
            let initialize = handle.query_interface(QUERY_NVAPI_INITIALIZE)?;
            let initialize: unsafe extern "C" fn() -> NvApiStatus = transmute(initialize);
            let status = initialize();
            handle.handle_status(status)?;
        }

        Ok(handle)
    }

    pub fn find_gpu_by_index(&self, index: u32) -> anyhow::Result<Option<NvPhysicalGpuHandle>> {
        unsafe {
            let handles = self.enum_physical_gpus()?;
            Ok(handles.get(index as usize).copied())
        }
    }

    pub unsafe fn get_thermals(
        &self,
        handle: NvPhysicalGpuHandle,
        mask: i32,
    ) -> anyhow::Result<NvApiThermals> {
        let f = self.query_interface(QUERY_NVAPI_THERMALS)?;
        let f: unsafe extern "C" fn(
            handle: NvPhysicalGpuHandle,
            sensors: &mut NvApiThermals,
        ) -> NvApiStatus = transmute(f);

        let mut sensors = NvApiThermals {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiThermals>() | (2 << 16)) as u32,
            mask,
            values: [0; 40],
        };

        let status = f(handle, &mut sensors);
        self.handle_status(status)?;

        Ok(sensors)
    }

    pub unsafe fn get_voltage(&self, handle: NvPhysicalGpuHandle) -> anyhow::Result<u32> {
        let f = self.query_interface(QUERY_NVAPI_VOLTAGE)?;
        let f: unsafe extern "C" fn(
            handle: NvPhysicalGpuHandle,
            data: &mut NvApiVoltage,
        ) -> NvApiStatus = transmute(f);

        let mut data = NvApiVoltage {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiVoltage>() | (1 << 16)) as u32,
            flags: 0,
            padding_1: [0; 8],
            value_uv: 0,
            padding_2: [0; 8],
        };
        let status = f(handle, &mut data);
        self.handle_status(status)?;

        Ok(data.value_uv)
    }

    pub unsafe fn calculate_thermals_mask(
        &self,
        handle: NvPhysicalGpuHandle,
    ) -> anyhow::Result<i32> {
        let f = self.query_interface(QUERY_NVAPI_THERMALS)?;
        let f: unsafe extern "C" fn(
            handle: NvPhysicalGpuHandle,
            sensors: &mut NvApiThermals,
        ) -> NvApiStatus = transmute(f);

        let mut sensors = NvApiThermals {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiThermals>() | (2 << 16)) as u32,
            mask: 1,
            values: [0; 40],
        };

        for bit in 0..32 {
            sensors.mask = 1 << bit;
            let status = f(handle, &mut sensors);
            if status != 0 {
                return Ok(sensors.mask - 1);
            }
        }

        bail!("Could not find suitable mask");
    }

    unsafe fn enum_physical_gpus(&self) -> anyhow::Result<Vec<NvPhysicalGpuHandle>> {
        let f = self.query_interface(QUERY_NVAPI_ENUM_PHYSICAL_GPUS)?;
        let f: unsafe extern "C" fn(
            handles: &mut [NvPhysicalGpuHandle; NVAPI_MAX_PHYSICAL_GPUS as usize],
            count: &mut u32,
        ) -> NvApiStatus = transmute(f);

        let mut count = 0;
        let mut handles =
            [(ptr::null_mut() as NvPhysicalGpuHandle); NVAPI_MAX_PHYSICAL_GPUS as usize];

        let status = f(&mut handles, &mut count);
        self.handle_status(status)?;

        Ok(handles.into_iter().take(count as usize).collect())
    }

    unsafe fn query_interface(&self, id: u32) -> anyhow::Result<*const ()> {
        let query_interface = self
            .lib
            .get::<unsafe extern "C" fn(u32) -> *const ()>(QUERY_INTERFACE_FN)
            .context("Could not get main symbol")?;

        let f = query_interface(id);

        if f.is_null() {
            bail!("Got null response for query id {id:x}");
        }

        Ok(f)
    }

    unsafe fn handle_status(&self, status: NvApiStatus) -> anyhow::Result<()> {
        if status == 0 {
            Ok(())
        } else {
            bail!("NVAPI error with status {status:x}");
        }
    }
}

impl Drop for NvApi {
    fn drop(&mut self) {
        unsafe {
            if let Ok(unload) = self.query_interface(QUERY_NVAPI_UNLOAD) {
                let unload: unsafe extern "C" fn() -> NvApiStatus = transmute(unload);
                unload();
            }
        }
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct NvApiThermals {
    version: u32,
    mask: i32,
    values: [i32; 40],
}

impl NvApiThermals {
    fn get_value(&self, index: usize) -> Option<f32> {
        self.values
            .get(index)
            .map(|&value| (value / 256) as f32)
            .filter(|&value| value > 0.0 && value < 255.0)
    }

    pub fn hotspot(&self) -> Option<f32> {
        self.get_value(9)
    }

    pub fn vram(&self) -> Option<f32> {
        self.get_value(15)
    }
}

#[repr(C)]
#[derive(Debug)]
struct NvApiVoltage {
    version: u32,
    flags: u32,
    padding_1: [u32; 8],
    value_uv: u32,
    padding_2: [u32; 8],
}
