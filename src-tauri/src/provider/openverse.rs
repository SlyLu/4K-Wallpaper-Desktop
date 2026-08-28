use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::{
    RemoteWallpaper, WallpaperCategory, WallpaperProvider, WallpaperQuery, download,
    provider_keywords,
};

const API_URL: &str = "https://api.openverse.org/v1/images/";

/// Openverse adapter for broadly licensed images without embedding an application API key.
pub struct OpenverseProvider {
    client: Client,
    download_directory: PathBuf,
}

impl OpenverseProvider {
    /// Builds a timeout-bounded client so one public API cannot stall aggregated search.
    pub fn new(download_directory: PathBuf) -> AppResult<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(35))
                .user_agent(
                    "4K-Wallpaper-Desktop/0.2 (+https://github.com/SlyLu/4K-Wallpaper-Desktop)",
                )
                .build()?,
            download_directory,
        })
    }

    /// Maps the common query into Openverse and retains only usable high-resolution originals.
    async fn query(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        if !query.safety.eq_ignore_ascii_case("sfw") {
            return Err(AppError::Provider(
                "Openverse requests are restricted to SFW results".into(),
            ));
        }
        let mut keywords = query
            .keyword
            .as_deref()
            .map(provider_keywords)
            .unwrap_or_default();
        if keywords.is_empty() {
            let category = match query.category {
                WallpaperCategory::Nature => "nature landscape",
                WallpaperCategory::Anime => "anime illustration",
                WallpaperCategory::Games => "video game artwork",
                WallpaperCategory::People => "people portrait",
                WallpaperCategory::All => "wallpaper landscape",
                WallpaperCategory::Local => {
                    return Err(AppError::Provider(
                        "the local category must use LocalProvider".into(),
                    ));
                }
            };
            keywords.push(category.to_owned());
        }
        let response = self
            .client
            .get(API_URL)
            .query(&[
                // English-first search indexes perform better with the expanded alias alone.
                ("q", keywords.last().cloned().unwrap_or_default()),
                ("page", query.page.max(1).to_string()),
                ("page_size", query.page_size.clamp(1, 40).to_string()),
                ("mature", "false".to_owned()),
                // Large results avoid exhausting a page on images below the requested 4K floor.
                ("size", "large".to_owned()),
            ])
            .send()
            .await?
            .error_for_status()?;
        let payload: SearchResponse = response.json().await?;
        Ok(payload
            .results
            .into_iter()
            .filter_map(|image| map_image(image, query.min_width, query.min_height))
            .collect())
    }
}

#[async_trait]
impl WallpaperProvider for OpenverseProvider {
    fn provider_name(&self) -> &'static str {
        "openverse"
    }

    async fn latest(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        if !valid_id(remote_id) {
            return Err(AppError::Provider("invalid Openverse image id".into()));
        }
        let response = self
            .client
            .get(format!("{API_URL}{remote_id}/"))
            .send()
            .await?
            .error_for_status()?;
        let image: OpenverseImage = response.json().await?;
        map_image(image, 0, 0)
            .ok_or_else(|| AppError::Provider("Openverse image is not usable".into()))
    }

    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name() || !valid_id(&wallpaper.remote_id) {
            return Err(AppError::Provider(
                "wallpaper does not belong to OpenverseProvider".into(),
            ));
        }
        let url = wallpaper
            .original_url
            .as_deref()
            .ok_or_else(|| AppError::Provider("Openverse image has no original URL".into()))?;
        download::download_original(
            &self.client,
            &self.download_directory,
            self.provider_name(),
            &wallpaper.remote_id,
            url,
            wallpaper.mime_type.as_deref(),
        )
        .await
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<OpenverseImage>,
}

#[derive(Deserialize)]
struct OpenverseImage {
    id: String,
    title: Option<String>,
    creator: Option<String>,
    license: Option<String>,
    license_url: Option<String>,
    foreign_landing_url: Option<String>,
    url: String,
    thumbnail: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    filesize: Option<i64>,
    filetype: Option<String>,
    #[serde(default)]
    tags: Vec<OpenverseTag>,
}

