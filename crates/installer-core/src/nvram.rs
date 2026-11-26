//! Cross-platform UEFI NVRAM variable access and `BootOrder` / `BootXXXX` management.

use crate::efi::{
    build_device_path, encode_boot_order, extract_hard_drive_node, parse_boot_order,
    FirmwareBootEntry, LoadOption, EFI_OS_INDICATIONS_BOOT_TO_FW_UI, LOAD_OPTION_ACTIVE,
};
use crate::{InstallerError, Result, MODERNBOOT_EFI_PATH, MODERNBOOT_ENTRY_DESCRIPTION};

/// Reads the UEFI `BootOrder` variable. Returns an empty vector if not set.
pub fn read_boot_order() -> Result<Vec<u16>> {
    match get_efi_var("BootOrder")? {
        Some(bytes) => Ok(parse_boot_order(&bytes)),
        None => Ok(Vec::new()),
    }
}

/// Writes the UEFI `BootOrder` variable.
pub fn write_boot_order(order: &[u16]) -> Result<()> {
    let bytes = encode_boot_order(order);
    set_efi_var("BootOrder", &bytes)
}

/// Reads a single `BootXXXX` entry from UEFI NVRAM.
pub fn read_boot_entry(id: u16) -> Result<Option<FirmwareBootEntry>> {
    let name = format!("Boot{:04X}", id);
    match get_efi_var(&name)? {
        Some(bytes) => {
            let opt = LoadOption::from_bytes(&bytes)?;
            Ok(Some(opt.into_entry(id)))
        }
        None => Ok(None),
    }
}

/// Reads `BootOrder` and resolves all referenced `BootXXXX` entries (plus any ModernBoot
/// entries found in `0x0000..=0x0020` even if temporarily dropped from `BootOrder`).
pub fn list_boot_entries() -> Result<(Vec<u16>, Vec<FirmwareBootEntry>)> {
    let order = read_boot_order()?;
    let mut entries = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for &id in &order {
        if seen.insert(id) {
            if let Ok(Some(entry)) = read_boot_entry(id) {
                entries.push(entry);
            }
        }
    }

    // Probe low BootXXXX slots in case an entry exists outside BootOrder.
    for id in 0..=0x0020u16 {
        if seen.contains(&id) {
            continue;
        }
        if let Ok(Some(entry)) = read_boot_entry(id) {
            entries.push(entry);
        }
    }

    Ok((order, entries))
}

/// Finds an existing `ModernBoot` firmware boot entry.
pub fn find_modernboot_entry(entries: &[FirmwareBootEntry]) -> Option<&FirmwareBootEntry> {
    entries.iter().find(|e| {
        e.description.eq_ignore_ascii_case(MODERNBOOT_ENTRY_DESCRIPTION)
            || e.efi_path
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(MODERNBOOT_EFI_PATH))
                .unwrap_or(false)
    })
}

/// Registers (or updates) the `ModernBoot` `BootXXXX` NVRAM entry and places it at index 0
/// of `BootOrder`. Returns `(boot_id, new_boot_order)`.
pub fn register_modernboot_entry() -> Result<(u16, Vec<u16>)> {
    let (mut order, entries) = list_boot_entries()?;

    if let Some(existing) = find_modernboot_entry(&entries) {
        let id = existing.id;
        move_to_front(&mut order, id);
        write_boot_order(&order)?;
        return Ok((id, order));
    }

    // Find a HardDrive device path node from an existing entry (e.g. Windows Boot Manager)
    // so our BootXXXX entry points to the same GPT ESP partition.
    let hd_node = entries
        .iter()
        .find_map(|e| extract_hard_drive_node(&e.device_path_raw));

    let boot_id = find_free_boot_id(&entries, &order)?;
    let device_path = build_device_path(hd_node.as_deref(), MODERNBOOT_EFI_PATH);
    let load_opt = LoadOption {
        attributes: LOAD_OPTION_ACTIVE,
        description: MODERNBOOT_ENTRY_DESCRIPTION.to_string(),
        file_path_list: device_path,
        optional_data: Vec::new(),
    };

    let var_name = format!("Boot{:04X}", boot_id);
    set_efi_var(&var_name, &load_opt.to_bytes())?;

    move_to_front(&mut order, boot_id);
    write_boot_order(&order)?;

    Ok((boot_id, order))
}

