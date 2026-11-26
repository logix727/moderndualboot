//! UEFI NVRAM binary structures (`EFI_LOAD_OPTION`, `EFI_DEVICE_PATH_PROTOCOL`, `BootOrder`).

use crate::{InstallerError, Result};

/// Standard UEFI Global Variable GUID string for Windows `Get/SetFirmwareEnvironmentVariableW`.
pub const EFI_GLOBAL_VARIABLE_GUID: &str = "{8be4df61-93ca-11d2-aa0d-00e098032b8c}";

/// Lowercase hex GUID without braces for Linux `/sys/firmware/efi/efivars/<Name>-<Guid>`.
pub const EFI_GLOBAL_VARIABLE_GUID_BARE: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// `LOAD_OPTION_ACTIVE` attribute bit.
pub const LOAD_OPTION_ACTIVE: u32 = 0x0000_0001;

/// `EFI_OS_INDICATIONS_BOOT_TO_FW_UI` bit for `OsIndications`.
pub const EFI_OS_INDICATIONS_BOOT_TO_FW_UI: u64 = 0x0000_0000_0000_0001;

/// Device Path node types.
pub const DP_TYPE_MEDIA: u8 = 0x04;
pub const DP_SUBTYPE_HARD_DRIVE: u8 = 0x01;
pub const DP_SUBTYPE_FILE_PATH: u8 = 0x04;
pub const DP_TYPE_END: u8 = 0x7F;
pub const DP_SUBTYPE_END_ENTIRE: u8 = 0xFF;

/// Parsed representation of a UEFI `BootXXXX` `EFI_LOAD_OPTION` variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirmwareBootEntry {
    /// Numeric boot ID (e.g. `0x0001` for `Boot0001`).
    pub id: u16,
    /// `EFI_LOAD_OPTION.Attributes` bitmask.
    pub attributes: u32,
    /// Human-readable description (UTF-16 decoded).
    pub description: String,
    /// Extracted EFI file path from the Media FilePath device path node, if present.
    pub efi_path: Option<String>,
    /// Raw `FilePathList` bytes.
    pub device_path_raw: Vec<u8>,
    /// Trailing `OptionalData` bytes.
    pub optional_data: Vec<u8>,
    /// Whether `LOAD_OPTION_ACTIVE` (`0x1`) is set.
    pub is_active: bool,
}

impl FirmwareBootEntry {
    /// Returns the UEFI NVRAM variable name (e.g. `"Boot0001"`).
    pub fn boot_var_name(&self) -> String {
        format!("Boot{:04X}", self.id)
    }
}

/// Decoded `EFI_LOAD_OPTION` fields (without the `BootXXXX` ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOption {
    pub attributes: u32,
    pub description: String,
    pub file_path_list: Vec<u8>,
    pub optional_data: Vec<u8>,
}

impl LoadOption {
    /// Parses a raw `BootXXXX` binary blob according to UEFI Spec Section 3.1.3 (`EFI_LOAD_OPTION`).
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 6 {
            return Err(InstallerError::InvalidLoadOption(
                "load option shorter than 6-byte header".into(),
            ));
        }

        let attributes = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let file_path_list_len = u16::from_le_bytes([data[4], data[5]]) as usize;

        // Description is a null-terminated UTF-16LE string starting at offset 6.
        let mut pos = 6;
        let mut utf16_units = Vec::new();
        let mut found_nul = false;
        while pos + 1 < data.len() {
            let unit = u16::from_le_bytes([data[pos], data[pos + 1]]);
            pos += 2;
            if unit == 0 {
                found_nul = true;
                break;
            }
            utf16_units.push(unit);
        }

        if !found_nul {
            return Err(InstallerError::InvalidLoadOption(
                "unterminated UTF-16LE description".into(),
            ));
        }

        let description = String::from_utf16_lossy(&utf16_units);
        if pos + file_path_list_len > data.len() {
            return Err(InstallerError::InvalidLoadOption(format!(
                "file_path_list_len ({file_path_list_len}) exceeds remaining bytes ({})",
                data.len() - pos
            )));
        }

