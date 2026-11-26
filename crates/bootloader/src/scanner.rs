//! Auto-scanner for OS EFI binaries across all `SimpleFileSystem` handles.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use modernboot_shared::{BootConfig, BootEntry, OsKind, KNOWN_OS_PATHS};
use uefi::boot::{self, SearchType};
use uefi::fs::FileSystem;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::{cstr16, CString16, Handle, Identify};

/// A boot menu entry resolved to a concrete UEFI `SimpleFileSystem` handle
/// (or a built-in firmware action).
#[derive(Debug, Clone)]
pub struct ResolvedEntry {
    pub entry: BootEntry,
    pub esp_handle: Option<Handle>,
    pub is_firmware_setup: bool,
}

/// Discovers and merges configured boot entries with auto-scanned OS binaries
/// across all available `SimpleFileSystem` handles.
pub fn discover_entries(config: &BootConfig) -> Vec<ResolvedEntry> {
    let handles: Vec<Handle> = boot::locate_handle_buffer(SearchType::ByProtocol(&SimpleFileSystem::GUID))
        .map(|buf| buf.to_vec())
        .unwrap_or_default();

    let mut result = Vec::new();
    let mut seen_paths: Vec<String> = Vec::new();

    // 1. Process explicitly configured entries (preserving order, skipping hidden).
    for entry in &config.entries {
        seen_paths.push(normalize_efi_path(&entry.efi_path));
        if entry.hidden {
            continue;
        }
        let esp_handle = find_handle_with_file(&handles, &entry.efi_path);
        // Include configured entries even if handle wasn't found yet, or only if found?
        // Including them with esp_handle = None lets the UI show them and report a clean error if missing.
        result.push(ResolvedEntry {
            entry: entry.clone(),
            esp_handle,
            is_firmware_setup: false,
        });
    }

    // 2. Auto-scan all SimpleFileSystem handles for known OS paths and Linux UKIs.
    if config.auto_scan {
        for &handle in &handles {
            let Ok(sfs) = boot::open_protocol_exclusive::<SimpleFileSystem>(handle) else {
                continue;
            };
            let mut fs = FileSystem::new(sfs);

            for known in KNOWN_OS_PATHS {
                let norm = normalize_efi_path(known.efi_path);
                if seen_paths.iter().any(|p| p == &norm) {
                    continue;
                }
                if let Ok(path_cstr) = CString16::try_from(known.efi_path) {
                    if fs.try_exists(path_cstr.as_ref()).unwrap_or(false) {
                        seen_paths.push(norm);
                        let id = format!(
                            "auto-{}-{}",
                            known.os_kind.default_badge().to_ascii_lowercase(),
                            result.len() + 1
                        );
                        result.push(ResolvedEntry {
                            entry: BootEntry {
                                id,
                                label: String::from(known.default_label),
                                os_kind: known.os_kind,
                                efi_path: String::from(known.efi_path),
                                hidden: false,
                                auto_detected: true,
                                icon_override: None,
                            },
                            esp_handle: Some(handle),
                            is_firmware_setup: false,
                        });
                    }
                }
            }

            // Scan \EFI\Linux\*.efi for Unified Kernel Images (UKIs).
            scan_linux_uki_dir(&mut fs, handle, &mut seen_paths, &mut result);
        }
    }

    // 3. Optionally append UEFI Firmware Setup action.
    if config.show_firmware_setup {
        result.push(ResolvedEntry {
            entry: BootEntry {
                id: String::from("firmware-setup"),
                label: String::from("UEFI Firmware Setup"),
                os_kind: OsKind::UefiShell,
                efi_path: String::new(),
                hidden: false,
                auto_detected: true,
                icon_override: Some(String::from("firmware")),
            },
            esp_handle: None,
            is_firmware_setup: true,
        });
    }

    result
}

fn scan_linux_uki_dir(
    fs: &mut FileSystem,
    handle: Handle,
    seen_paths: &mut Vec<String>,
    result: &mut Vec<ResolvedEntry>,
) {
    let Ok(entries) = fs.read_dir(cstr16!("\\EFI\\Linux")) else {
        return;
    };

    for dir_entry in entries.flatten() {
        if dir_entry.is_directory() {
            continue;
        }
        let name = dir_entry.file_name().to_string();
        if !name.to_ascii_lowercase().ends_with(".efi") {
            continue;
        }
        let full_path = format!("\\EFI\\Linux\\{name}");
        let norm = normalize_efi_path(&full_path);
        if seen_paths.iter().any(|p| p == &norm) {
            continue;
        }
        seen_paths.push(norm);

        let stem = name.strip_suffix(".efi").or_else(|| name.strip_suffix(".EFI")).unwrap_or(&name);
        let label = if stem.is_empty() {
            String::from("Linux (UKI)")
        } else {
            format!("Linux ({stem})")
        };

        result.push(ResolvedEntry {
            entry: BootEntry {
                id: format!("auto-uki-{}", result.len() + 1),
                label,
                os_kind: OsKind::Linux,
                efi_path: full_path,
                hidden: false,
                auto_detected: true,
                icon_override: None,
            },
            esp_handle: Some(handle),
            is_firmware_setup: false,
        });
    }
}

fn find_handle_with_file(handles: &[Handle], efi_path: &str) -> Option<Handle> {
    let normalized_win = efi_path.replace('/', "\\");
    let path_cstr = CString16::try_from(normalized_win.as_str()).ok()?;
    for &handle in handles {
        let Ok(sfs) = boot::open_protocol_exclusive::<SimpleFileSystem>(handle) else {
            continue;
        };
        let mut fs = FileSystem::new(sfs);
        if fs.try_exists(path_cstr.as_ref()).unwrap_or(false) {
            return Some(handle);
        }
    }
    None
}

fn normalize_efi_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.trim_start_matches(['\\', '/']).chars() {
        if ch == '/' {
            out.push('\\');
        } else {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}
