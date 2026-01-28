use std::{
    ffi::c_void,
    fs::File,
    mem,
    os::fd::{AsRawFd, RawFd},
    ptr,
};
use anyhow::{bail, Context, Result};
use nix::ioctl_readwrite;

pub const NV_IOCTL_MAGIC: u8 = b'F';
pub const NV_ESC_RM_ALLOC: u8 = 0x2b;
pub const NV_ESC_RM_CONTROL: u8 = 0x2a;
pub const NV_ESC_REGISTER_FD: u8 = 0x27;

pub const NV01_DEVICE_0: u32 = 0x00000080;
pub const NV20_SUBDEVICE_0: u32 = 0x00002080;

pub const NV2080_CTRL_CMD_THERMAL_GET_TEMPERATURES: u32 = 0x20800501;
pub const NV2080_CTRL_CMD_THERMAL_GET_THERMAL_SENSORS_INFO: u32 = 0x20800502;
pub const NV2080_CTRL_CMD_THERMAL_GET_ALL_THERMAL_SENSORS_INFO: u32 = 0x65fe3aad;
pub const NV2080_THERMAL_SENSORS_MAX_COUNT: usize = 32;

pub const NV2080_CTRL_CMD_VOLT_GET_VOLTAGE: u32 = 0x20803201;
pub const NV2080_CTRL_CMD_VOLT_GET_VOLTAGE_EX: u32 = 0x465f9bcf;
pub const NV2080_CTRL_VOLT_DOMAIN_CORE: u32 = 0x00000000;

pub const NV2080_CTRL_THERMAL_SENSOR_TYPE_MEMORY: u32 = 0x00000002;

