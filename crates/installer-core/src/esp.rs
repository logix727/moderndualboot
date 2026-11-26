//! EFI System Partition (ESP) discovery, RAII mounting, and OS scanning.

use crate::{InstallerError, Result};
use modernboot_shared::{BootEntry, OsKind, KNOWN_OS_PATHS};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// RAII guard representing a mounted EFI System Partition.
///
/// On Windows, if `EspMount` temporarily mounted the ESP via `mountvol X: /S`,
/// dropping `EspMount` automatically unmounts it via `mountvol X: /D`.
#[derive(Debug)]
pub struct EspMount {
    root: PathBuf,
    mounted_by_us: bool,
    #[cfg(windows)]
    drive_letter: Option<char>,
}

impl EspMount {
    /// Discovers and (on Windows, if needed) temporarily mounts the EFI System Partition.
    pub fn acquire() -> Result<Self> {
        acquire_platform_esp()
    }

    /// Returns the filesystem root path of the mounted ESP (e.g. `Z:\` or `/boot/efi`).
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns a human-readable string representation of the ESP root path.
    pub fn root_string(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }

    /// Resolves a UEFI-style path (e.g. `\EFI\ModernBoot\bootx64.efi`) relative to the ESP root.
    pub fn resolve_efi_path(&self, uefi_path: &str) -> PathBuf {
        let mut out = self.root.clone();
        for component in uefi_path.split(['\\', '/']).filter(|s| !s.is_empty()) {
            out.push(component);
        }
        out
    }

    /// Resolves a UEFI-style path case-insensitively on the mounted ESP.
    pub fn find_efi_file_ci(&self, uefi_path: &str) -> Option<PathBuf> {
        let exact = self.resolve_efi_path(uefi_path);
        if exact.exists() {
            return Some(exact);
        }

        let mut current = self.root.clone();
        for component in uefi_path.split(['\\', '/']).filter(|s| !s.is_empty()) {
            let entries = std::fs::read_dir(&current).ok()?;
            let mut matched = None;
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(component)
                {
                    matched = Some(entry.path());
                    break;
                }
            }
            current = matched?;
        }
        Some(current)
    }

    /// Scans the mounted ESP for known OS bootloaders (`KNOWN_OS_PATHS`) and Linux UKIs
    /// (`\EFI\Linux\*.efi`).
    pub fn scan_oses(&self) -> Vec<BootEntry> {
        let mut entries = Vec::new();
        let mut seen_paths = HashSet::new();
        let mut used_ids = HashSet::new();

        for known in KNOWN_OS_PATHS {
            if self.find_efi_file_ci(known.efi_path).is_some() {
                let norm = normalize_uefi_path(known.efi_path);
                if seen_paths.insert(norm) {
                    let base_id = default_id_for_kind(known.os_kind);
                    let id = make_unique_id(base_id, &mut used_ids);
                    entries.push(BootEntry {
                        id,
                        label: known.default_label.to_string(),
                        os_kind: known.os_kind,
                        efi_path: known.efi_path.to_string(),
                        hidden: false,
                        auto_detected: true,
                        icon_override: None,
                    });
                }
            }
        }

        // Also scan \EFI\Linux\*.efi for systemd-boot / dracut Unified Kernel Images.
        if let Some(linux_dir) = self.find_efi_file_ci("\\EFI\\Linux") {
            if let Ok(read_dir) = std::fs::read_dir(linux_dir) {
                for item in read_dir.flatten() {
                    let path = item.path();
                    let is_efi = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.eq_ignore_ascii_case("efi"))
                        .unwrap_or(false);
                    if !is_efi {
                        continue;
                    }
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        let uefi_path = format!("\\EFI\\Linux\\{file_name}");
                        let norm = normalize_uefi_path(&uefi_path);
                        if seen_paths.insert(norm) {
                            let id = make_unique_id("linux-uki", &mut used_ids);
                            let stem = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("Linux UKI");
                            entries.push(BootEntry {
                                id,
                                label: format!("Linux ({stem})"),
                                os_kind: OsKind::Linux,
                                efi_path: uefi_path,
                                hidden: false,
                                auto_detected: true,
                                icon_override: None,
                            });
                        }
                    }
                }
            }
        }

        entries
    }
}

