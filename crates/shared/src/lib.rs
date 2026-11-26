//! Shared configuration, theme, and OS detection schema for ModernDualBoot.
//!
//! This crate is `#![no_std]` + `alloc` so it can be used identically by both
//! the UEFI bootloader (`x86_64-unknown-uefi`) and the `std` desktop/installer
//! crates.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Current schema version for [`BootConfig`].
pub const CONFIG_VERSION: u32 = 1;

/// Default location of the ModernDualBoot configuration file on the EFI System Partition.
pub const DEFAULT_CONFIG_PATH: &str = "\\EFI\\ModernBoot\\config.json";

/// Top-level ModernDualBoot configuration persisted on the EFI System Partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BootConfig {
    /// Configuration schema version.
    pub version: u32,
    /// Countdown timeout in seconds before booting the default entry (`0` = wait indefinitely).
    pub timeout_seconds: u32,
    /// ID of the [`BootEntry`] selected by default.
    pub default_entry_id: Option<String>,
    /// Whether the bootloader should auto-scan the ESP for known OS EFI binaries.
    pub auto_scan: bool,
    /// Visual theme configuration.
    pub theme: ThemeConfig,
    /// Configured and/or discovered boot entries.
    pub entries: Vec<BootEntry>,
    /// Whether to show a "UEFI Firmware Settings" action in the boot menu.
    pub show_firmware_setup: bool,
}

impl Default for BootConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            timeout_seconds: 5,
            default_entry_id: None,
            auto_scan: true,
            theme: ThemeConfig::default(),
            entries: Vec::new(),
            show_firmware_setup: true,
        }
    }
}

impl BootConfig {
    /// Serializes this configuration to a pretty-printed JSON string.
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Deserializes a [`BootConfig`] from a JSON string.
    pub fn from_json_str(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }
}

/// A single bootable OS or utility entry shown in the boot menu.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootEntry {
    /// Unique stable identifier for this entry (e.g. `"windows"`, `"ubuntu"`).
    pub id: String,
    /// Human-readable label displayed in the boot menu card.
    pub label: String,
    /// Operating system family used for icons, badges, and styling.
    pub os_kind: OsKind,
    /// UEFI-style path to the EFI executable (e.g. `\\EFI\\Microsoft\\Boot\\bootmgfw.efi`).
    pub efi_path: String,
    /// Whether this entry is hidden from the boot menu.
    #[serde(default)]
    pub hidden: bool,
    /// Whether this entry was added automatically by ESP scanning.
    #[serde(default)]
    pub auto_detected: bool,
    /// Optional custom icon identifier or path overriding `os_kind`.
    #[serde(default)]
    pub icon_override: Option<String>,
}

/// Known operating system and utility categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OsKind {
    Windows,
    Linux,
    Ubuntu,
    Fedora,
    Arch,
    Debian,
    SteamOS,
    MacOS,
    UefiShell,
    #[default]
    Unknown,
}

impl OsKind {
    /// Returns a human-readable display name for this OS kind.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::Ubuntu => "Ubuntu",
            Self::Fedora => "Fedora",
            Self::Arch => "Arch Linux",
            Self::Debian => "Debian",
            Self::SteamOS => "SteamOS",
            Self::MacOS => "macOS",
            Self::UefiShell => "UEFI Shell",
            Self::Unknown => "Unknown OS",
        }
    }

    /// Returns a short uppercase badge label for compact UI rendering.
    pub const fn default_badge(&self) -> &'static str {
        match self {
            Self::Windows => "WIN",
            Self::Linux => "LNX",
            Self::Ubuntu => "UBU",
            Self::Fedora => "FED",
            Self::Arch => "ARC",
            Self::Debian => "DEB",
            Self::SteamOS => "STM",
            Self::MacOS => "MAC",
            Self::UefiShell => "EFI",
            Self::Unknown => "OS",
        }
    }
}

/// Built-in visual theme presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreset {
    #[default]
    Midnight,
    Nord,
    CatppuccinMocha,
    MinimalLight,
}