type NvHandle = u32;

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NVOS21_PARAMETERS {
    hRoot: NvHandle,
    hObjectParent: NvHandle,
    hObjectNew: NvHandle,
    hClass: u32,
    pAllocParms: *mut c_void,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NVOS64_PARAMETERS {
    hRoot: NvHandle,
    hObjectParent: NvHandle,
    hObjectNew: NvHandle,
    hClass: u32,
    pAllocParms: *mut c_void,
    pRightsRequested: *mut c_void,
    paramsSize: u32,
    flags: u32,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV0080_ALLOC_PARAMETERS {
    deviceId: u32,
    hClientShare: NvHandle,
    hTargetClient: NvHandle,
    hTargetDevice: NvHandle,
    flags: u32,
    pad0: [u32; 2],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV2080_ALLOC_PARAMETERS {
    reserved: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NVOS54_PARAMETERS {
    hClient: NvHandle,
    hObject: NvHandle,
    cmd: u32,
    flags: u32,
    params: *mut c_void,
    paramsSize: u32,
    status: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV2080_CTRL_THERMAL_GET_TEMPERATURES_PARAMS {
    mask: u32,
    temperatures: [i32; NV2080_THERMAL_SENSORS_MAX_COUNT],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
struct NV2080_CTRL_THERMAL_GET_THERMAL_SENSORS_INFO_PARAMS {
    sensorCount: u32,
    sensorInfo: [NV2080_CTRL_THERMAL_SENSOR_INFO; NV2080_THERMAL_SENSORS_MAX_COUNT],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
struct NV2080_CTRL_THERMAL_SENSOR_INFO {
    sensorId: u32,
    sensorType: u32,
    controllerId: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV2080_CTRL_THERMAL_GET_ALL_THERMAL_SENSORS_INFO_PARAMS {
    sensorCount: u32,
    sensorInfo: [NV2080_CTRL_THERMAL_ALL_SENSOR_INFO; NV2080_THERMAL_SENSORS_MAX_COUNT],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
struct NV2080_CTRL_THERMAL_ALL_SENSOR_INFO {
    sensorId: u32,
    sensorType: u32,
    controllerId: u32,
    currentTemp: i32,
    warnTemp: i32,
    critTemp: i32,
    unknown: u32,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV2080_CTRL_VOLT_GET_VOLTAGE_PARAMS {
    voltDomainId: u32,
    voltageuV: u32,
}

ioctl_readwrite!(rm_alloc_nvos21, NV_IOCTL_MAGIC, NV_ESC_RM_ALLOC, NVOS21_PARAMETERS);
ioctl_readwrite!(rm_alloc_nvos64, NV_IOCTL_MAGIC, NV_ESC_RM_ALLOC, NVOS64_PARAMETERS);
ioctl_readwrite!(register_fd, NV_IOCTL_MAGIC, NV_ESC_REGISTER_FD, RawFd);
ioctl_readwrite!(rm_control_nvos54, NV_IOCTL_MAGIC, NV_ESC_RM_CONTROL, NVOS54_PARAMETERS);

pub struct NvidiaDriverHandle {
    nvidiactl_fd: File,
    #[allow(dead_code)]
    device_fd: File,
    client_handle: NvHandle,
    #[allow(dead_code)]
    device_handle: NvHandle,
    subdevice_handle: NvHandle,
}

impl NvidiaDriverHandle {
    pub fn open(minor_number: u32) -> Result<Self> {
        let nvidiactl_fd = File::options()
            .read(true)
            .write(true)
            .open("/dev/nvidiactl")
            .context("Could not open nvidiactl")?;

        let client_handle: NvHandle = unsafe {
            let mut client_request: NVOS21_PARAMETERS = mem::zeroed();
            rm_alloc_nvos21(nvidiactl_fd.as_raw_fd(), &raw mut client_request)?;
            client_request.hObjectNew
        };

        let device_fd = File::options()
            .read(true)
            .write(true)
            .open(format!("/dev/nvidia{minor_number}"))
            .context("Could not open nvidia device")?;

        let device_handle: NvHandle = unsafe {
            let mut fd = device_fd.as_raw_fd();
            register_fd(nvidiactl_fd.as_raw_fd(), &mut fd)?;

            let mut alloc_params: NV0080_ALLOC_PARAMETERS = mem::zeroed();
            alloc_params.deviceId = minor_number;
            let mut request = NVOS64_PARAMETERS {
                hRoot: client_handle,
                hObjectParent: client_handle,
                hObjectNew: 0,
                hClass: NV01_DEVICE_0,
                pAllocParms: ptr::from_mut(&mut alloc_params).cast::<c_void>(),
                pRightsRequested: ptr::null_mut(),
                paramsSize: mem::size_of::<NV0080_ALLOC_PARAMETERS>() as u32,
                flags: 0,
                status: 0,
            };

            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &raw mut request)?;

            if request.status != 0 {
                bail!("Got error status 0x{:x} on Nvidia device handle creation", request.status);
            }

            request.hObjectNew
        };

        let subdevice_handle: NvHandle = unsafe {
            let mut alloc_params: NV2080_ALLOC_PARAMETERS = mem::zeroed();

            let mut request = NVOS64_PARAMETERS {
                hRoot: client_handle,
                hObjectParent: device_handle,
                hObjectNew: 0,
                hClass: NV20_SUBDEVICE_0,
                pAllocParms: ptr::from_mut(&mut alloc_params).cast(),
                pRightsRequested: ptr::null_mut(),
                paramsSize: mem::size_of::<NV2080_ALLOC_PARAMETERS>() as u32,
                flags: 0,
                status: 0,
            };

            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &raw mut request)?;

            if request.status != 0 {
                bail!("Got error status 0x{:x} on Nvidia subdevice handle creation", request.status);
            }

            request.hObjectNew
        };

        Ok(Self {
            nvidiactl_fd,
            device_fd,
            client_handle,
            device_handle,
            subdevice_handle,
        })
    }

    fn query_rm_control<T: Copy>(&self, cmd: u32, params: &mut T) -> Result<()> {
        let mut request = NVOS54_PARAMETERS {
            hClient: self.client_handle,
            hObject: self.subdevice_handle,
            cmd,
            flags: 0,
            params: ptr::from_mut(params).cast(),
            paramsSize: mem::size_of::<T>().try_into().unwrap(),
            status: 0,
        };
        unsafe {
            rm_control_nvos54(self.nvidiactl_fd.as_raw_fd(), &raw mut request)?;
        }

        if request.status != 0 {
            bail!("Nvidia request failed with status 0x{:x}", request.status);
        }

        Ok(())
    }

    pub fn get_thermal_sensors_info(&self) -> Result<Vec<(u32, u32)>> {
        let mut params = NV2080_CTRL_THERMAL_GET_THERMAL_SENSORS_INFO_PARAMS::default();
        self.query_rm_control(NV2080_CTRL_CMD_THERMAL_GET_THERMAL_SENSORS_INFO, &mut params)?;

        let sensors = params.sensorInfo[..params.sensorCount as usize]
            .iter()
            .map(|info| (info.sensorId, info.sensorType))
            .collect();

        Ok(sensors)
    }

    pub fn get_temperatures(&self, mask: u32) -> Result<Vec<(u32, i32)>> {
        let mut params = NV2080_CTRL_THERMAL_GET_TEMPERATURES_PARAMS {
            mask,
            temperatures: [0; NV2080_THERMAL_SENSORS_MAX_COUNT],
        };
        self.query_rm_control(NV2080_CTRL_CMD_THERMAL_GET_TEMPERATURES, &mut params)?;

        let mut results = Vec::new();
        for i in 0..NV2080_THERMAL_SENSORS_MAX_COUNT {
            if (mask & (1 << i)) != 0 {
                results.push((i as u32, params.temperatures[i]));
            }
        }

        Ok(results)
    }

    pub fn get_voltage(&self, domain_id: u32) -> Result<u32> {
        let mut params = NV2080_CTRL_VOLT_GET_VOLTAGE_PARAMS {
            voltDomainId: domain_id,
            voltageuV: 0,
        };
        // Try standard first, then EX
        if let Err(_) = self.query_rm_control(NV2080_CTRL_CMD_VOLT_GET_VOLTAGE, &mut params) {
             self.query_rm_control(NV2080_CTRL_CMD_VOLT_GET_VOLTAGE_EX, &mut params)?;
        }
        Ok(params.voltageuV)
    }

    pub fn get_all_thermals(&self) -> Result<Vec<(u32, u32, f32)>> {
        let mut params: NV2080_CTRL_THERMAL_GET_ALL_THERMAL_SENSORS_INFO_PARAMS = unsafe { mem::zeroed() };
        self.query_rm_control(NV2080_CTRL_CMD_THERMAL_GET_ALL_THERMAL_SENSORS_INFO, &mut params)?;

        let mut results = Vec::new();
        for i in 0..params.sensorCount as usize {
            if i < NV2080_THERMAL_SENSORS_MAX_COUNT {
                let entry = &params.sensorInfo[i];
                results.push((entry.sensorId, entry.sensorType, entry.currentTemp as f32 / 256.0));
            }
        }
        Ok(results)
    }
}
