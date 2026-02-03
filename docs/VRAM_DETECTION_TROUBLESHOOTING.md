# NVIDIA VRAM Detection Troubleshooting Guide

This guide helps diagnose and fix NVIDIA VRAM detection issues in the Clevo UI daemon.

## Quick Diagnostics

### 1. Check NVIDIA Driver is Loaded

```bash
# Check if NVIDIA driver module is loaded
lsmod | grep nvidia

# Check NVIDIA device files exist
ls -la /dev/nvidia* /dev/nvidiactl

# Expected output should show:
# /dev/nvidia0, /dev/nvidia1, etc. (one per GPU)
# /dev/nvidiactl (control device)
```

### 2. Check Permissions

```bash
# Check current user's groups
groups

# Add user to video group if not already a member
sudo usermod -aG video $USER

# Log out and log back in for group changes to take effect
```

### 3. Test NVIDIA Driver Manually

```bash
# Use nvidia-smi to verify GPUs are detected
nvidia-smi

# Check NVML library can enumerate devices
nvidia-smi -L
```

## Common Error Messages and Solutions

### Error: "Permission denied"
**Cause**: User doesn't have permissions to access `/dev/nvidiactl` or `/dev/nvidia*`

**Solution**:
```bash
sudo usermod -aG video $USER
# Then log out and log back in
```

### Error: "No such file or directory"
**Cause**: NVIDIA driver not loaded or GPU not detected

**Solution**:
```bash
# Check if NVIDIA module is loaded
sudo modprobe nvidia

# If module doesn't exist, install NVIDIA drivers:
# - Ubuntu/Debian: sudo apt install nvidia-driver-XXX
# - Arch: sudo pacman -S nvidia
# - Fedora: sudo dnf install akmod-nvidia
```

### Error: "Device or resource busy"
**Cause**: Another process is exclusively using the GPU

**Solution**:
```bash
# Check what's using the GPU
sudo lsof /dev/nvidia*

# If needed, stop the conflicting process
```

### Error: "RM control failed: 0x00000056 (NV_ERR_GPU_NOT_FULL_POWER)"
**Cause**: GPU is in a low-power/suspended state

**Solution**:
- This is expected behavior when GPU is idle/suspended
- VRAM info will be retrieved from cache when GPU wakes up
- To force GPU to wake up: run `nvidia-smi` or start a GPU application

### Error: "RM control failed: 0x0000000a (NV_ERR_NOT_SUPPORTED)"
**Cause**: Feature not supported on this GPU or driver version

**Solution**:
- Update to the latest NVIDIA driver
- Some older GPUs may not support all VRAM info queries
- Check NVIDIA driver release notes for your GPU

## Daemon Log Messages

### Successful Detection
```
[INFO] GPU 0: Successfully retrieved partial VRAM info for minor 0: type=0x0000000b, bus=256 bits, vendor=0x00000002
[INFO] GPU 0: VRAM detected - Type: Some("GDDR6"), Vendor: Some("Samsung"), Bus Width: Some(256) bits, Bandwidth: Some(448.0) GB/s
```

### Failed Detection with Details
```
[WARN] Failed to get RAM type for minor 0 (index=0x01): RM control failed: status=0x00000056 (NV_ERR_GPU_NOT_FULL_POWER) - This may indicate driver/GPU incompatibility or suspended GPU state
[WARN] GPU 0: VRAM info not available - Check daemon logs above for detailed error messages
```

### Cache Usage
```
[DEBUG] GPU 0: Using cached metadata
[DEBUG] GPU 0: Using cached VRAM info - Type: Some("GDDR6"), Vendor: Some("Samsung"), Bus: Some(256)
```

### Cache Retry on None
```
[INFO] GPU 0: Cached VRAM info is all None, retrying detection
[INFO] GPU 0: Successfully detected VRAM on retry - Type: Some("GDDR6"), Vendor: Some("Samsung"), Bus: Some(256)
```

## Debug Logging

To enable debug-level logging for VRAM detection:

1. Set environment variable:
```bash
export RUST_LOG=hw.detect=debug
```

2. Run the daemon:
```bash
./target/debug/lapsphere-daemon
```

3. Look for detailed IOCTL logging:
```
[DEBUG] get_fb_info: index=0x01, hClient=0x1, hObject=0x2, cmd=0x20800101, paramsSize=16
[DEBUG] get_fb_info: IOCTL succeeded, request.status=0x00000000, info.data=0x0000000b
```

## Verification Checklist

- [ ] NVIDIA driver is loaded (`lsmod | grep nvidia`)
- [ ] Device files exist (`ls /dev/nvidia*`)
- [ ] User is in video group (`groups | grep video`)
- [ ] nvidia-smi works (`nvidia-smi`)
- [ ] Daemon has permission to access devices
- [ ] Logs show detailed error messages if detection fails
- [ ] VRAM info appears in GUI when GPU is active

## NVIDIA RM Error Codes Reference

| Code | Name | Meaning |
|------|------|---------|
| 0x00000001 | NV_ERR_INVALID_ARGUMENT | Invalid parameter passed to RM control |
| 0x00000002 | NV_ERR_INVALID_OBJECT_HANDLE | Invalid object handle |
| 0x00000003 | NV_ERR_INVALID_OBJECT_PARENT | Invalid parent object |
| 0x00000005 | NV_ERR_INSUFFICIENT_RESOURCES | Out of memory or resources |
| 0x00000006 | NV_ERR_INVALID_FLAGS | Invalid flags parameter |
| 0x00000008 | NV_ERR_INVALID_STATE | Operation not valid in current state |
| 0x0000000a | NV_ERR_NOT_SUPPORTED | Feature not supported |
| 0x0000000d | NV_ERR_OBJECT_NOT_FOUND | Requested object not found |
| 0x00000056 | NV_ERR_GPU_NOT_FULL_POWER | GPU in low-power state |
| 0x0000ffff | NV_ERR_GENERIC | Generic/unspecified error |

## Constants Verification

The daemon uses the following NVIDIA RM API constants, verified against NVIDIA headers and LACT implementation:

- `NV2080_CTRL_CMD_FB_GET_INFO = 0x20800101` - Framebuffer info query command
- `NV2080_CTRL_FB_INFO_INDEX_RAM_TYPE = 0x01` - RAM type query index
- `NV2080_CTRL_FB_INFO_INDEX_BUS_WIDTH = 0x02` - Bus width query index
- `NV2080_CTRL_FB_INFO_INDEX_MEMORYINFO_VENDOR_ID = 0x06` - Vendor ID query index

These match the NVIDIA open-source kernel driver headers.

## Need More Help?

If VRAM detection still fails after following this guide:

1. Enable debug logging (see above)
2. Collect daemon logs showing the error
3. Run `nvidia-smi -q` and include output
4. Include GPU model and driver version (`nvidia-smi --query-gpu=name,driver_version --format=csv`)
5. Open an issue with all the above information
