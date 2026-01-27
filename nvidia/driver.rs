
use std::{
    ffi::c_void,
    fs::File,
    mem,
    os::fd::{AsRawFd, RawFd},
    ptr,
};

use crate::bindings::nvidia::{
    NvHandle, NV0080_ALLOC_PARAMETERS, NV01_DEVICE_0, NV2080_ALLOC_PARAMETERS,
    NV2080_CTRL_CMD_FB_GET_INFO, NV2080_CTRL_CMD_GR_GET_GLOBAL_SM_ORDER,
    NV2080_CTRL_CMD_GR_GET_ROP_INFO, NV2080_CTRL_FB_GET_INFO_PARAMS, NV2080_CTRL_FB_INFO,
    NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH, NV2080_CTRL_FB_INFO_INDEX_L2CACHE_SIZE,
    NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID, NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ELPIDA, NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ESMT,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ETRON, NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_HYNIX,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_MICRON,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_MOSEL, NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_NANYA,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_QIMONDA,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_SAMSUNG,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_UNKNOWN,
    NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_WINBOND, NV2080_CTRL_FB_INFO_RAM_TYPE_DDR1,
    NV2080_CTRL_FB_INFO_RAM_TYPE_DDR2, NV2080_CTRL_FB_INFO_RAM_TYPE_DDR3,
    NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR2, NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR3,
    NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR4, NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR5,
    NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR5X, NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR6,
    NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR6X, NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR7,
    NV2080_CTRL_FB_INFO_RAM_TYPE_HBM1, NV2080_CTRL_FB_INFO_RAM_TYPE_HBM2,
    NV2080_CTRL_FB_INFO_RAM_TYPE_HBM3, NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR2,
    NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR4, NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR5,
    NV2080_CTRL_FB_INFO_RAM_TYPE_SDDR4, NV2080_CTRL_FB_INFO_RAM_TYPE_SDRAM,
    NV2080_CTRL_FB_INFO_RAM_TYPE_UNKNOWN, NV2080_CTRL_GR_GET_GLOBAL_SM_ORDER_PARAMS,
    NV2080_CTRL_GR_GET_ROP_INFO_PARAMS, NV20_SUBDEVICE_0, NVOS21_PARAMETERS, NVOS54_PARAMETERS,
    NVOS64_PARAMETERS, NV_ESC_REGISTER_FD, NV_ESC_RM_ALLOC, NV_ESC_RM_CONTROL, NV_IOCTL_MAGIC,
};
use anyhow::{bail, Context};
use lact_schema::RopInfo;
use nix::ioctl_readwrite;

const NV2080_CTRL_CMD_THERMAL_GET_TEMPERATURES: u32 = 0x20800501;
const NV2080_CTRL_CMD_THERMAL_GET_THERMAL_SENSORS_INFO: u32 = 0x20800502;
const NV2080_CTRL_THERMAL_SENSORS_MAX_COUNT: usize = 32;

const NV2080_CTRL_CMD_VOLT_GET_VOLTAGE: u32 = 0x20803201;
pub const NV2080_CTRL_VOLT_DOMAIN_CORE: u32 = 0x00000000;

