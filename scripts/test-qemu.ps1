<#
.SYNOPSIS
    Safely boots modernboot-efi.efi inside a QEMU + OVMF virtual machine
    using a temporary virtual FAT32 ESP folder (never touches your real disk).
#>
$ErrorActionPreference = "Stop"

$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$EfiBin = Join-Path $Root "target\x86_64-unknown-uefi\release\modernboot-efi.efi"

if (-not (Test-Path $EfiBin)) {
    Write-Host "Building modernboot-efi.efi..." -ForegroundColor Cyan
    Push-Location $Root
    cargo build -p modernboot-efi --target x86_64-unknown-uefi --release
    Pop-Location
}

$Qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
if (-not (Test-Path $Qemu)) {
    $cmd = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($cmd) { $Qemu = $cmd.Source }
    else {
        Write-Error "QEMU not found. Install it with: winget install SoftwareFreedomConservancy.QEMU"
        exit 1
    }
}

$OvmfCode = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
if (-not (Test-Path $OvmfCode)) {
    Write-Error "OVMF firmware not found at $OvmfCode"
    exit 1
}

$EspDir = Join-Path $Root "target\qemu-esp"
$BootDir = Join-Path $EspDir "EFI\BOOT"
$MbDir = Join-Path $EspDir "EFI\ModernBoot"
New-Item -ItemType Directory -Force -Path $BootDir, $MbDir | Out-Null
Copy-Item $EfiBin (Join-Path $BootDir "BOOTX64.EFI") -Force
Copy-Item $EfiBin (Join-Path $MbDir "bootx64.efi") -Force

Write-Host "Launching QEMU OVMF with virtual ESP at $EspDir..." -ForegroundColor Green
& $Qemu `
    -machine q35 `
    -m 512M `
    -drive "if=pflash,format=raw,readonly=on,file=$OvmfCode" `
    -drive "format=raw,file=fat:rw:$EspDir" `
    -net none
