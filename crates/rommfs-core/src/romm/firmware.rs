use super::RommClient;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FirmwareDto {
    pub id: i64,
    pub platform_id: i64,
    pub file_name: String,
    pub file_size_bytes: u64,
    #[serde(default)]
    pub missing_from_fs: bool,
    #[serde(default)]
    pub updated_at: String,
}
impl RommClient {
    pub fn firmware(&self) -> Result<Vec<FirmwareDto>> {
        self.get_json("/api/firmware")
    }
    pub fn download_firmware(&self, firmware: &FirmwareDto, writer: &mut dyn Write) -> Result<u64> {
        if firmware.id <= 0 || firmware.missing_from_fs {
            return Err(Error::Unsupported(
                "Firmware is missing or has an invalid identity".into(),
            ));
        }
        crate::save_sync::validate_windows_path_component(&firmware.file_name)
            .map_err(Error::Unsupported)?;
        let filename = firmware
            .file_name
            .bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect::<String>();
        self.download_content(
            format!(
                "{}/api/firmware/{}/content/{filename}",
                self.base_url, firmware.id
            ),
            writer,
            &mut |_, _| {},
        )
    }
}
