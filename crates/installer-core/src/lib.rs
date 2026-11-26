//! Cross-platform UEFI installer, ESP manager, NVRAM editor, and diagnostics
//! for ModernDualBoot.

pub mod efi;
pub mod esp;
pub mod nvram;
pub mod platform;

use modernboot_shared::{BootConfig, BootEntry, DEFAULT_CONFIG_PATH};
use std::fs;
use thiserror::Error;

pub use efi::FirmwareBootEntry;
pub use esp::EspMount;
pub use platform::{
    check_bitlocker, check_secure_boot, is_admin, is_uefi, relaunch_as_admin, trigger_reboot,
};

/// UEFI path of the ModernBoot EFI binary on the EFI System Partition.
pub const MODERNBOOT_EFI_PATH: &str = "\\EFI\\ModernBoot\\bootx64.efi";

/// UEFI directory of ModernBoot on the EFI System Partition.
pub const MODERNBOOT_EFI_DIR: &str = "\\EFI\\ModernBoot";

/// UEFI NVRAM BootXXXX description for ModernBoot.
pub const MODERNBOOT_ENTRY_DESCRIPTION: &str = "ModernBoot";

/// Convenience result alias for `modernboot-installer`.
pub type Result<T> = std::result::Result<T, InstallerError>;

/// Errors returned by `modernboot-installer` operations.
#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON serialization/deserialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("EFI System Partition not found: {0}")]
    EspNotFound(String),

    #[error("UEFI NVRAM error: {0}")]
    Nvram(String),

    #[error("invalid EFI_LOAD_OPTION binary data: {0}")]
    InvalidLoadOption(String),

    #[error("permission denied: {0}")]
    PermissionDenied(String),

    #[error("system command failed: {0}")]
    CommandFailed(String),

    #[error("matching firmware boot entry not found for '{0}'")]
    BootEntryNotFound(String),

    #[error("this operation is not supported on the current platform")]
    UnsupportedPlatform,
}

/// Comprehensive system health, ESP, and UEFI NVRAM diagnostic snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStatus {
    /// Whether the process has Administrator (Windows) or root (Linux) privileges.
    pub is_admin: bool,
    /// Whether the system booted in UEFI mode.
    pub is_uefi: bool,
    /// Secure Boot state (`Some(true)` = enabled, `Some(false)` = disabled, `None` = unknown).
    pub secure_boot_enabled: Option<bool>,
    /// BitLocker protection state on Windows OS drive (`None` on Linux or if unknown).
    pub bitlocker_active: Option<bool>,
    /// Whether an EFI System Partition was discovered/mounted.
    pub esp_found: bool,
    /// Filesystem root path of the ESP if accessible.
    pub esp_path: Option<String>,
    /// Whether `\EFI\ModernBoot\bootx64.efi` is present on the ESP.
    pub modernboot_installed: bool,
    /// Whether ModernBoot is currently first in UEFI `BootOrder`.
    pub modernboot_is_first: bool,
    /// Raw UEFI `BootOrder` list of `BootXXXX` IDs.
    pub boot_order: Vec<u16>,
    /// Decoded UEFI `BootXXXX` firmware boot entries.
    pub boot_entries: Vec<FirmwareBootEntry>,
    /// Human-readable warnings surfaced during diagnostics (e.g. BitLocker, Secure Boot, non-admin).
    pub warnings: Vec<String>,
}

/// Summary returned after a successful [`install_modernboot`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub esp_path: String,
    pub efi_installed_path: String,
    pub config_installed_path: String,
    pub boot_entry_id: Option<u16>,
    pub boot_order: Vec<u16>,
    pub warnings: Vec<String>,
}

