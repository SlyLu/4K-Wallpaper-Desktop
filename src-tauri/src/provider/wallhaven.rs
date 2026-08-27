use std::{path::PathBuf, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{fs, io::AsyncWriteExt, sync::Semaphore};

use crate::{
    error::{AppError, AppResult},
    image_processing::inspect_image,
};

use super::{RemoteWallpaper, WallpaperCategory, WallpaperProvider, WallpaperQuery, WallpaperSort};

const API_ROOT: &str = "https://wallhaven.cc/api/v1";

/// Wallhaven adapter with a bounded download pool and no required API key for SFW data.
pub struct WallhavenProvider {
    client: reqwest::Client,
    download_directory: PathBuf,
    download_slots: Arc<Semaphore>,
}

impl WallhavenProvider {
    /// Builds a client with explicit timeouts so an unavailable provider cannot hang startup.
    pub fn new(download_directory: PathBuf) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .user_agent("4K-Wallpaper-Desktop/0.1")
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            client,
            download_directory,
            download_slots: Arc::new(Semaphore::new(3)),
        })
    }

    /// Maps the stable product query into Wallhaven's flags only inside this adapter.
    async fn query(&self, mut query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        if !query.safety.eq_ignore_ascii_case("sfw") {
            return Err(AppError::Provider(
                "V1 only permits SFW Wallhaven requests".into(),
            ));
        }
        let (categories, category_keyword) = match query.category {
            WallpaperCategory::All => ("111", None),
            WallpaperCategory::Nature => ("100", Some("nature")),
            WallpaperCategory::Anime => ("010", None),
            WallpaperCategory::People => ("001", None),
            WallpaperCategory::Local => {
                return Err(AppError::Provider(
                    "the local category must use LocalProvider".into(),
                ));
            }
        };
        if query.keyword.as_deref().is_none_or(str::is_empty) {
            query.keyword = category_keyword.map(str::to_owned);
        }
        let sorting = match query.sort {
            WallpaperSort::Latest => "date_added",
            WallpaperSort::Popular => "toplist",
            WallpaperSort::Random => "random",
        };
        let mut parameters = vec![
            ("categories", categories.to_owned()),
            ("purity", "100".to_owned()),
            ("sorting", sorting.to_owned()),
            ("order", "desc".to_owned()),
            (
                "atleast",
                format!("{}x{}", query.min_width, query.min_height),
            ),
            ("page", query.page.max(1).to_string()),
        ];
        if let Some(keyword) = query.keyword.filter(|value| !value.trim().is_empty()) {
            parameters.push(("q", keyword));
        }
        if let Some(ratio) = query.aspect_ratio.filter(|value| !value.trim().is_empty()) {
            parameters.push(("ratios", ratio.replace(':', "x")));
        }

        let response = self
            .client
            .get(format!("{API_ROOT}/search"))
            .query(&parameters)
            .send()
            .await?
            .error_for_status()?;
        let payload: SearchResponse = response.json().await?;
        let limit = query.page_size.clamp(1, 24) as usize;
        Ok(payload
            .data
            .into_iter()
            .take(limit)
            .map(map_listing)
            .collect())
    }
}

#[async_trait]
impl WallpaperProvider for WallhavenProvider {
    fn provider_name(&self) -> &'static str {
        "wallhaven"
    }

    /// Latest is a query specialization rather than a second API implementation.
    async fn latest(&self, mut query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        query.sort = WallpaperSort::Latest;
        self.query(query).await
    }

    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        if !valid_remote_id(remote_id) {
            return Err(AppError::Provider("invalid Wallhaven wallpaper id".into()));
        }
        let response = self
            .client
            .get(format!("{API_ROOT}/w/{remote_id}"))
            .send()
            .await?
            .error_for_status()?;
        let payload: DetailResponse = response.json().await?;
        Ok(map_detail(payload.data))
    }

    /// Streams an original into a temporary file, validates its signature, then renames it.
    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name() || !valid_remote_id(&wallpaper.remote_id) {
            return Err(AppError::Provider(
                "wallpaper does not belong to WallhavenProvider".into(),
            ));
        }
        let url = wallpaper.original_url.as_deref().ok_or_else(|| {
            AppError::Provider("Wallhaven metadata did not include an original URL".into())
        })?;
        if !url.starts_with("https://") {
            return Err(AppError::Provider(
                "Wallhaven downloads require an HTTPS URL".into(),
            ));
        }
        let _permit =
            self.download_slots.acquire().await.map_err(|_| {
                AppError::Provider("Wallhaven download limiter is unavailable".into())
            })?;
        fs::create_dir_all(&self.download_directory).await?;
        let extension = file_extension(wallpaper.mime_type.as_deref(), url);
        let target = self
            .download_directory
            .join(format!("wallhaven-{}.{}", wallpaper.remote_id, extension));
        if fs::try_exists(&target).await? {
            return Ok(target);
        }
        let temporary = target.with_extension(format!("{extension}.part"));
        let result = self.download_to(url, &temporary).await;
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary).await;
            return Err(error);
        }
        fs::rename(&temporary, &target).await?;
        Ok(target)
    }
}