/// Removes the `ModernBoot` entry from `BootOrder` and deletes its `BootXXXX` NVRAM variable.
pub fn remove_modernboot_entry() -> Result<()> {
    let (mut order, entries) = list_boot_entries()?;
    let modern_ids: Vec<u16> = entries
        .iter()
        .filter(|e| {
            e.description.eq_ignore_ascii_case(MODERNBOOT_ENTRY_DESCRIPTION)
                || e.efi_path
                    .as_deref()
                    .map(|p| p.eq_ignore_ascii_case(MODERNBOOT_EFI_PATH))
                    .unwrap_or(false)
        })
        .map(|e| e.id)
        .collect();

    if modern_ids.is_empty() {
        return Ok(());
    }

    order.retain(|id| !modern_ids.contains(id));
    write_boot_order(&order)?;

    for id in modern_ids {
        let var_name = format!("Boot{:04X}", id);
        let _ = delete_efi_var(&var_name);
    }

    Ok(())
}

/// Moves `ModernBoot` back to index 0 of `BootOrder` (e.g. after a Windows Update).
pub fn fix_boot_order() -> Result<Vec<u16>> {
    let (mut order, entries) = list_boot_entries()?;
    let existing = find_modernboot_entry(&entries)
        .ok_or_else(|| InstallerError::Nvram("ModernBoot NVRAM entry not found".into()))?;
    move_to_front(&mut order, existing.id);
    write_boot_order(&order)?;
    Ok(order)
}

/// Sets `BootNext` to the specified `boot_id` for a one-time boot on the next restart.
pub fn set_boot_next(boot_id: u16) -> Result<()> {
    set_efi_var("BootNext", &boot_id.to_le_bytes())
}

/// Sets `EFI_OS_INDICATIONS_BOOT_TO_FW_UI` in `OsIndications` so the next reboot enters
/// UEFI Firmware Setup.
pub fn set_boot_to_firmware_ui() -> Result<()> {
    let existing = match get_efi_var("OsIndications")? {
        Some(bytes) if bytes.len() >= 8 => {
            u64::from_le_bytes(bytes[..8].try_into().unwrap_or([0; 8]))
        }
        _ => 0,
    };
    let updated = existing | EFI_OS_INDICATIONS_BOOT_TO_FW_UI;
    set_efi_var("OsIndications", &updated.to_le_bytes())
}

fn move_to_front(order: &mut Vec<u16>, id: u16) {
    order.retain(|&x| x != id);
    order.insert(0, id);
}

fn find_free_boot_id(entries: &[FirmwareBootEntry], order: &[u16]) -> Result<u16> {
    for candidate in 0..=0x00FFu16 {
        if !order.contains(&candidate) && !entries.iter().any(|e| e.id == candidate) {
            return Ok(candidate);
        }
    }
    Err(InstallerError::Nvram(
        "no free BootXXXX slot available in 0x0000..=0x00FF".into(),
    ))
}

// ---------------------------------------------------------------------------
// Platform-specific UEFI NVRAM variable backend
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod backend {
    use super::*;
    use crate::efi::EFI_GLOBAL_VARIABLE_GUID;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ENVVAR_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE, LUID,
    };
    use windows::Win32::Security::{
        AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
        TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::System::WindowsProgramming::{
        GetFirmwareEnvironmentVariableW, SetFirmwareEnvironmentVariableW,
    };

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn enable_system_environment_privilege() -> Result<()> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(
                GetCurrentProcess(),
                TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
                &mut token,
            )
            .map_err(|e| {
                InstallerError::PermissionDenied(format!("OpenProcessToken failed: {e}"))
            })?;

            let priv_name = to_wide("SeSystemEnvironmentPrivilege");
            let mut luid = LUID::default();
            let lookup_res = LookupPrivilegeValueW(PCWSTR::null(), PCWSTR(priv_name.as_ptr()), &mut luid);
            if let Err(e) = lookup_res {
                let _ = CloseHandle(token);
                return Err(InstallerError::PermissionDenied(format!(
                    "LookupPrivilegeValueW(SeSystemEnvironmentPrivilege) failed: {e}"
                )));
            }

            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };

            let adj_res = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
            let last_err = GetLastError();
            let _ = CloseHandle(token);

            if let Err(e) = adj_res {
                return Err(InstallerError::PermissionDenied(format!(
                    "AdjustTokenPrivileges failed: {e}"
                )));
            }
            if last_err.0 != 0 {
                return Err(InstallerError::PermissionDenied(format!(
                    "SeSystemEnvironmentPrivilege not held (error {}) — run as Administrator",
                    last_err.0
                )));
            }
            Ok(())
        }
    }

    pub fn get_efi_var(name: &str) -> Result<Option<Vec<u8>>> {
        enable_system_environment_privilege()?;
        let w_name = to_wide(name);
        let w_guid = to_wide(EFI_GLOBAL_VARIABLE_GUID);

        let mut buf = vec![0u8; 4096];
        loop {
            let len = unsafe {
                GetFirmwareEnvironmentVariableW(
                    PCWSTR(w_name.as_ptr()),
                    PCWSTR(w_guid.as_ptr()),
                    Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                    buf.len() as u32,
                )
            };
            if len > 0 {
                buf.truncate(len as usize);
                return Ok(Some(buf));
            }
            let err = unsafe { GetLastError() };
            if err == ERROR_ENVVAR_NOT_FOUND {
                return Ok(None);
            }
            if err == ERROR_INSUFFICIENT_BUFFER && buf.len() < 65536 {
                buf.resize(buf.len() * 2, 0);
                continue;
            }
            return Err(InstallerError::Nvram(format!(
                "GetFirmwareEnvironmentVariableW({name}) failed with Win32 error {}",
                err.0
            )));
        }
    }

    pub fn set_efi_var(name: &str, data: &[u8]) -> Result<()> {
        enable_system_environment_privilege()?;
        let w_name = to_wide(name);
        let w_guid = to_wide(EFI_GLOBAL_VARIABLE_GUID);

        let res = unsafe {
            SetFirmwareEnvironmentVariableW(
                PCWSTR(w_name.as_ptr()),
                PCWSTR(w_guid.as_ptr()),
                Some(data.as_ptr() as *const core::ffi::c_void),
                data.len() as u32,
            )
        };
        res.map_err(|e| {
            InstallerError::Nvram(format!("SetFirmwareEnvironmentVariableW({name}) failed: {e}"))
        })
    }

    pub fn delete_efi_var(name: &str) -> Result<()> {
        enable_system_environment_privilege()?;
        let w_name = to_wide(name);
        let w_guid = to_wide(EFI_GLOBAL_VARIABLE_GUID);

        let res = unsafe {
            SetFirmwareEnvironmentVariableW(
                PCWSTR(w_name.as_ptr()),
                PCWSTR(w_guid.as_ptr()),
                None,
                0,
            )
        };
        res.map_err(|e| {
            InstallerError::Nvram(format!(
                "SetFirmwareEnvironmentVariableW({name}, delete) failed: {e}"
            ))
        })
    }
}