impl ThemePreset {
    /// All available built-in theme presets.
    pub const ALL: &'static [ThemePreset] = &[
        Self::Midnight,
        Self::Nord,
        Self::CatppuccinMocha,
        Self::MinimalLight,
    ];

    /// Returns the human-readable name of this theme preset.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Midnight => "Midnight",
            Self::Nord => "Nord",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::MinimalLight => "Minimal Light",
        }
    }

    /// Returns the RGB color palette for this preset.
    pub const fn colors(&self) -> ThemeColors {
        match self {
            Self::Midnight => ThemeColors {
                bg_top: [11, 15, 25],
                bg_bottom: [18, 25, 42],
                card_bg: [22, 30, 48],
                card_selected_bg: [32, 46, 74],
                card_border: [48, 64, 96],
                accent: [96, 165, 250],
                text_primary: [241, 245, 249],
                text_secondary: [148, 163, 184],
                badge_bg: [38, 56, 88],
            },
            Self::Nord => ThemeColors {
                bg_top: [46, 52, 64],
                bg_bottom: [36, 41, 51],
                card_bg: [59, 66, 82],
                card_selected_bg: [67, 76, 94],
                card_border: [76, 86, 106],
                accent: [136, 192, 208],
                text_primary: [236, 239, 244],
                text_secondary: [216, 222, 233],
                badge_bg: [76, 86, 106],
            },
            Self::CatppuccinMocha => ThemeColors {
                bg_top: [24, 24, 37],
                bg_bottom: [30, 30, 46],
                card_bg: [49, 50, 68],
                card_selected_bg: [69, 71, 90],
                card_border: [88, 91, 112],
                accent: [180, 190, 254],
                text_primary: [205, 214, 244],
                text_secondary: [166, 173, 200],
                badge_bg: [69, 71, 90],
            },
            Self::MinimalLight => ThemeColors {
                bg_top: [248, 250, 252],
                bg_bottom: [241, 245, 249],
                card_bg: [255, 255, 255],
                card_selected_bg: [226, 232, 240],
                card_border: [203, 213, 225],
                accent: [37, 99, 235],
                text_primary: [15, 23, 42],
                text_secondary: [100, 116, 139],
                badge_bg: [226, 232, 240],
            },
        }
    }
}

/// RGB (`[R, G, B]`) color palette used to render the boot menu UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeColors {
    pub bg_top: [u8; 3],
    pub bg_bottom: [u8; 3],
    pub card_bg: [u8; 3],
    pub card_selected_bg: [u8; 3],
    pub card_border: [u8; 3],
    pub accent: [u8; 3],
    pub text_primary: [u8; 3],
    pub text_secondary: [u8; 3],
    pub badge_bg: [u8; 3],
}

/// User-customizable theme settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Base color preset.
    pub preset: ThemePreset,
    /// Optional `[R, G, B]` accent color overriding the preset's `accent`.
    pub custom_accent: Option<[u8; 3]>,
    /// Whether to display the countdown timer / clock in the boot menu header.
    pub show_clock_or_timer: bool,
    /// Corner radius in pixels for boot entry cards.
    pub corner_radius: u16,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: ThemePreset::Midnight,
            custom_accent: None,
            show_clock_or_timer: true,
            corner_radius: 12,
        }
    }
}

impl ThemeConfig {
    /// Returns the resolved [`ThemeColors`] with any `custom_accent` applied.
    pub const fn effective_colors(&self) -> ThemeColors {
        let mut colors = self.preset.colors();
        if let Some(accent) = self.custom_accent {
            colors.accent = accent;
        }
        colors
    }
}

/// A well-known EFI binary path signature used to identify installed OS bootloaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownOsPath {
    /// Standard UEFI path relative to the ESP root (using `\` separators).
    pub efi_path: &'static str,
    /// Associated OS category.
    pub os_kind: OsKind,
    /// Default display label when auto-detected.
    pub default_label: &'static str,
}

/// Built-in table of known OS EFI binary paths on the EFI System Partition.
pub const KNOWN_OS_PATHS: &[KnownOsPath] = &[
    KnownOsPath {
        efi_path: "\\EFI\\Microsoft\\Boot\\bootmgfw.efi",
        os_kind: OsKind::Windows,
        default_label: "Windows Boot Manager",
    },
    KnownOsPath {
        efi_path: "\\EFI\\ubuntu\\shimx64.efi",
        os_kind: OsKind::Ubuntu,
        default_label: "Ubuntu",
    },
    KnownOsPath {
        efi_path: "\\EFI\\ubuntu\\grubx64.efi",
        os_kind: OsKind::Ubuntu,
        default_label: "Ubuntu (GRUB)",
    },
    KnownOsPath {
        efi_path: "\\EFI\\fedora\\shimx64.efi",
        os_kind: OsKind::Fedora,
        default_label: "Fedora",
    },
    KnownOsPath {
        efi_path: "\\EFI\\fedora\\grubx64.efi",
        os_kind: OsKind::Fedora,
        default_label: "Fedora (GRUB)",
    },
    KnownOsPath {
        efi_path: "\\EFI\\arch\\grubx64.efi",
        os_kind: OsKind::Arch,
        default_label: "Arch Linux",
    },
    KnownOsPath {
        efi_path: "\\EFI\\debian\\shimx64.efi",
        os_kind: OsKind::Debian,
        default_label: "Debian",
    },
    KnownOsPath {
        efi_path: "\\EFI\\debian\\grubx64.efi",
        os_kind: OsKind::Debian,
        default_label: "Debian (GRUB)",
    },
    KnownOsPath {
        efi_path: "\\EFI\\steamos\\steamcl.efi",
        os_kind: OsKind::SteamOS,
        default_label: "SteamOS",
    },
    KnownOsPath {
        efi_path: "\\EFI\\systemd\\systemd-bootx64.efi",
        os_kind: OsKind::Linux,
        default_label: "Linux (systemd-boot)",
    },
    KnownOsPath {
        efi_path: "\\EFI\\refind\\refind_x64.efi",
        os_kind: OsKind::Unknown,
        default_label: "rEFInd Boot Manager",
    },
    KnownOsPath {
        efi_path: "\\EFI\\Shell\\Shell.efi",
        os_kind: OsKind::UefiShell,
        default_label: "UEFI Shell",
    },
];

