use serde::{Deserialize, Serialize};

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
    pub download_status: String,
    pub preset: bool,
    pub created_at: Option<String>,
    pub synced_at: String,
    #[serde(default)]
    pub tags: Vec<String>,
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
    pub favorite: Option<bool>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub include_blacklisted: bool,
    pub sort: Option<String>,
    pub page: u32,
    pub page_size: u32,
}
