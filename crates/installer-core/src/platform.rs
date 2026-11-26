//! Platform-specific privilege, firmware, BitLocker, and reboot helpers.

use crate::{InstallerError, Result};

/// Returns `true` if the current process has Administrator (Windows) or root (Linux) privileges.
pub fn is_admin() -> bool {
    platform_is_admin()
}

/// Returns `true` if the system booted in UEFI mode.
pub fn is_uefi() -> bool {
    platform_is_uefi()
}

/// Returns `Some(true)` if Secure Boot is enabled, `Some(false)` if disabled, or `None` if unknown.
pub fn check_secure_boot() -> Option<bool> {
    platform_check_secure_boot()
}

/// Returns `Some(true)` if BitLocker is active on the OS drive (Windows only), `None` otherwise.
pub fn check_bitlocker() -> Option<bool> {
    platform_check_bitlocker()
}

/// Triggers an immediate system reboot.
pub fn trigger_reboot() -> Result<()> {
    platform_trigger_reboot()
}

/// Attempts to relaunch the current executable with elevated privileges (UAC prompt on Windows).
pub fn relaunch_as_admin() -> Result<()> {
    platform_relaunch_as_admin()
}

#[cfg(windows)]
fn platform_is_admin() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut core::ffi::c_void),
            core::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

#[cfg(windows)]
fn platform_is_uefi() -> bool {
    use windows::Win32::System::SystemInformation::{
        FirmwareTypeUefi, GetFirmwareType, FIRMWARE_TYPE,
    };

    let mut fw_type = FIRMWARE_TYPE::default();
    unsafe {
        if GetFirmwareType(&mut fw_type).is_ok() {
            return fw_type == FirmwareTypeUefi;
        }
    }
    false
}

#[cfg(windows)]
fn platform_check_secure_boot() -> Option<bool> {
    if let Ok(Some(bytes)) = crate::nvram::get_efi_var("SecureBoot") {
        if let Some(&val) = bytes.first() {
            return Some(val == 1);
        }
    }

    // Fallback for non-elevated sessions: PowerShell `Confirm-SecureBootUEFI`.
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", "Confirm-SecureBootUEFI"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_ascii_lowercase();
    match stdout.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

#[cfg(windows)]
fn platform_check_bitlocker() -> Option<bool> {
    let output = std::process::Command::new("manage-bde")
        .args(["-status", "C:"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("protection status") {
            if lower.contains("protection on") {
                return Some(true);
            }
            if lower.contains("protection off") {
                return Some(false);
            }
        }
    }
    None
}

#[cfg(windows)]
fn platform_trigger_reboot() -> Result<()> {
    let output = std::process::Command::new("shutdown")
        .args(["/r", "/t", "0"])
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(InstallerError::CommandFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[cfg(windows)]
fn platform_relaunch_as_admin() -> Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;

    let exe = std::env::current_exe()?;
    let w_verb: Vec<u16> = OsStr::new("runas").encode_wide().chain(Some(0)).collect();
    let w_exe: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();

    // SW_SHOWNORMAL = 1
    let res = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(w_verb.as_ptr()),
            PCWSTR(w_exe.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    if (res.0 as isize) > 32 {
        Ok(())
    } else {
        Err(InstallerError::PermissionDenied(
            "UAC elevation was cancelled or failed".into(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn platform_is_admin() -> bool {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                let mut parts = rest.split_whitespace();
                let _real = parts.next();
                if let Some(effective) = parts.next() {
                    return effective == "0";
                }
            }
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn platform_is_uefi() -> bool {
    std::path::Path::new("/sys/firmware/efi").is_dir()
}

#[cfg(target_os = "linux")]
fn platform_check_secure_boot() -> Option<bool> {
    let entries = std::fs::read_dir("/sys/firmware/efi/efivars").ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("SecureBoot-") {
            let bytes = std::fs::read(entry.path()).ok()?;
            if let Some(&last) = bytes.last() {
                return Some(last == 1);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn platform_check_bitlocker() -> Option<bool> {
    None
}

#[cfg(target_os = "linux")]
fn platform_trigger_reboot() -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .arg("reboot")
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => {
            let fallback = std::process::Command::new("reboot").status()?;
            if fallback.success() {
                Ok(())
            } else {
                Err(InstallerError::CommandFailed(
                    "failed to trigger system reboot".into(),
                ))
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn platform_relaunch_as_admin() -> Result<()> {
    Err(InstallerError::PermissionDenied(
        "please relaunch with sudo or pkexec".into(),
    ))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn platform_is_admin() -> bool {
    false
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_is_uefi() -> bool {
    false
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_check_secure_boot() -> Option<bool> {
    None
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_check_bitlocker() -> Option<bool> {
    None
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_trigger_reboot() -> Result<()> {
    Err(InstallerError::UnsupportedPlatform)
}
#[cfg(not(any(windows, target_os = "linux")))]
fn platform_relaunch_as_admin() -> Result<()> {
    Err(InstallerError::UnsupportedPlatform)
}
