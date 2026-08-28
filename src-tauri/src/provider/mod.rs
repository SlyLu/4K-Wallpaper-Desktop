mod aggregated;
mod art_institute;
mod download;
mod local;
mod openverse;
mod thegamesdb;
mod wallhaven;
mod wikimedia;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{
    error::{AppError, AppResult},
    paths::AppPaths,
};

pub use art_institute::ArtInstituteChicagoProvider;
pub use local::LocalProvider;
pub use openverse::OpenverseProvider;
pub use thegamesdb::TheGamesDbProvider;
pub use wallhaven::WallhavenProvider;
pub use wikimedia::WikimediaCommonsProvider;

/// Product categories remain stable even when a provider uses different category flags.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WallpaperCategory {
    #[default]
    All,
    Nature,
    Anime,
    Games,
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
    /// Optional provider scope supplied by the UI; absence means every enabled online source.
    #[serde(default)]
    pub providers: Option<Vec<String>>,
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
            providers: None,
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
    pub author: Option<String>,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    /// Optional thumbnail-derived dHash supports cross-provider pre-display deduplication.
    pub perceptual_hash: Option<String>,
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
#[derive(Clone)]
pub struct ProviderServices {
    wallhaven: Arc<dyn WallpaperProvider>,
    wikimedia: Arc<dyn WallpaperProvider>,
    openverse: Arc<dyn WallpaperProvider>,
    art_institute: Arc<dyn WallpaperProvider>,
    thegamesdb: Arc<TheGamesDbProvider>,
    local: Arc<dyn WallpaperProvider>,
}

impl ProviderServices {
    /// Initializes both V1 adapters; local roots remain empty until the user selects folders.
    pub fn new(paths: &AppPaths) -> AppResult<Self> {
        Ok(Self {
            wallhaven: Arc::new(WallhavenProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            wikimedia: Arc::new(WikimediaCommonsProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            openverse: Arc::new(OpenverseProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            art_institute: Arc::new(ArtInstituteChicagoProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            thegamesdb: Arc::new(TheGamesDbProvider::new(
                paths.wallpapers_original_dir.clone(),
            )?),
            local: Arc::new(LocalProvider::new(Vec::new())),
        })
    }

    /// Resolves only supported V1 providers and reports unsupported names as recoverable errors.
    pub fn get(&self, provider: &str) -> AppResult<Arc<dyn WallpaperProvider>> {
        match provider {
            "wallhaven" => Ok(Arc::clone(&self.wallhaven)),
            "wikimedia_commons" => Ok(Arc::clone(&self.wikimedia)),
            "openverse" => Ok(Arc::clone(&self.openverse)),
            "art_institute_chicago" => Ok(Arc::clone(&self.art_institute)),
            "thegamesdb" => Ok(Arc::clone(&self.thegamesdb) as Arc<dyn WallpaperProvider>),
            "local" => Ok(Arc::clone(&self.local)),
            _ => Err(AppError::Provider(format!(
                "unsupported wallpaper provider: {provider}"
            ))),
        }
    }

    /// Returns every built-in online adapter for aggregated search and refresh.
    pub fn online(&self) -> Vec<Arc<dyn WallpaperProvider>> {
        vec![
            Arc::clone(&self.wallhaven),
            Arc::clone(&self.wikimedia),
            Arc::clone(&self.openverse),
            Arc::clone(&self.art_institute),
            Arc::clone(&self.thegamesdb) as Arc<dyn WallpaperProvider>,
        ]
    }

    /// Applies a user-owned key in memory without logging or exposing its value to providers.
    pub fn configure_thegamesdb_api_key(&self, api_key: Option<&str>) -> AppResult<()> {
        self.thegamesdb.set_api_key(api_key)
    }
}

/// Expands a small, auditable set of common Chinese wallpaper terms for English-first APIs.
pub(crate) fn provider_keywords(keyword: &str) -> Vec<String> {
    let trimmed = keyword.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let normalized = trimmed.to_lowercase();
    let alias = match normalized.as_str() {
        "七龙珠" | "龙珠" => Some("Dragon Ball"),
        "孙悟空" | "悟空" => Some("Goku"),
        "海贼王" => Some("One Piece anime"),
        "火影忍者" | "火影" => Some("Naruto anime"),
        "黑神话悟空" | "黑神话：悟空" => Some("Black Myth Wukong"),
        "赛博朋克2077" | "赛博朋克 2077" => Some("Cyberpunk 2077"),
        "原神" => Some("Genshin Impact"),
        "艾尔登法环" => Some("Elden Ring"),
        "塞尔达传说" | "塞尔达" => Some("The Legend of Zelda"),
        "英雄联盟" => Some("League of Legends"),
        "守望先锋" => Some("Overwatch"),
        "雪山" => Some("snow mountain"),
        "日落" | "夕阳" => Some("sunset"),
        "星空" => Some("starry sky"),
        "城市夜景" | "夜景" => Some("city night"),
        _ => None,
    };
    let mut keywords = vec![trimmed.to_owned()];
    if let Some(alias) = alias {
        keywords.push(alias.to_owned());
    }
    keywords
}

/// Uses normalized title/tag text to reject known-loose provider matches such as Commons search.
pub(crate) fn metadata_matches_keyword(wallpaper: &RemoteWallpaper, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return true;
    }
    let searchable = std::iter::once(wallpaper.name.as_str())
        .chain(wallpaper.tags.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    keywords
        .iter()
        .map(|keyword| keyword.to_lowercase())
        .any(|keyword| searchable.contains(&keyword))
}

#[cfg(test)]
mod query_tests {
    use super::{RemoteWallpaper, metadata_matches_keyword, provider_keywords};

    /// Builds the smallest provider-neutral record needed for relevance assertions.
    fn wallpaper(name: &str, tags: &[&str]) -> RemoteWallpaper {
        RemoteWallpaper {
            remote_id: "fixture".into(),
            provider: "wikimedia_commons".into(),
            name: name.into(),
            source_page_url: None,
            original_url: None,
            thumbnail_url: None,
            local_path: None,
            width: Some(3840),
            height: Some(2160),
            resolution: Some("3840x2160".into()),
            ratio: Some("16:9".into()),
            file_size: None,
            mime_type: Some("image/jpeg".into()),
            category: "all".into(),
            purity: "sfw".into(),
            tags: tags.iter().map(|tag| (*tag).into()).collect(),
            created_at: None,
            author: None,
            license_name: None,
            license_url: None,
            perceptual_hash: None,
        }
    }

    #[test]
    fn expands_chinese_titles_and_rejects_unrelated_metadata() {
        let keywords = provider_keywords("七龙珠");
        assert_eq!(keywords, ["七龙珠", "Dragon Ball"]);
        assert!(metadata_matches_keyword(
            &wallpaper("Dragon Ball Z mural", &["anime"]),
            &keywords
        ));
        assert!(!metadata_matches_keyword(
            &wallpaper("Stillgelegte Zabergäubahn", &["railway"]),
            &keywords
        ));
        assert_eq!(
            provider_keywords("黑神话悟空"),
            ["黑神话悟空", "Black Myth Wukong"]
        );
    }
}
#[cfg(not(test))]
pub use aggregated::AggregatedProviderService;