/// Runs non-destructive diagnostics on privileges, UEFI mode, Secure Boot, BitLocker,
/// the EFI System Partition, and UEFI NVRAM `BootOrder`.
pub fn diagnose_system() -> SystemStatus {
    let admin = platform::is_admin();
    let uefi = platform::is_uefi();
    let secure_boot_enabled = if uefi {
        platform::check_secure_boot()
    } else {
        None
    };
    let bitlocker_active = platform::check_bitlocker();

    let mut warnings = Vec::new();
    if !admin {
        warnings.push(
            "Not running with Administrator/root privileges; ESP mounting and NVRAM edits require elevation."
                .into(),
        );
    }
    if !uefi {
        warnings.push("System does not appear to be booted in UEFI mode.".into());
    }
    if secure_boot_enabled == Some(true) {
        warnings.push(
            "Secure Boot is enabled. Unsigned ModernBoot EFI binaries will be blocked unless Secure Boot is disabled or the binary is signed/enrolled."
                .into(),
        );
    }
    if bitlocker_active == Some(true) {
        warnings.push(
            "BitLocker is active on C:. Ensure you have your BitLocker recovery key backed up before changing UEFI boot order."
                .into(),
        );
    }

    let (esp_found, esp_path, modernboot_installed) = match EspMount::acquire() {
        Ok(esp) => {
            let installed = esp.find_efi_file_ci(MODERNBOOT_EFI_PATH).is_some();
            (true, Some(esp.root_string()), installed)
        }
        Err(_) => (false, None, false),
    };

    let (boot_order, boot_entries) = if uefi {
        nvram::list_boot_entries().unwrap_or_default()
    } else {
        (Vec::new(), Vec::new())
    };

    let modernboot_id = nvram::find_modernboot_entry(&boot_entries).map(|e| e.id);
    let modernboot_is_first = match (modernboot_id, boot_order.first().copied()) {
        (Some(mb_id), Some(first_id)) => mb_id == first_id,
        _ => false,
    };

    if modernboot_installed && !modernboot_is_first && modernboot_id.is_some() {
        warnings.push(
            "ModernBoot is installed but is not first in UEFI BootOrder (e.g. after a Windows Update). Use 'Fix Boot Order' to restore it."
                .into(),
        );
    }

    SystemStatus {
        is_admin: admin,
        is_uefi: uefi,
        secure_boot_enabled,
        bitlocker_active,
        esp_found,
        esp_path,
        modernboot_installed,
        modernboot_is_first,
        boot_order,
        boot_entries,
        warnings,
    }
}

/// Installs or updates ModernBoot on the EFI System Partition, writes `config.json`,
/// and registers ModernBoot first in UEFI `BootOrder`.
pub fn install_modernboot(efi_bytes: &[u8], config: &BootConfig) -> Result<InstallReport> {
    if !platform::is_uefi() {
        return Err(InstallerError::Nvram(
            "cannot install UEFI bootloader on a non-UEFI system".into(),
        ));
    }

    let mut warnings = Vec::new();
    if platform::check_bitlocker() == Some(true) {
        warnings.push(
            "BitLocker is active on C:. Keep your recovery key handy in case Windows requests it after boot-order changes."
                .into(),
        );
    }
    if platform::check_secure_boot() == Some(true) {
        warnings.push(
            "Secure Boot is enabled; ensure your ModernBoot binary is signed or Secure Boot is disabled."
                .into(),
        );
    }

    let esp = EspMount::acquire()?;
    let esp_path = esp.root_string();

    let efi_dest = esp.resolve_efi_path(MODERNBOOT_EFI_PATH);
    if let Some(parent) = efi_dest.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&efi_dest, efi_bytes)?;

    let config_dest = esp.resolve_efi_path(DEFAULT_CONFIG_PATH);
    if let Some(parent) = config_dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = config.to_json_pretty()?;
    fs::write(&config_dest, json.as_bytes())?;

    let (boot_id, boot_order) = nvram::register_modernboot_entry()?;

    Ok(InstallReport {
        esp_path,
        efi_installed_path: efi_dest.to_string_lossy().into_owned(),
        config_installed_path: config_dest.to_string_lossy().into_owned(),
        boot_entry_id: Some(boot_id),
        boot_order,
        warnings,
    })
}

/// Uninstalls ModernBoot by removing `\EFI\ModernBoot\` from the ESP and deleting its
/// UEFI NVRAM `BootXXXX` entry and `BootOrder` reference.
pub fn uninstall_modernboot() -> Result<()> {
    let esp = EspMount::acquire()?;
    let dir = esp
        .find_efi_file_ci(MODERNBOOT_EFI_DIR)
        .unwrap_or_else(|| esp.resolve_efi_path(MODERNBOOT_EFI_DIR));
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }

    nvram::remove_modernboot_entry()?;
    Ok(())
}

/// Moves ModernBoot back to index `0` in UEFI `BootOrder` (useful after Windows Updates).
pub fn fix_boot_order() -> Result<Vec<u16>> {
    nvram::fix_boot_order()
}

/// Scans the EFI System Partition for known OS bootloaders (`KNOWN_OS_PATHS` + Linux UKIs).
pub fn scan_esp_oses() -> Result<Vec<BootEntry>> {
    let esp = EspMount::acquire()?;
    Ok(esp.scan_oses())
}

