use nvml_wrapper::sys::nvmlClockType_t;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct nvmlClockOffset {
    pub version: u32,
    pub type_: nvmlClockType_t,
    pub pstate: u32,
    pub clock_offset_mhz: i32,
    pub min_clock_offset_mhz: i32,
    pub max_clock_offset_mhz: i32,
}