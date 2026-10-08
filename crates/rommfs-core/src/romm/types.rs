//! Minimal DTO subset of the verified RomM 5.3.1 contract.
//! Unknown fields are ignored on purpose (serde defaults). Never invent
//! response shapes beyond `.planning/API-CONTRACT.md`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires: u64,
    pub refresh_token: String,
    pub refresh_expires: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlatformDto {
    pub id: i64,
    pub slug: String,
    pub fs_slug: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub rom_count: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RomFileDto {
    pub id: i64,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub last_modified: Option<String>,
    pub crc_hash: Option<String>,
    pub md5_hash: Option<String>,
    pub sha1_hash: Option<String>,
    pub is_top_level: bool,
    #[serde(default)]
    pub file_path: String,
    #[serde(default)]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
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
    pub fs_path: String,
    #[serde(flatten)]
    pub metadata: RomMetadata,
    #[serde(default)]
    pub files: Vec<RomFileDto>,
}

/// Metadata supplied with the paginated catalogue; no per-game detail fetch.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RomMetadata {
    pub name: Option<String>,
    pub summary: Option<String>,
    pub title_id: Option<String>,
    pub path_cover_small: Option<String>,
    pub path_cover_large: Option<String>,
    pub url_cover: Option<String>,
    #[serde(default)]
    pub merged_screenshots: Vec<String>,
    #[serde(default)]
    pub metadatum: Option<MetadataFields>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MetadataFields {
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub developers: Vec<String>,
    pub player_count: Option<String>,
    pub first_release_date: Option<i64>,
    pub average_rating: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct RomsPage {
    pub items: Vec<RomDto>,
    pub total: Option<u64>,
    pub limit: u64,
    pub offset: u64,
}
