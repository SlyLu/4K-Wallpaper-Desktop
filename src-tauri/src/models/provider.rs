use serde::Serialize;

/// Persisted provider configuration and latest isolated health result.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatus {
    pub provider: String,
    pub enabled: bool,
    pub status: String,
    pub last_success_at: Option<String>,
    pub last_error_at: Option<String>,
    pub last_error: Option<String>,
    pub response_time_ms: Option<i64>,
}

/// Provenance retained for every provider that resolves to one unified wallpaper entity.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperProviderSource {
    pub provider: String,
    pub remote_id: String,
    pub source_page_url: Option<String>,
    pub original_url: Option<String>,
    pub author: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
