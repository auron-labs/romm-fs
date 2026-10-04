//! Minimal DTO subset of the verified RomM 5.3.1 contract.
//! Unknown fields are ignored on purpose (serde defaults). Never invent
//! response shapes beyond `.planning/API-CONTRACT.md`.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires: u64,
    pub refresh_token: String,
    pub refresh_expires: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PlatformDto {
    pub id: i64,
    pub slug: String,
    pub fs_slug: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub rom_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RomFileDto {
    pub id: i64,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub last_modified: Option<String>,
    pub crc_hash: Option<String>,
    pub md5_hash: Option<String>,
    pub sha1_hash: Option<String>,
    pub is_top_level: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RomDto {
    pub id: i64,
    pub platform_fs_slug: String,
    pub platform_slug: String,
    pub fs_name: String,
    pub fs_size_bytes: u64,
    pub has_simple_single_file: bool,
    pub has_nested_single_file: bool,
    pub has_multiple_files: bool,
    pub missing_from_fs: bool,
    pub is_physical: bool,
    pub updated_at: String,
    #[serde(default)]
    pub files: Vec<RomFileDto>,
}

#[derive(Debug, Deserialize)]
pub struct RomsPage {
    pub items: Vec<RomDto>,
    pub total: Option<u64>,
    pub limit: u64,
    pub offset: u64,
}