        let file_path_list = data[pos..pos + file_path_list_len].to_vec();
        let optional_data = data[pos + file_path_list_len..].to_vec();

        Ok(Self {
            attributes,
            description,
            file_path_list,
            optional_data,
        })
    }

    /// Serializes this `LoadOption` into UEFI `EFI_LOAD_OPTION` binary bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.attributes.to_le_bytes());
        let fp_len = u16::try_from(self.file_path_list.len()).unwrap_or(u16::MAX);
        out.extend_from_slice(&fp_len.to_le_bytes());

        for unit in self.description.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());

        out.extend_from_slice(&self.file_path_list[..fp_len as usize]);
        out.extend_from_slice(&self.optional_data);
        out
    }

    /// Converts this `LoadOption` into a [`FirmwareBootEntry`] with the given `id`.
    pub fn into_entry(self, id: u16) -> FirmwareBootEntry {
        let efi_path = extract_file_path(&self.file_path_list);
        let is_active = (self.attributes & LOAD_OPTION_ACTIVE) != 0;
        FirmwareBootEntry {
            id,
            attributes: self.attributes,
            description: self.description,
            efi_path,
            device_path_raw: self.file_path_list,
            optional_data: self.optional_data,
            is_active,
        }
    }
}

/// Iterates over `EFI_DEVICE_PATH_PROTOCOL` nodes in a `FilePathList` and returns the
/// first Media FilePath (`Type = 0x04`, `SubType = 0x04`) string, if any.
pub fn extract_file_path(device_path: &[u8]) -> Option<String> {
    let mut offset = 0;
    while offset + 4 <= device_path.len() {
        let dp_type = device_path[offset];
        let dp_subtype = device_path[offset + 1];
        let len = u16::from_le_bytes([device_path[offset + 2], device_path[offset + 3]]) as usize;
        if len < 4 || offset + len > device_path.len() {
            break;
        }
        if dp_type == DP_TYPE_END && dp_subtype == DP_SUBTYPE_END_ENTIRE {
            break;
        }
        if dp_type == DP_TYPE_MEDIA && dp_subtype == DP_SUBTYPE_FILE_PATH {
            let payload = &device_path[offset + 4..offset + len];
            let mut units = Vec::with_capacity(payload.len() / 2);
            for chunk in payload.chunks_exact(2) {
                let u = u16::from_le_bytes([chunk[0], chunk[1]]);
                if u == 0 {
                    break;
                }
                units.push(u);
            }
            return Some(String::from_utf16_lossy(&units));
        }
        offset += len;
    }
    None
}

/// Extracts the raw `HardDrive` (`Type = 0x04, SubType = 0x01`) device path node if present.
pub fn extract_hard_drive_node(device_path: &[u8]) -> Option<Vec<u8>> {
    let mut offset = 0;
    while offset + 4 <= device_path.len() {
        let dp_type = device_path[offset];
        let dp_subtype = device_path[offset + 1];
        let len = u16::from_le_bytes([device_path[offset + 2], device_path[offset + 3]]) as usize;
        if len < 4 || offset + len > device_path.len() {
            break;
        }
        if dp_type == DP_TYPE_END && dp_subtype == DP_SUBTYPE_END_ENTIRE {
            break;
        }
        if dp_type == DP_TYPE_MEDIA && dp_subtype == DP_SUBTYPE_HARD_DRIVE {
            return Some(device_path[offset..offset + len].to_vec());
        }
        offset += len;
    }
    None
}

/// Builds a Media FilePath device path node (`Type = 0x04, SubType = 0x04`).
pub fn build_file_path_node(efi_path: &str) -> Vec<u8> {
    let normalized = efi_path.replace('/', "\\");
    let mut payload = Vec::new();
    for unit in normalized.encode_utf16() {
        payload.extend_from_slice(&unit.to_le_bytes());
    }
    payload.extend_from_slice(&0u16.to_le_bytes());

    let total_len = (4 + payload.len()) as u16;
    let mut node = Vec::with_capacity(total_len as usize);
    node.push(DP_TYPE_MEDIA);
    node.push(DP_SUBTYPE_FILE_PATH);
    node.extend_from_slice(&total_len.to_le_bytes());
    node.extend_from_slice(&payload);
    node
}