impl Drop for EspMount {
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.mounted_by_us {
            if let Some(letter) = self.drive_letter {
                let target = format!("{letter}:");
                let _ = std::process::Command::new("mountvol")
                    .args([&target, "/D"])
                    .output();
            }
        }
    }
}

fn normalize_uefi_path(path: &str) -> String {
    path.replace('/', "\\").to_ascii_lowercase()
}

fn default_id_for_kind(kind: OsKind) -> &'static str {
    match kind {
        OsKind::Windows => "windows",
        OsKind::Linux => "linux",
        OsKind::Ubuntu => "ubuntu",
        OsKind::Fedora => "fedora",
        OsKind::Arch => "arch",
        OsKind::Debian => "debian",
        OsKind::SteamOS => "steamos",
        OsKind::MacOS => "macos",
        OsKind::UefiShell => "uefi-shell",
        OsKind::Unknown => "other",
    }
}

fn make_unique_id(base: &str, used: &mut HashSet<String>) -> String {
    if used.insert(base.to_string()) {
        return base.to_string();
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}-{idx}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        idx += 1;
    }
}

#[cfg(windows)]
fn acquire_platform_esp() -> Result<EspMount> {
    use windows::Win32::Storage::FileSystem::GetLogicalDrives;

    let mask = unsafe { GetLogicalDrives() };

    // First try mounting the system ESP to an unused drive letter via `mountvol X: /S`.
    if let Some(letter) = find_unused_drive_letter(mask) {
        let target = format!("{letter}:");
        let output = std::process::Command::new("mountvol")
            .args([&target, "/S"])
            .output()
            .map_err(|e| InstallerError::EspNotFound(format!("failed to run mountvol: {e}")))?;

        if output.status.success() {
            return Ok(EspMount {
                root: PathBuf::from(format!("{letter}:\\")),
                mounted_by_us: true,
                drive_letter: Some(letter),
            });
        }
    }

    // Fallback: check if an existing mounted drive already exposes `\EFI`.
    for bit in 0..26u32 {
        if (mask & (1 << bit)) != 0 {
            let letter = (b'A' + bit as u8) as char;
            let root = PathBuf::from(format!("{letter}:\\"));
            if root.join("EFI").join("Microsoft").exists()
                || root.join("EFI").join("ModernBoot").exists()
                || root.join("EFI").join("Boot").exists()
            {
                return Ok(EspMount {
                    root,
                    mounted_by_us: false,
                    drive_letter: None,
                });
            }
        }
    }

    Err(InstallerError::EspNotFound(
        "could not mount or locate EFI System Partition (Administrator privileges required)"
            .into(),
    ))
}

#[cfg(windows)]
fn find_unused_drive_letter(drive_mask: u32) -> Option<char> {
    // Prefer Z..=M so we don't collide with common local/network letters.
    for letter in ('M'..='Z').rev() {
        let bit = (letter as u32) - ('A' as u32);
        if (drive_mask & (1 << bit)) == 0 {
            return Some(letter);
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn acquire_platform_esp() -> Result<EspMount> {
    for candidate in ["/boot/efi", "/efi", "/boot"] {
        let root = PathBuf::from(candidate);
        if root.join("EFI").is_dir() || root.join("efi").is_dir() {
            return Ok(EspMount {
                root,
                mounted_by_us: false,
            });
        }
    }

    Err(InstallerError::EspNotFound(
        "no EFI System Partition found at /boot/efi, /efi, or /boot".into(),
    ))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn acquire_platform_esp() -> Result<EspMount> {
    Err(InstallerError::UnsupportedPlatform)
}