pub const NV2080_CTRL_THERMAL_SENSOR_TYPE_GPU: u32 = 0x00000001;
pub const NV2080_CTRL_THERMAL_SENSOR_TYPE_MEMORY: u32 = 0x00000002;

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct NV2080_CTRL_THERMAL_GET_TEMPERATURES_PARAMS {
    mask: u32,
    temperatures: [i32; NV2080_CTRL_THERMAL_SENSORS_MAX_COUNT],
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
struct NV2080_CTRL_THERMAL_GET_THERMAL_SENSORS_INFO_PARAMS {
    sensorCount: u32,
    sensorInfo: [NV2080_CTRL_THERMAL_SENSOR_INFO; NV2080_CTRL_THERMAL_SENSORS_MAX_COUNT],
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
struct NV2080_CTRL_VOLT_GET_VOLTAGE_PARAMS {
    voltDomainId: u32,
    voltageuV: u32,
}

pub struct DriverHandle {
    nvidiactl_fd: File,
    #[allow(dead_code)]
    device_fd: File,

    client_handle: NvHandle,
    #[allow(dead_code)]
    device_handle: NvHandle,
    subdevice_handle: NvHandle,
}

impl DriverHandle {
    pub fn open(minor_number: u32) -> anyhow::Result<Self> {
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
            register_fd(device_fd.as_raw_fd(), &mut nvidiactl_fd.as_raw_fd())?;

            let mut alloc_params: NV0080_ALLOC_PARAMETERS = mem::zeroed();
            alloc_params.deviceId = minor_number;
            let mut request = NVOS64_PARAMETERS {
                hRoot: client_handle,
                hObjectParent: client_handle,
                hObjectNew: 0,
                hClass: NV01_DEVICE_0,
                pAllocParms: ptr::from_mut(&mut alloc_params).cast::<c_void>(),
                pRightsRequested: ptr::null_mut(),
                paramsSize: 0,
                flags: 0,
                status: 0,
            };

            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &raw mut request)?;

            if request.status != 0 {
                bail!(
                    "Got error status {} on Nvidia device handle creation",
                    request.status
                );
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
                paramsSize: 0,
                flags: 0,
                status: 0,
            };

            rm_alloc_nvos64(nvidiactl_fd.as_raw_fd(), &raw mut request)?;

            if request.status != 0 {
                bail!(
                    "Got error status {} on Nvidia subdevice handle creation",
                    request.status
                );
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

    pub fn get_rop_info(&self) -> anyhow::Result<RopInfo> {
        unsafe {
            let mut params: NV2080_CTRL_GR_GET_ROP_INFO_PARAMS = mem::zeroed();
            self.query_rm_control(NV2080_CTRL_CMD_GR_GET_ROP_INFO, &mut params)?;

            Ok(RopInfo {
                unit_count: params.ropUnitCount,
                operations_factor: params.ropOperationsFactor,
                operations_count: params.ropOperationsCount,
            })
        }
    }

    pub fn get_sm_count(&self) -> anyhow::Result<u32> {
        unsafe {
            let mut params: NV2080_CTRL_GR_GET_GLOBAL_SM_ORDER_PARAMS = mem::zeroed();
            self.query_rm_control(NV2080_CTRL_CMD_GR_GET_GLOBAL_SM_ORDER, &mut params)?;
            Ok(u32::from(params.numSm))
        }
    }

    pub fn get_ram_type(&self) -> anyhow::Result<&'static str> {
        let value = self.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE)?;
        let name = match value {
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR2 => "GDDR2",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR3 => "GDDR3",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR4 => "GDDR4",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR5 => "GDDR5",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR5X => "GDDR5X",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR6 => "GDDR6",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR6X => "GDDR6x",
            NV2080_CTRL_FB_INFO_RAM_TYPE_GDDR7 => "GDDR7",

            NV2080_CTRL_FB_INFO_RAM_TYPE_HBM1 => "HBM1",
            NV2080_CTRL_FB_INFO_RAM_TYPE_HBM2 => "HBM2",
            NV2080_CTRL_FB_INFO_RAM_TYPE_HBM3 => "HBM3",

            NV2080_CTRL_FB_INFO_RAM_TYPE_DDR1 => "DDR1",
            NV2080_CTRL_FB_INFO_RAM_TYPE_DDR2 => "DDR2",
            NV2080_CTRL_FB_INFO_RAM_TYPE_DDR3 => "DDR3",

            NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR2 => "LPDDR2",
            NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR4 => "LPDDR4",
            NV2080_CTRL_FB_INFO_RAM_TYPE_LPDDR5 => "LPDDR5",

            NV2080_CTRL_FB_INFO_RAM_TYPE_SDDR4 => "SDDR4",
            NV2080_CTRL_FB_INFO_RAM_TYPE_SDRAM => "SDRAM",

            NV2080_CTRL_FB_INFO_RAM_TYPE_UNKNOWN => "Unknown",
            _ => "Unrecognized",
        };
        Ok(name)
    }

    pub fn get_bus_width(&self) -> anyhow::Result<u32> {
        self.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH)
    }

    pub fn get_ram_vendor(&self) -> anyhow::Result<&'static str> {
        let value = self.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID)?;
        let name = match value {
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_SAMSUNG => "Samsung",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_QIMONDA => "Qimonda",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ELPIDA => "Elpida",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ETRON => "Etron",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_NANYA => "Nanya",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_HYNIX => "SK Hynix",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_MOSEL => "Mosel",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_WINBOND => "Winbond",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_ESMT => "ESMT",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_MICRON => "Micron",
            NV2080_CTRL_FB_INFO_MEMORYINFO_VENDOR_ID_UNKNOWN => "Unknown",
            _ => "Unrecognized",
        };
        Ok(name)
    }

    pub fn get_l2_cache_size(&self) -> anyhow::Result<u32> {
        self.get_fb_info(NV2080_CTRL_FB_INFO_INDEX_L2CACHE_SIZE)
    }

    pub fn get_thermal_sensors_info(&self) -> anyhow::Result<Vec<(u32, u32)>> {
        let mut params = NV2080_CTRL_THERMAL_GET_THERMAL_SENSORS_INFO_PARAMS::default();
        self.query_rm_control(NV2080_CTRL_CMD_THERMAL_GET_THERMAL_SENSORS_INFO, &mut params)?;

        let sensors = params.sensorInfo[..params.sensorCount as usize]
            .iter()
            .map(|info| (info.sensorId, info.sensorType))
            .collect();

        Ok(sensors)
    }

    pub fn get_voltage(&self, domain_id: u32) -> anyhow::Result<u32> {
        let mut params = NV2080_CTRL_VOLT_GET_VOLTAGE_PARAMS {
            voltDomainId: domain_id,
            voltageuV: 0,
        };
        self.query_rm_control(NV2080_CTRL_CMD_VOLT_GET_VOLTAGE, &mut params)?;
        Ok(params.voltageuV)
    }

    pub fn get_temperatures(&self, mask: u32) -> anyhow::Result<Vec<(u32, i32)>> {
        let mut params = NV2080_CTRL_THERMAL_GET_TEMPERATURES_PARAMS {
            mask,
            temperatures: [0; NV2080_CTRL_THERMAL_SENSORS_MAX_COUNT],
        };
        self.query_rm_control(NV2080_CTRL_CMD_THERMAL_GET_TEMPERATURES, &mut params)?;

        let mut results = Vec::new();
        for i in 0..NV2080_CTRL_THERMAL_SENSORS_MAX_COUNT {
            if (mask & (1 << i)) != 0 {
                results.push((i as u32, params.temperatures[i]));
            }
        }

        Ok(results)
    }

    fn get_fb_info(&self, stat_index: u32) -> anyhow::Result<u32> {
        let mut info_list = vec![NV2080_CTRL_FB_INFO {
            index: stat_index,
            data: 0,
        }];
        let mut params = NV2080_CTRL_FB_GET_INFO_PARAMS {
            fbInfoListSize: u32::try_from(info_list.len()).unwrap(),
            fbInfoList: info_list.as_mut_ptr().cast(),
        };

        self.query_rm_control(NV2080_CTRL_CMD_FB_GET_INFO, &mut params)?;

        Ok(info_list[0].data)
    }

    fn query_rm_control<T: Copy>(&self, cmd: u32, params: &mut T) -> anyhow::Result<()> {
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
            bail!("Nvidia request failed with status {:x}", request.status);
        }

        Ok(())
    }
}

ioctl_readwrite!(
    rm_alloc_nvos21,
    NV_IOCTL_MAGIC,
    NV_ESC_RM_ALLOC,
    NVOS21_PARAMETERS
);

ioctl_readwrite!(
    rm_alloc_nvos64,
    NV_IOCTL_MAGIC,
    NV_ESC_RM_ALLOC,
    NVOS64_PARAMETERS
);

ioctl_readwrite!(register_fd, NV_IOCTL_MAGIC, NV_ESC_REGISTER_FD, RawFd);

ioctl_readwrite!(
    rm_control_nvos54,
    NV_IOCTL_MAGIC,
    NV_ESC_RM_CONTROL,
    NVOS54_PARAMETERS
);