#[derive(Deserialize)]
struct OpenverseTag {
    name: String,
}

/// Converts only HTTPS raster results with enough pixels for the active resolution filter.
fn map_image(image: OpenverseImage, min_width: u32, min_height: u32) -> Option<RemoteWallpaper> {
    let width = image.width?;
    let height = image.height?;
    if width < min_width || height < min_height || !image.url.starts_with("https://") {
        return None;
    }
    let inferred_type = image.filetype.as_deref().or_else(|| {
        image
            .url
            .split('?')
            .next()
            .and_then(|path| path.rsplit('.').next())
    })?;
    let mime_type = match inferred_type.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return None,
    };
    let name = image
        .title
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| format!("Openverse {}", image.id));
    Some(RemoteWallpaper {
        remote_id: image.id,
        provider: "openverse".into(),
        name,
        source_page_url: image.foreign_landing_url,
        original_url: Some(image.url),
        thumbnail_url: image.thumbnail.filter(|url| url.starts_with("https://")),
        local_path: None,
        width: Some(width),
        height: Some(height),
        resolution: Some(format!("{width}x{height}")),
        ratio: Some(reduced_ratio(width, height)),
        file_size: image.filesize,
        mime_type: Some(mime_type.into()),
        category: "all".into(),
        purity: "sfw".into(),
        tags: image
            .tags
            .into_iter()
            .take(24)
            .map(|tag| tag.name)
            .collect(),
        created_at: None,
        author: image.creator,
        license_name: image.license.map(|license| license.to_uppercase()),
        license_url: image.license_url,
        perceptual_hash: None,
    })
}

/// Restricts detail URLs and cache filenames to Openverse UUID-like identifiers.
fn valid_id(remote_id: &str) -> bool {
    !remote_id.is_empty()
        && remote_id
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
}

/// Generates exact integer aspect labels for persisted provider-neutral metadata.
fn reduced_ratio(mut width: u32, mut height: u32) -> String {
    let original = (width, height);
    while height != 0 {
        let remainder = width % height;
        width = height;
        height = remainder;
    }
    let divisor = width.max(1);
    format!("{}:{}", original.0 / divisor, original.1 / divisor)
}

#[cfg(test)]
mod tests {
    use super::{OpenverseImage, OpenverseProvider, OpenverseTag, map_image};
    use crate::provider::{WallpaperProvider, WallpaperQuery};

    #[test]
    fn maps_only_large_supported_openverse_images() {
        let image = OpenverseImage {
            id: "123e4567-e89b-12d3-a456-426614174000".into(),
            title: Some("Mountain lake".into()),
            creator: Some("Jane".into()),
            license: Some("cc0".into()),
            license_url: Some("https://creativecommons.org/publicdomain/zero/1.0/".into()),
            foreign_landing_url: Some("https://example.test/photo".into()),
            url: "https://example.test/photo.jpg".into(),
            thumbnail: Some("https://example.test/thumb.jpg".into()),
            width: Some(5000),
            height: Some(3000),
            filesize: Some(10),
            filetype: Some("jpg".into()),
            tags: vec![OpenverseTag {
                name: "mountain".into(),
            }],
        };
        let mapped = map_image(image, 3840, 2160).expect("large JPEG should map");
        assert_eq!(mapped.provider, "openverse");
        assert_eq!(mapped.license_name.as_deref(), Some("CC0"));
    }

    #[tokio::test]
    #[ignore = "calls the live Openverse API"]
    async fn live_search_returns_licensed_images() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let provider = OpenverseProvider::new(directory.path().to_path_buf())?;
        let results = provider
            .search(WallpaperQuery {
                keyword: Some("mountain landscape".into()),
                min_width: 1920,
                min_height: 1080,
                page_size: 20,
                ..WallpaperQuery::default()
            })
            .await?;
        assert!(!results.is_empty());
        assert!(
            results
                .iter()
                .all(|wallpaper| wallpaper.license_name.is_some())
        );
        Ok(())
    }
}