/// Builds an End-of-Entire-Device-Path node (`Type = 0x7F, SubType = 0xFF, Length = 4`).
pub fn build_end_device_path_node() -> [u8; 4] {
    [DP_TYPE_END, DP_SUBTYPE_END_ENTIRE, 0x04, 0x00]
}

/// Builds a complete `FilePathList` from an optional `HardDrive` node and an EFI file path.
pub fn build_device_path(hard_drive_node: Option<&[u8]>, efi_path: &str) -> Vec<u8> {
    let mut list = Vec::new();
    if let Some(hd) = hard_drive_node {
        list.extend_from_slice(hd);
    }
    list.extend_from_slice(&build_file_path_node(efi_path));
    list.extend_from_slice(&build_end_device_path_node());
    list
}

/// Parses UEFI `BootOrder` variable bytes into a list of `u16` boot IDs.
pub fn parse_boot_order(bytes: &[u8]) -> Vec<u16> {
    bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

/// Encodes a slice of `u16` boot IDs into UEFI `BootOrder` variable bytes.
pub fn encode_boot_order(ids: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(ids.len() * 2);
    for id in ids {
        out.extend_from_slice(&id.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_option_round_trip_with_hard_drive_and_file_path() {
        // Synthetic 42-byte GPT HardDrive node (Type 0x04, SubType 0x01, Len 42)
        let mut hd_node = vec![DP_TYPE_MEDIA, DP_SUBTYPE_HARD_DRIVE, 42, 0];
        hd_node.resize(42, 0xAB);

        let file_path_list = build_device_path(Some(&hd_node), "\\EFI\\ModernBoot\\bootx64.efi");
        let opt = LoadOption {
            attributes: LOAD_OPTION_ACTIVE,
            description: "ModernBoot".to_string(),
            file_path_list: file_path_list.clone(),
            optional_data: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };

        let encoded = opt.to_bytes();
        let decoded = LoadOption::from_bytes(&encoded).expect("decode LoadOption");
        assert_eq!(decoded, opt);

        let entry = decoded.into_entry(0x0007);
        assert_eq!(entry.id, 0x0007);
        assert_eq!(entry.boot_var_name(), "Boot0007");
        assert!(entry.is_active);
        assert_eq!(entry.description, "ModernBoot");
        assert_eq!(
            entry.efi_path.as_deref(),
            Some("\\EFI\\ModernBoot\\bootx64.efi")
        );
        assert_eq!(
            extract_hard_drive_node(&entry.device_path_raw).as_deref(),
            Some(hd_node.as_slice())
        );
        assert_eq!(entry.optional_data, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn load_option_rejects_truncated_inputs() {
        assert!(LoadOption::from_bytes(&[1, 0, 0]).is_err());

        // Missing null terminator in description
        let bad_desc = [1, 0, 0, 0, 4, 0, b'A', 0];
        assert!(LoadOption::from_bytes(&bad_desc).is_err());

        // FilePathListLength exceeds remaining bytes
        let bad_len = [1, 0, 0, 0, 100, 0, b'A', 0, 0, 0];
        assert!(LoadOption::from_bytes(&bad_len).is_err());
    }

    #[test]
    fn boot_order_round_trip() {
        let ids = vec![0x0003, 0x0000, 0x0001, 0x000A];
        let raw = encode_boot_order(&ids);
        assert_eq!(raw, vec![0x03, 0x00, 0x00, 0x00, 0x01, 0x00, 0x0A, 0x00]);
        assert_eq!(parse_boot_order(&raw), ids);
    }

    #[test]
    fn file_path_normalizes_forward_slashes() {
        let dp = build_device_path(None, "/EFI/ubuntu/shimx64.efi");
        assert_eq!(
            extract_file_path(&dp).as_deref(),
            Some("\\EFI\\ubuntu\\shimx64.efi")
        );
    }
}
