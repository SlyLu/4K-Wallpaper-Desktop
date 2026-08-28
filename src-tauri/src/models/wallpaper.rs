use serde::{Deserialize, Serialize};

use crate::image_processing::ProcessedImage;

/// Provider-neutral metadata persisted by the local catalog.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperRecord {
    pub id: i64,
    pub provider: String,
    pub remote_id: String,
    pub name: String,
    pub source_page_url: Option<String>,
    pub original_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_local_path: Option<String>,
    pub local_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub category: String,
    pub purity: String,
    pub hash: Option<String>,
    pub download_status: String,
    pub favorite: bool,
    pub blacklisted: bool,
    pub preset: bool,
    pub created_at: Option<String>,
    pub synced_at: String,
    pub downloaded_at: Option<String>,
    pub last_used_at: Option<String>,
    /// Separates remote-only metadata from available, offline, and missing files.
    pub file_availability: String,
    /// Identifies whether the preferred file is user-owned or application-managed.
    pub storage_kind: String,
    /// Counts all indexed paths for duplicate-copy management.
    pub file_copy_count: u32,
    pub tags: Vec<String>,
}

/// Data accepted by the catalog upsert boundary before SQLite assigns an id.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewWallpaper {
    pub provider: String,
    pub remote_id: String,
    pub name: String,
    pub source_page_url: Option<String>,
    pub original_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub thumbnail_local_path: Option<String>,
    pub local_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub category: String,
    pub purity: String,
    pub hash: Option<String>,
    pub perceptual_hash: Option<String>,
    pub download_status: String,
    pub preset: bool,
    pub created_at: Option<String>,
    pub author: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub synced_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Manual apply result carries the retained identity because hash dedup may replace the input id.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedWallpaper {
    pub processed: ProcessedImage,
    pub wallpaper: WallpaperRecord,
}

/// Paginated response keeps catalog queries bounded as the metadata set grows.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperPage {
    pub items: Vec<WallpaperRecord>,
    pub page: u32,
    pub page_size: u32,
    pub total: u64,
}

/// One physical file copy participating in a content-hash duplicate group.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFileCopy {
    pub wallpaper_id: i64,
    pub path: String,
    pub storage_kind: String,
    pub availability: String,
    pub file_size: Option<i64>,
}

/// Groups file locations without deleting or silently merging user-owned originals.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateFileGroup {
    pub content_hash: String,
    pub copies: Vec<DuplicateFileCopy>,
}

/// Bounded metadata filters used by every Phase 7 catalog page.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CatalogQuery {
    pub keyword: Option<String>,
    pub name: Option<String>,
    pub category: Option<String>,
    pub provider: Option<String>,
    /// Restricts results to originals that are already available on this device.
    pub locally_available: bool,
    /// Includes indexed file records even when a source is currently offline or missing.
    pub file_backed: bool,
    pub download_status: Option<String>,
    pub file_availability: Option<String>,
    pub storage_kind: Option<String>,
    pub collection_id: Option<i64>,
    pub favorite: Option<bool>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub max_width: Option<u32>,
    pub max_height: Option<u32>,
    pub aspect_ratio: Option<String>,
    pub mime_type: Option<String>,
    pub tags: Vec<String>,
    pub include_blacklisted: bool,
    pub sort: Option<String>,
    pub page: u32,
    pub page_size: u32,
}