/// Reads `\EFI\ModernBoot\config.json` from the ESP if present.
pub fn read_esp_config() -> Result<Option<BootConfig>> {
    let esp = EspMount::acquire()?;
    let Some(path) = esp.find_efi_file_ci(DEFAULT_CONFIG_PATH) else {
        return Ok(None);
    };
    let text = fs::read_to_string(path)?;
    let cfg = BootConfig::from_json_str(&text)?;
    Ok(Some(cfg))
}

/// Writes `\EFI\ModernBoot\config.json` to the ESP.
pub fn write_esp_config(config: &BootConfig) -> Result<()> {
    let esp = EspMount::acquire()?;
    let dest = esp.resolve_efi_path(DEFAULT_CONFIG_PATH);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = config.to_json_pretty()?;
    fs::write(&dest, json.as_bytes())?;
    Ok(())
}

/// Sets `BootNext` to the firmware boot entry matching `entry` and immediately reboots.
pub fn reboot_to_os(entry: &BootEntry) -> Result<()> {
    let (_order, fw_entries) = nvram::list_boot_entries()?;
    let target = find_matching_firmware_entry(&fw_entries, entry)
        .ok_or_else(|| InstallerError::BootEntryNotFound(entry.label.clone()))?;
    nvram::set_boot_next(target.id)?;
    platform::trigger_reboot()
}

/// Sets `BootNext` directly to `boot_id`.
pub fn set_boot_next(boot_id: u16) -> Result<()> {
    nvram::set_boot_next(boot_id)
}

/// Requests reboot directly into UEFI Firmware Setup (`OsIndications`) and triggers reboot.
pub fn reboot_to_firmware() -> Result<()> {
    nvram::set_boot_to_firmware_ui()?;
    platform::trigger_reboot()
}

/// Matches a [`BootEntry`] against firmware NVRAM [`FirmwareBootEntry`] items by EFI path
/// (preferred) or description/label.
pub fn find_matching_firmware_entry<'a>(
    fw_entries: &'a [FirmwareBootEntry],
    entry: &BootEntry,
) -> Option<&'a FirmwareBootEntry> {
    let target_norm = entry.efi_path.replace('/', "\\");
    if let Some(by_path) = fw_entries.iter().find(|fw| {
        fw.efi_path
            .as_deref()
            .map(|p| p.replace('/', "\\").eq_ignore_ascii_case(&target_norm))
            .unwrap_or(false)
    }) {
        return Some(by_path);
    }

    fw_entries
        .iter()
        .find(|fw| fw.description.eq_ignore_ascii_case(&entry.label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use modernboot_shared::{OsKind, ThemeConfig, CONFIG_VERSION};

    #[test]
    fn config_serialization_for_esp_round_trip() {
        let cfg = BootConfig {
            version: CONFIG_VERSION,
            timeout_seconds: 5,
            default_entry_id: Some("windows".into()),
            auto_scan: true,
            theme: ThemeConfig::default(),
            entries: vec![BootEntry {
                id: "windows".into(),
                label: "Windows Boot Manager".into(),
                os_kind: OsKind::Windows,
                efi_path: "\\EFI\\Microsoft\\Boot\\bootmgfw.efi".into(),
                hidden: false,
                auto_detected: true,
                icon_override: None,
            }],
            show_firmware_setup: true,
        };

        let json = cfg.to_json_pretty().expect("serialize BootConfig");
        let parsed = BootConfig::from_json_str(&json).expect("deserialize BootConfig");
        assert_eq!(parsed, cfg);
    }

    #[test]
    fn match_firmware_entry_by_efi_path_case_insensitive() {
        let fw = vec![
            FirmwareBootEntry {
                id: 1,
                attributes: 1,
                description: "Windows Boot Manager".into(),
                efi_path: Some("\\EFI\\Microsoft\\Boot\\bootmgfw.efi".into()),
                device_path_raw: vec![],
                optional_data: vec![],
                is_active: true,
            },
            FirmwareBootEntry {
                id: 2,
                attributes: 1,
                description: "ubuntu".into(),
                efi_path: Some("\\EFI\\ubuntu\\shimx64.efi".into()),
                device_path_raw: vec![],
                optional_data: vec![],
                is_active: true,
            },
        ];

        let entry = BootEntry {
            id: "ubuntu".into(),
            label: "Ubuntu 24.04".into(),
            os_kind: OsKind::Ubuntu,
            efi_path: "/efi/UBUNTU/shimx64.efi".into(),
            hidden: false,
            auto_detected: true,
            icon_override: None,
        };

        let matched = find_matching_firmware_entry(&fw, &entry).expect("should match ubuntu");
        assert_eq!(matched.id, 2);
    }
}