impl WallhavenProvider {
    /// Downloads one response while hashing it and retaining only a small signature prefix.
    async fn download_to(&self, url: &str, target: &PathBuf) -> AppResult<()> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut file = fs::File::create(target).await?;
        let mut signature = Vec::with_capacity(16);
        let mut hasher = Sha256::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let needed = 16_usize.saturating_sub(signature.len());
            signature.extend_from_slice(&chunk[..chunk.len().min(needed)]);
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        if !has_supported_image_signature(&signature) {
            return Err(AppError::Image(
                "downloaded Wallhaven payload is not JPEG, PNG, or WebP".into(),
            ));
        }
        let streamed_hash = format!("{:x}", hasher.finalize());
        let decoded = inspect_image(target)?;
        if decoded.sha256 != streamed_hash {
            return Err(AppError::Image(
                "downloaded image hash changed during validation".into(),
            ));
        }
        tracing::info!(sha256 = %streamed_hash, width = decoded.width, height = decoded.height, "original download verified");
        Ok(())
    }
}

/// Restricts ids used in URLs and file names to Wallhaven's public identifier shape.
fn valid_remote_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

/// Chooses an extension from trusted metadata with the URL as a compatibility fallback.
fn file_extension(mime_type: Option<&str>, url: &str) -> &'static str {
    match mime_type {
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ if url.to_ascii_lowercase().ends_with(".png") => "png",
        _ if url.to_ascii_lowercase().ends_with(".webp") => "webp",
        _ => "jpg",
    }
}

/// Rejects HTML error pages even if a provider endpoint returns HTTP success.
fn has_supported_image_signature(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP"))
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Vec<Listing>,
}

#[derive(Deserialize)]
struct DetailResponse {
    data: Detail,
}

#[derive(Deserialize)]
struct Listing {
    id: String,
    url: String,
    purity: String,
    category: String,
    dimension_x: u32,
    dimension_y: u32,
    resolution: String,
    ratio: String,
    file_size: i64,
    file_type: String,
    created_at: String,
    path: String,
    thumbs: Thumbnails,
}

#[derive(Deserialize)]
struct Detail {
    #[serde(flatten)]
    listing: Listing,
    #[serde(default)]
    tags: Vec<Tag>,
}

#[derive(Deserialize)]
struct Thumbnails {
    large: String,
}

#[derive(Deserialize)]
struct Tag {
    name: String,
}

/// Converts Wallhaven listing fields into the provider-neutral contract.
fn map_listing(value: Listing) -> RemoteWallpaper {
    RemoteWallpaper {
        name: format!("Wallhaven {}", value.id),
        remote_id: value.id,
        provider: "wallhaven".into(),
        source_page_url: Some(value.url),
        original_url: Some(value.path),
        thumbnail_url: Some(value.thumbs.large),
        local_path: None,
        width: Some(value.dimension_x),
        height: Some(value.dimension_y),
        resolution: Some(value.resolution),
        ratio: Some(value.ratio),
        file_size: Some(value.file_size),
        mime_type: Some(value.file_type),
        category: value.category,
        purity: value.purity,
        tags: Vec::new(),
        created_at: Some(value.created_at),
    }
}

/// Preserves listing mapping while adding detail-only tag metadata.
fn map_detail(value: Detail) -> RemoteWallpaper {
    let tags = value.tags.into_iter().map(|tag| tag.name).collect();
    let mut wallpaper = map_listing(value.listing);
    wallpaper.tags = tags;
    wallpaper
}

#[cfg(test)]
mod tests {
    use super::{
        WallhavenProvider, file_extension, has_supported_image_signature, valid_remote_id,
    };
    use crate::provider::{WallpaperProvider, WallpaperQuery};

    #[test]
    fn validates_download_boundaries() {
        assert!(valid_remote_id("abc123"));
        assert!(!valid_remote_id("../abc"));
        assert_eq!(file_extension(Some("image/png"), "https://x/y"), "png");
        assert!(has_supported_image_signature(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!has_supported_image_signature(b"<html>error"));
    }

    #[tokio::test]
    #[ignore = "calls the live Wallhaven API and downloads one original into a temporary directory"]
    async fn live_latest_detail_and_download() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let provider = WallhavenProvider::new(directory.path().to_path_buf())?;
        let mut query = WallpaperQuery::default();
        query.page_size = 1;
        let listing = provider.latest(query).await?;
        let first = listing
            .first()
            .ok_or("Wallhaven returned no SFW 4K wallpaper")?;
        let detail = provider.get_detail(&first.remote_id).await?;
        assert!(detail.width.is_some_and(|width| width >= 3840));
        assert!(detail.height.is_some_and(|height| height >= 2160));
        assert_eq!(detail.purity, "sfw");
        let downloaded = provider.download(&detail).await?;
        assert!(downloaded.is_file());
        Ok(())
    }
}