/// Matches a candidate EFI path against [`KNOWN_OS_PATHS`] case-insensitively,
/// treating `/` and `\` as equivalent.
pub fn identify_efi_path(path: &str) -> Option<&'static KnownOsPath> {
    let trimmed = path.trim_start_matches(['\\', '/']);
    KNOWN_OS_PATHS.iter().find(|known| {
        let known_trimmed = known.efi_path.trim_start_matches(['\\', '/']);
        efi_paths_eq_ignore_ascii_case(trimmed, known_trimmed)
    })
}

fn efi_paths_eq_ignore_ascii_case(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).all(|(ca, cb)| {
        let na = if ca == b'/' { b'\\' } else { ca.to_ascii_lowercase() };
        let nb = if cb == b'/' { b'\\' } else { cb.to_ascii_lowercase() };
        na == nb
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn boot_config_json_round_trip() {
        let config = BootConfig {
            version: CONFIG_VERSION,
            timeout_seconds: 8,
            default_entry_id: Some(String::from("windows")),
            auto_scan: true,
            theme: ThemeConfig {
                preset: ThemePreset::CatppuccinMocha,
                custom_accent: Some([255, 128, 64]),
                show_clock_or_timer: false,
                corner_radius: 16,
            },
            entries: vec![
                BootEntry {
                    id: String::from("windows"),
                    label: String::from("Windows 11"),
                    os_kind: OsKind::Windows,
                    efi_path: String::from("\\EFI\\Microsoft\\Boot\\bootmgfw.efi"),
                    hidden: false,
                    auto_detected: true,
                    icon_override: None,
                },
                BootEntry {
                    id: String::from("arch"),
                    label: String::from("Arch Linux"),
                    os_kind: OsKind::Arch,
                    efi_path: String::from("\\EFI\\arch\\grubx64.efi"),
                    hidden: false,
                    auto_detected: false,
                    icon_override: Some(String::from("arch-custom")),
                },
            ],
            show_firmware_setup: true,
        };

        let json = config.to_json_pretty().expect("serialize BootConfig");
        let decoded = BootConfig::from_json_str(&json).expect("deserialize BootConfig");
        assert_eq!(config, decoded);
    }

    #[test]
    fn boot_config_partial_json_uses_defaults() {
        let json = r#"{ "timeout_seconds": 10 }"#;
        let decoded = BootConfig::from_json_str(json).expect("deserialize partial BootConfig");
        assert_eq!(decoded.version, CONFIG_VERSION);
        assert_eq!(decoded.timeout_seconds, 10);
        assert!(decoded.auto_scan);
        assert!(decoded.show_firmware_setup);
        assert_eq!(decoded.theme, ThemeConfig::default());
        assert!(decoded.entries.is_empty());
    }

    #[test]
    fn theme_presets_and_custom_accent() {
        for preset in ThemePreset::ALL {
            let colors = preset.colors();
            assert_ne!(colors.bg_top, colors.text_primary);
            assert!(!preset.display_name().is_empty());
        }

        let cfg = ThemeConfig {
            preset: ThemePreset::Nord,
            custom_accent: Some([10, 20, 30]),
            ..ThemeConfig::default()
        };
        let resolved = cfg.effective_colors();
        assert_eq!(resolved.accent, [10, 20, 30]);
        assert_eq!(resolved.bg_top, ThemePreset::Nord.colors().bg_top);
    }

    #[test]
    fn os_kind_helpers_and_known_paths() {
        assert_eq!(OsKind::Windows.display_name(), "Windows");
        assert_eq!(OsKind::Windows.default_badge(), "WIN");
        assert_eq!(OsKind::SteamOS.default_badge(), "STM");

        let win = identify_efi_path("/efi/microsoft/boot/BOOTMGFW.EFI")
            .expect("should match Windows bootmgfw.efi");
        assert_eq!(win.os_kind, OsKind::Windows);

        let steamos = identify_efi_path("\\EFI\\steamos\\steamcl.efi")
            .expect("should match SteamOS");
        assert_eq!(steamos.os_kind, OsKind::SteamOS);

        assert!(identify_efi_path("\\EFI\\custom\\unknown.efi").is_none());
    }
}
