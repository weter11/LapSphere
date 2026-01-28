
#![allow(clippy::unreadable_literal)]
#![allow(dead_code)]
#![allow(non_camel_case_types)]

use anyhow::{bail, Context};
use std::{
    ffi::{c_char, CStr},
    mem::{self, transmute},
    ptr,
};

const LIBRARY_NAME: &str = "libnvidia-api.so.1";
const QUERY_INTERFACE_FN: &[u8] = b"nvapi_QueryInterface\0";

const QUERY_NVAPI_INITIALIZE: u32 = 0x0150e828;
const QUERY_NVAPI_UNLOAD: u32 = 0xd22bdd7e;
const QUERY_NVAPI_ENUM_PHYSICAL_GPUS: u32 = 0xe5ac921f;
const QUERY_NVAPI_GET_BUS_ID: u32 = 0x1be0b8e5;
const QUERY_NVAPI_GET_ERROR_MESSAGE: u32 = 0x6c2d048c;
const QUERY_NVAPI_THERMALS: u32 = 0x65fe3aad; // Undocumented call
const QUERY_NVAPI_VOLTAGE: u32 = 0x465f9bcf; // Undocumented call

pub type NvAPI_Status = i32;
pub type NvPhysicalGpuHandle = *mut std::ffi::c_void;
pub const NVAPI_MAX_PHYSICAL_GPUS: usize = 64;
pub const NVAPI_SHORT_STRING_MAX: usize = 64;

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
            let initialize: unsafe extern "C" fn() -> NvAPI_Status = transmute(initialize);
            let status = initialize();
            handle.handle_status(status)?;
        }

        Ok(handle)
    }

    pub fn find_matching_gpu(&self, bus_id: u32) -> anyhow::Result<Option<NvPhysicalGpuHandle>> {
        unsafe {
            let handles = self.enum_physical_gpus()?;
            for handle in handles {
                let f = self.query_interface(QUERY_NVAPI_GET_BUS_ID)?;
                let f: unsafe extern "C" fn(
                    handle: NvPhysicalGpuHandle,
                    id: &mut u32,
                ) -> NvAPI_Status = transmute(f);

                let mut id = 0;
                let status = f(handle, &mut id);
                self.handle_status(status)?;

                if id == bus_id {
                    return Ok(Some(handle));
                }
            }

            Ok(None)
        }
    }

    pub unsafe fn get_thermals(
        &self,
        handle: NvPhysicalGpuHandle,
    ) -> anyhow::Result<NvApiThermals> {
        let f = self.query_interface(QUERY_NVAPI_THERMALS)?;
        let f: unsafe extern "C" fn(
            handle: NvPhysicalGpuHandle,
            sensors: &mut NvApiThermals,
        ) -> NvAPI_Status = transmute(f);

        let mut sensors = NvApiThermals {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiThermals>() | (2 << 16)) as u32,
            count: 0,
            sensors: [NvApiThermalSensor::default(); 8],
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
        ) -> NvAPI_Status = transmute(f);

        let mut data = NvApiVoltage {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiVoltage>() | (1 << 16)) as u32,
            flags: 0,
            value_uv: 0,
            padding: [0; 16],
        };
        let status = f(handle, &mut data);
        self.handle_status(status)?;

        Ok(data.value_uv)
    }

    unsafe fn enum_physical_gpus(&self) -> anyhow::Result<Vec<NvPhysicalGpuHandle>> {
        let f = self.query_interface(QUERY_NVAPI_ENUM_PHYSICAL_GPUS)?;
        let f: unsafe extern "C" fn(
            handles: &mut [NvPhysicalGpuHandle; NVAPI_MAX_PHYSICAL_GPUS],
            count: &mut u32,
        ) -> NvAPI_Status = transmute(f);

        let mut count = 0;
        let mut handles =
            [(ptr::null_mut() as NvPhysicalGpuHandle); NVAPI_MAX_PHYSICAL_GPUS];

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

    unsafe fn handle_status(&self, status: NvAPI_Status) -> anyhow::Result<()> {
        if status == 0 {
            Ok(())
        } else {
            let f = self.query_interface(QUERY_NVAPI_GET_ERROR_MESSAGE)?;
            let f: unsafe extern "C" fn(
                status: NvAPI_Status,
                text: *mut c_char,
            ) -> NvAPI_Status = transmute(f);

            let mut text = [0 as c_char; NVAPI_SHORT_STRING_MAX];
            let other_status = f(status, text.as_mut_ptr());
            if other_status != 0 {
                bail!(
                    "Got status {other_status:x} when fetching error message for status {status:x}"
                );
            }
            let text_cstr = CStr::from_ptr(text.as_ptr());
            bail!(
                "Got error {status:x} from NvAPI: {}",
                text_cstr.to_string_lossy()
            );
        }
    }
}

impl Drop for NvApi {
    fn drop(&mut self) {
        unsafe {
            if let Ok(unload) = self.query_interface(QUERY_NVAPI_UNLOAD) {
                let unload: unsafe extern "C" fn() -> NvAPI_Status = transmute(unload);
                unload();
            }
        }
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct NvApiThermalSensor {
    pub controller: i32,
    pub default_min: i32,
    pub default_max: i32,
    pub current_temp: i32,
    pub target: i32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct NvApiThermals {
    pub version: u32,
    pub count: u32,
    pub sensors: [NvApiThermalSensor; 8],
}

impl NvApiThermals {
    pub fn get_temp(&self, target_id: i32) -> Option<f32> {
        for i in 0..(self.count as usize).min(8) {
            if self.sensors[i].target == target_id {
                let t = self.sensors[i].current_temp;
                // Standard NVAPI returns degrees Celsius as NvS32.
                // We check for reasonable values.
                if t > 0 && t < 150 {
                    return Some(t as f32);
                }
                // Some undocumented implementations might return 8.8 fixed point.
                if t > 1000 && t < 40000 {
                    let tf = t as f32 / 256.0;
                    if tf > 0.0 && tf < 150.0 {
                        return Some(tf);
                    }
                }
            }
        }
        None
    }

    pub fn core(&self) -> Option<f32> {
        self.get_temp(1) // NVAPI_THERMAL_TARGET_GPU
    }

    pub fn vram(&self) -> Option<f32> {
        self.get_temp(2) // NVAPI_THERMAL_TARGET_MEMORY
    }

    pub fn hotspot(&self) -> Option<f32> {
        self.get_temp(9) // NVAPI_THERMAL_TARGET_HOTSPOT
    }
}

#[repr(C)]
#[derive(Debug)]
struct NvApiVoltage {
    version: u32,
    flags: u32,
    value_uv: u32,
    padding: [u32; 16], // Generous padding to match suspected structure size
}
