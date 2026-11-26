# ModernBoot

A modern, unified **Rust** UEFI dual-boot manager and one-click Windows/Linux desktop installer.

## Architecture

ModernBoot is split into four workspace crates:

- **`crates/shared` (`modernboot-shared`)**: `#![no_std]` + `alloc` config, theme presets (`Midnight`, `Nord`, `CatppuccinMocha`, `MinimalLight`), and OS EFI path signatures shared by both the UEFI bootloader and the desktop app.
- **`crates/bootloader` (`modernboot-efi`)**: Native `x86_64-unknown-uefi` pre-boot GUI manager using `uefi-rs`. Renders a double-buffered GOP framebuffer card UI with HiDPI scaling, keyboard + mouse input, multi-ESP OS auto-discovery, countdown timer, and `LoadImage`/`StartImage` chainloading.
- **`crates/installer-core` (`modernboot-installer`)**: Cross-platform (Windows & Linux) library for EFI System Partition (ESP) discovery/mounting, UEFI NVRAM (`BootOrder`, `BootXXXX`, `BootNext`) management, Secure Boot & BitLocker health checks, and one-click install/uninstall/repair.
- **`crates/desktop-app` (`modernboot-app`)**: Standalone `eframe`/`egui` desktop app for Windows and Linux that embeds `modernboot-efi.efi`, provides one-click installation, OS entry editing, NVRAM inspection, and a live interactive boot menu preview.

## Building

### Prerequisites
```bash
rustup target add x86_64-unknown-uefi
```

### Build UEFI Bootloader & Desktop App
```bash
# 1. Run unit tests
cargo test -p modernboot-shared -p modernboot-installer

# 2. Build the UEFI bootloader (.efi)
cargo build -p modernboot-efi --target x86_64-unknown-uefi --release

# 3. Build the desktop app (automatically embeds modernboot-efi.efi)
cargo build -p modernboot-app --release
```

## Notes on Secure Boot & BitLocker
- **Secure Boot**: Modern Windows 11 PCs ship with Secure Boot enabled. Unsigned `.efi` binaries require either disabling Secure Boot in UEFI firmware or signing/enrolling via `shim` + MOK. ModernBoot's desktop app detects Secure Boot status and guides you before modifying boot order.
- **BitLocker**: Changing UEFI boot order or Secure Boot settings can trigger BitLocker recovery if PCR measurements change. ModernBoot checks BitLocker status on Windows and warns you beforehand.

## License
Dual-licensed under MIT or Apache-2.0.
