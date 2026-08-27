mod local;
mod wallhaven;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    paths::AppPaths,
};

pub use local::LocalProvider;
pub use wallhaven::WallhavenProvider;

/// Product categories remain stable even when a provider uses different category flags.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperCategory {
    #[default]
    All,
    Nature,
    Anime,
    People,
    Local,
}

/// Sorting vocabulary supported by the business layer rather than one remote API.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperSort {
    #[default]
    Latest,
    Popular,
    Random,
}

/// Provider-neutral query with safe 4K landscape defaults from the V1 baseline.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperQuery {
    pub keyword: Option<String>,
    pub category: WallpaperCategory,
    pub min_width: u32,
    pub min_height: u32,
    pub aspect_ratio: Option<String>,
    pub page: u32,
    pub page_size: u32,
    pub sort: WallpaperSort,
    pub safety: String,
}

impl Default for WallpaperQuery {
    /// Defaults guarantee that ordinary online browsing remains SFW and at least 4K.
    fn default() -> Self {
        Self {
            keyword: None,
            category: WallpaperCategory::All,
            min_width: 3840,
            min_height: 2160,
            aspect_ratio: Some("16:9".into()),
            page: 1,
            page_size: 24,
            sort: WallpaperSort::Latest,
            safety: "sfw".into(),
        }
    }
}

/// Complete provider-neutral metadata returned by remote and local adapters.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWallpaper {
    pub remote_id: String,
    pub provider: String,
    pub name: String,
    pub source_page_url: Option<String>,
    pub original_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub local_path: Option<PathBuf>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub resolution: Option<String>,
    pub ratio: Option<String>,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub category: String,
    pub purity: String,
    pub tags: Vec<String>,
    pub created_at: Option<String>,
}

/// Unified adapter boundary used by services and commands.
#[async_trait]
pub trait WallpaperProvider: Send + Sync {
    fn provider_name(&self) -> &'static str;
    async fn latest(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>>;
    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>>;
    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper>;
    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf>;
}

/// Runtime registry routes provider names without leaking concrete adapters into commands.
pub struct ProviderServices {
    wallhaven: Arc<dyn WallpaperProvider>,
    local: Arc<dyn WallpaperProvider>,
}

impl ProviderServices {
    /// Initializes both V1 adapters; local roots remain empty until the user selects folders.
    pub fn new(paths: &AppPaths) -> AppResult<Self> {
        Ok(Self {
            wallhaven: Arc::new(WallhavenProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            local: Arc::new(LocalProvider::new(Vec::new())),
        })
    }

    /// Resolves only supported V1 providers and reports unsupported names as recoverable errors.
    pub fn get(&self, provider: &str) -> AppResult<Arc<dyn WallpaperProvider>> {
        match provider {
            "wallhaven" => Ok(Arc::clone(&self.wallhaven)),
            "local" => Ok(Arc::clone(&self.local)),
            _ => Err(AppError::Provider(format!(
                "unsupported wallpaper provider: {provider}"
            ))),
        }
    }
}
