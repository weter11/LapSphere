
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
        log::debug!("Loading NVIDIA API library: {}", LIBRARY_NAME);
        let lib = unsafe {
            libloading::Library::new(LIBRARY_NAME).context("Could not load nvidia API library")
        }?;

        let handle = Self { lib };

        unsafe {
            let initialize = handle.query_interface(QUERY_NVAPI_INITIALIZE)?;
            let initialize: unsafe extern "C" fn() -> NvAPI_Status = transmute(initialize);
            let status = initialize();
            handle.handle_status(status).context("NvAPI_Initialize failed")?;
        }

        log::debug!("NVIDIA API initialized successfully");
        Ok(handle)
    }

    pub fn find_matching_gpu(&self, bus_id: u32) -> anyhow::Result<Option<NvPhysicalGpuHandle>> {
        unsafe {
            let handles = self.enum_physical_gpus()?;
            log::debug!("Found {} physical GPUs via NVAPI", handles.len());
            for handle in handles {
                let f = self.query_interface(QUERY_NVAPI_GET_BUS_ID)?;
                let f: unsafe extern "C" fn(
                    handle: NvPhysicalGpuHandle,
                    id: &mut u32,
                ) -> NvAPI_Status = transmute(f);

                let mut id = 0;
                let status = f(handle, &mut id);
                self.handle_status(status)?;

                log::debug!("GPU handle {:?} has bus ID {}", handle, id);
                if id == bus_id {
                    return Ok(Some(handle));
                }
            }

            log::warn!("No GPU matching bus ID {} found in NVAPI", bus_id);
            Ok(None)
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
        ) -> NvAPI_Status = transmute(f);

        let mut sensors = NvApiThermals {
            #[allow(clippy::cast_possible_truncation)]
            version: (mem::size_of::<NvApiThermals>() | (2 << 16)) as u32,
            mask,
            values: [0; 40],
        };

        let status = f(handle, &mut sensors);
        self.handle_status(status).context("QUERY_NVAPI_THERMALS failed")?;

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
            padding_1: [0; 8],
            value_uv: 0,
            padding_2: [0; 8],
        };
        let status = f(handle, &mut data);
        self.handle_status(status).context("QUERY_NVAPI_VOLTAGE failed")?;

        Ok(data.value_uv)
    }

    unsafe fn enum_physical_gpus(&self) -> anyhow::Result<Vec<NvPhysicalGpuHandle>> {
        let f = self.query_interface(QUERY_NVAPI_ENUM_PHYSICAL_GPUS)?;
        let f: unsafe extern "C" fn(
            handles: *mut NvPhysicalGpuHandle,
            count: &mut u32,
        ) -> NvAPI_Status = transmute(f);

        let mut count = 0;
        let mut handles =
            [ptr::null_mut(); NVAPI_MAX_PHYSICAL_GPUS];

        let status = f(handles.as_mut_ptr(), &mut count);
        self.handle_status(status)?;

        Ok(handles.into_iter().take(count as usize).collect())
    }

    unsafe fn query_interface(&self, id: u32) -> anyhow::Result<*const ()> {
        let query_interface = self
            .lib
            .get::<unsafe extern "C" fn(u32) -> *const ()>(QUERY_INTERFACE_FN)
            .context("Could not get nvapi_QueryInterface symbol")?;

        let f = query_interface(id);

        if f.is_null() {
            bail!("Got null response from nvapi_QueryInterface for query id {id:x}");
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
            .map(|&value| value as f32 / 256.0)
            .filter(|&value| value > 0.0 && value < 255.0)
    }

    pub fn core(&self) -> Option<f32> {
        self.get_value(0)
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
