//! UEFI image chainloader and firmware setup reboot helper.

use alloc::vec::Vec;
use crate::scanner::ResolvedEntry;
use uefi::boot::{self, LoadImageSource};
use uefi::proto::device_path::build::{self, DevicePathBuilder};
use uefi::proto::device_path::DevicePath;
use uefi::proto::loaded_image::LoadedImage;
use uefi::runtime::{self, ResetType, VariableAttributes, VariableVendor};
use uefi::{cstr16, CString16, Status};
use uefi::proto::BootPolicy;

const EFI_OS_INDICATIONS_BOOT_TO_FW_UI: u64 = 0x0000_0000_0000_0001;

/// Chainloads the selected [`ResolvedEntry`] (or reboots into UEFI Firmware Setup).
///
/// Returns `Err(&'static str)` with a human-readable message if loading or starting
/// the target EFI binary fails.
pub fn boot_entry(entry: &ResolvedEntry) -> Result<(), &'static str> {
    if entry.is_firmware_setup {
        return reboot_to_firmware_setup();
    }

    let esp_handle = match entry.esp_handle {
        Some(h) => h,
        None => {
            let loaded = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
                .map_err(|_| "Target EFI partition not found")?;
            loaded.device().ok_or("Target EFI partition not found")?
        }
    };

    let esp_dp = boot::open_protocol_exclusive::<DevicePath>(esp_handle)
        .map_err(|_| "Failed to open ESP DevicePath protocol")?;

    let efi_path_win = entry.entry.efi_path.replace('/', "\\");
    let path_cstr =
        CString16::try_from(efi_path_win.as_str()).map_err(|_| "Invalid EFI binary path")?;

    let mut dp_buf = Vec::new();
    let mut builder = DevicePathBuilder::with_vec(&mut dp_buf);
    for node in esp_dp.node_iter() {
        builder = builder
            .push(&node)
            .map_err(|_| "Failed to copy ESP device path node")?;
    }
    builder = builder
        .push(&build::media::FilePath {
            path_name: path_cstr.as_ref(),
        })
        .map_err(|_| "Failed to append file path node")?;
    let full_dp = builder
        .finalize()
        .map_err(|_| "Failed to finalize target DevicePath")?;

    let child_handle = boot::load_image(
        boot::image_handle(),
        LoadImageSource::FromDevicePath {
            device_path: full_dp,
            boot_policy: BootPolicy::ExactMatch,
        },
    )
    .map_err(|err| match err.status() {
        Status::NOT_FOUND => "EFI binary not found on ESP",
        Status::SECURITY_VIOLATION | Status::ACCESS_DENIED => {
            "Blocked by Secure Boot / security policy"
        }
        Status::LOAD_ERROR | Status::UNSUPPORTED => "Invalid or unsupported EFI image",
        _ => "Failed to load EFI image",
    })?;

    boot::start_image(child_handle).map_err(|_| "Target EFI image returned an error")
}

/// Sets `OsIndications` (`EFI_OS_INDICATIONS_BOOT_TO_FW_UI`) and performs a warm
/// reset into UEFI Firmware Setup.
pub fn reboot_to_firmware_setup() -> Result<(), &'static str> {
    let mut buf = [0u8; 8];
    let supported = match runtime::get_variable(
        cstr16!("OsIndicationsSupported"),
        &VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        Ok((bytes, _)) if bytes.len() >= 8 => u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8])),
        _ => EFI_OS_INDICATIONS_BOOT_TO_FW_UI,
    };

    if supported & EFI_OS_INDICATIONS_BOOT_TO_FW_UI == 0 {
        return Err("Firmware does not support OsIndications FW UI");
    }

    let mut current = match runtime::get_variable(
        cstr16!("OsIndications"),
        &VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        Ok((bytes, _)) if bytes.len() >= 8 => u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8])),
        _ => 0,
    };
    current |= EFI_OS_INDICATIONS_BOOT_TO_FW_UI;

    let attrs = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;

    runtime::set_variable(
        cstr16!("OsIndications"),
        &VariableVendor::GLOBAL_VARIABLE,
        attrs,
        &current.to_le_bytes(),
    )
    .map_err(|_| "Failed to write OsIndications EFI variable")?;

    runtime::reset(ResetType::WARM, Status::SUCCESS, None)
}
