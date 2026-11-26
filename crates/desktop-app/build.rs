use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");

    let efi_path = workspace_root
        .join("target")
        .join("x86_64-unknown-uefi")
        .join("release")
        .join("modernboot-efi.efi");

    println!("cargo:rerun-if-changed={}", efi_path.display());

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_efi = out_dir.join("modernboot-efi.efi");

    if efi_path.exists() {
        let bytes = fs::read(&efi_path).expect("failed to read modernboot-efi.efi");
        fs::write(&out_efi, &bytes).expect("failed to copy modernboot-efi.efi to OUT_DIR");
        println!("cargo:rustc-env=MODERNBOOT_EFI_EMBEDDED=1");
    } else {
        // Placeholder so `cargo check -p modernboot-app` and `cargo build -p modernboot-app`
        // succeed even before the UEFI target binary is built.
        fs::write(&out_efi, b"").expect("failed to write placeholder modernboot-efi.efi");
        println!("cargo:rustc-env=MODERNBOOT_EFI_EMBEDDED=0");
        println!(
            "cargo:warning=modernboot-efi.efi not found at {}; embedding empty placeholder",
            efi_path.display()
        );
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon_path = workspace_root.join("assets").join("modernboot.ico");
        println!("cargo:rerun-if-changed={}", icon_path.display());
        let mut res = winres::WindowsResource::new();
        if icon_path.exists() {
            res.set_icon(icon_path.to_str().expect("valid utf-8 icon path"));
        }
        res.set("ProductName", "ModernBoot");
        res.set("FileDescription", "ModernBoot UEFI Dual-Boot Manager & Installer");
        res.set_manifest(
            r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="highestAvailable" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>"#,
        );
        res.compile().expect("failed to compile Windows resources");
    }
}