#[cfg(target_os = "linux")]
mod backend {
    use super::*;
    use crate::efi::EFI_GLOBAL_VARIABLE_GUID_BARE;
    use std::fs;
    use std::path::PathBuf;

    // Default efivarfs attributes: NON_VOLATILE (0x1) | BOOTSERVICE_ACCESS (0x2) | RUNTIME_ACCESS (0x4)
    const EFIVAR_ATTRS_DEFAULT: [u8; 4] = [0x07, 0x00, 0x00, 0x00];

    fn efivar_path(name: &str) -> PathBuf {
        PathBuf::from(format!(
            "/sys/firmware/efi/efivars/{name}-{EFI_GLOBAL_VARIABLE_GUID_BARE}"
        ))
    }

    pub fn get_efi_var(name: &str) -> Result<Option<Vec<u8>>> {
        let path = efivar_path(name);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(&path)?;
        if raw.len() <= 4 {
            return Ok(Some(Vec::new()));
        }
        Ok(Some(raw[4..].to_vec()))
    }

    pub fn set_efi_var(name: &str, data: &[u8]) -> Result<()> {
        let path = efivar_path(name);
        if path.exists() {
            // Clear immutable flag if set by kernel efivarfs
            let _ = std::process::Command::new("chattr")
                .args(["-i", &path.to_string_lossy()])
                .status();
        }
        let mut payload = Vec::with_capacity(4 + data.len());
        payload.extend_from_slice(&EFIVAR_ATTRS_DEFAULT);
        payload.extend_from_slice(data);
        fs::write(&path, &payload).map_err(|e| {
            InstallerError::Nvram(format!("failed to write efivar {}: {e}", path.display()))
        })
    }

    pub fn delete_efi_var(name: &str) -> Result<()> {
        let path = efivar_path(name);
        if !path.exists() {
            return Ok(());
        }
        let _ = std::process::Command::new("chattr")
            .args(["-i", &path.to_string_lossy()])
            .status();
        fs::remove_file(&path).map_err(|e| {
            InstallerError::Nvram(format!("failed to delete efivar {}: {e}", path.display()))
        })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod backend {
    use super::*;

    pub fn get_efi_var(_name: &str) -> Result<Option<Vec<u8>>> {
        Err(InstallerError::UnsupportedPlatform)
    }

    pub fn set_efi_var(_name: &str, _data: &[u8]) -> Result<()> {
        Err(InstallerError::UnsupportedPlatform)
    }

    pub fn delete_efi_var(_name: &str) -> Result<()> {
        Err(InstallerError::UnsupportedPlatform)
    }
}

pub use backend::{delete_efi_var, get_efi_var, set_efi_var};
