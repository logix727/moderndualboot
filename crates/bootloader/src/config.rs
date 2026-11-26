//! Configuration loader for `modernboot-efi`.
//!
//! Reads `\EFI\ModernBoot\config.json` from the EFI System Partition that the
//! bootloader was launched from, falling back to [`BootConfig::default`] if the
//! file is missing or cannot be parsed.

use alloc::string::String;
use modernboot_shared::{BootConfig, DEFAULT_CONFIG_PATH};
use uefi::boot;
use uefi::fs::FileSystem;
use uefi::CString16;

/// Loads [`BootConfig`] from the current image's EFI System Partition.
pub fn load_config() -> BootConfig {
    let Ok(sfs) = boot::get_image_file_system(boot::image_handle()) else {
        return BootConfig::default();
    };
    let mut fs = FileSystem::new(sfs);

    let Ok(path) = CString16::try_from(DEFAULT_CONFIG_PATH) else {
        return BootConfig::default();
    };

    let Ok(json_str): Result<String, _> = fs.read_to_string(path.as_ref()) else {
        return BootConfig::default();
    };

    BootConfig::from_json_str(&json_str).unwrap_or_default()
}
