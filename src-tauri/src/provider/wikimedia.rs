use std::{collections::HashMap, path::PathBuf, time::Duration};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use tokio::{fs, io::AsyncWriteExt};

use crate::{
    error::{AppError, AppResult},
    image_processing::inspect_image,
};

use super::{
    RemoteWallpaper, WallpaperProvider, WallpaperQuery, WallpaperSort, metadata_matches_keyword,
    provider_keywords,
};

const API_URL: &str = "https://commons.wikimedia.org/w/api.php";
const MAX_DOWNLOAD_BYTES: u64 = 100 * 1024 * 1024;

/// Official MediaWiki Action API adapter for high-resolution Wikimedia Commons images.
pub struct WikimediaCommonsProvider {
    client: Client,
    download_directory: PathBuf,
}

impl WikimediaCommonsProvider {
    /// Builds an identifiable, timeout-bounded client without an embedded API key.
    pub fn new(download_directory: PathBuf) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("4K-Wallpaper-Desktop/0.2 (+https://github.com/SlyLu/4K-Wallpaper-Desktop)")
            .build()?;
        Ok(Self {
            client,
            download_directory,
        })
    }

    /// Runs one bounded File-namespace search and filters unsupported or undersized media.
    async fn query(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        let page_size = query.page_size.clamp(1, 50);
        let offset = query.page.max(1).saturating_sub(1) * page_size;
        let keywords = query
            .keyword
            .as_deref()
            .map(provider_keywords)
            .unwrap_or_default();
        let search = if keywords.is_empty() {
            "filetype:bitmap".to_owned()
        } else {
            format!("({}) filetype:bitmap", keywords.join(" OR "))
        };
        let sort = match query.sort {
            WallpaperSort::Latest => "create_timestamp_desc",
            WallpaperSort::Popular => "incoming_links_desc",
            WallpaperSort::Random => "random",
        };
        let response = self
            .client
            .get(API_URL)
            .query(&[
                ("action", "query"),
                ("format", "json"),
                ("formatversion", "2"),
                ("generator", "search"),
                ("gsrnamespace", "6"),
                ("gsrsearch", search.as_str()),
                ("gsrlimit", &page_size.to_string()),
                ("gsroffset", &offset.to_string()),
                ("gsrsort", sort),
                ("prop", "imageinfo"),
                ("iiprop", "url|size|mime|sha1|timestamp|user|extmetadata"),
                ("iiurlwidth", "640"),
                (
                    "iiextmetadatafilter",
                    "Artist|LicenseShortName|LicenseUrl|ImageDescription|Categories",
                ),
                ("iiextmetadatalanguage", "zh"),
            ])
            .send()
            .await?
            .error_for_status()?;
        let payload: ApiResponse = response.json().await?;
        let results = payload
            .query
            .map(|query_result| query_result.pages)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|page| map_page(page, query.min_width, query.min_height))
            .filter(|wallpaper| metadata_matches_keyword(wallpaper, &keywords))
            .collect();
        Ok(results)
    }
}

#[async_trait]
impl WallpaperProvider for WikimediaCommonsProvider {
    fn provider_name(&self) -> &'static str {
        "wikimedia_commons"
    }

    async fn latest(&self, mut query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        query.sort = WallpaperSort::Latest;
        self.query(query).await
    }

    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        if !remote_id
            .chars()
            .all(|character| character.is_ascii_digit())
        {
            return Err(AppError::Provider(
                "invalid Wikimedia Commons page id".into(),
            ));
        }
        let response = self
            .client
            .get(API_URL)
            .query(&[
                ("action", "query"),
                ("format", "json"),
                ("formatversion", "2"),
                ("pageids", remote_id),
                ("prop", "imageinfo"),
                ("iiprop", "url|size|mime|sha1|timestamp|user|extmetadata"),
                ("iiurlwidth", "1280"),
                (
                    "iiextmetadatafilter",
                    "Artist|LicenseShortName|LicenseUrl|ImageDescription|Categories",
                ),
            ])
            .send()
            .await?
            .error_for_status()?;
        let payload: ApiResponse = response.json().await?;
        payload
            .query
            .and_then(|query| query.pages.into_iter().next())
            .and_then(|page| map_page(page, 0, 0))
            .ok_or_else(|| AppError::Provider("Wikimedia Commons image was not found".into()))
    }

    /// Downloads one original atomically and validates decoded image content before retention.
    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name() {
            return Err(AppError::Provider(
                "wallpaper does not belong to Wikimedia Commons".into(),
            ));
        }
        let original_url = wallpaper
            .original_url
            .as_deref()
            .ok_or_else(|| AppError::Provider("Wikimedia image has no original URL".into()))?;
        let extension = extension_for(wallpaper.mime_type.as_deref(), original_url);
        fs::create_dir_all(&self.download_directory).await?;
        let target = self
            .download_directory
            .join(format!("wikimedia-{}.{}", wallpaper.remote_id, extension));
        if target.is_file() {
            inspect_image(&target)?;
            return Ok(target);
        }
        let temporary = target.with_extension(format!("{}.tmp", extension));
        let response = self
            .client
            .get(original_url)
            .send()
            .await?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|size| size > MAX_DOWNLOAD_BYTES)
        {
            return Err(AppError::Provider(
                "Wikimedia image exceeds the download size limit".into(),
            ));
        }
        let mut file = fs::File::create(&temporary).await?;
        let mut received = 0_u64;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            received = received.saturating_add(chunk.len() as u64);
            if received > MAX_DOWNLOAD_BYTES {
                let _ = fs::remove_file(&temporary).await;
                return Err(AppError::Provider(
                    "Wikimedia image exceeds the download size limit".into(),
                ));
            }
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        drop(file);
        if let Err(error) = inspect_image(&temporary) {
            let _ = fs::remove_file(&temporary).await;
            return Err(error);
        }
        fs::rename(&temporary, &target).await?;
        Ok(target)
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    query: Option<QueryResult>,
}

#[derive(Deserialize)]
struct QueryResult {
    #[serde(default)]
    pages: Vec<Page>,
}

#[derive(Deserialize)]
struct Page {
    pageid: i64,
    title: String,
    #[serde(default)]
    imageinfo: Vec<ImageInfo>,
}

#[derive(Deserialize)]
struct ImageInfo {
    url: String,
    descriptionurl: String,
    thumburl: Option<String>,
    width: u32,
    height: u32,
    size: i64,
    mime: String,
    timestamp: Option<String>,
    user: Option<String>,
    #[serde(default)]
    extmetadata: HashMap<String, MetadataValue>,
}

#[derive(Deserialize)]
struct MetadataValue {
    value: String,
}

/// Maps only supported raster formats and retains attribution required for lawful reuse.
fn map_page(page: Page, min_width: u32, min_height: u32) -> Option<RemoteWallpaper> {
    let info = page.imageinfo.into_iter().next()?;
    if info.width < min_width
        || info.height < min_height
        || !matches!(
            info.mime.as_str(),
            "image/jpeg" | "image/png" | "image/webp"
        )
    {
        return None;
    }
    let metadata = |key: &str| {
        info.extmetadata
            .get(key)
            .map(|value| strip_html(&value.value))
            .filter(|value| !value.is_empty())
    };
    let title = page.title.strip_prefix("File:").unwrap_or(&page.title);
    let description = metadata("ImageDescription");
    let author = metadata("Artist").or_else(|| info.user.clone());
    let license_name = metadata("LicenseShortName");
    let license_url = metadata("LicenseUrl");
    let tags = metadata("Categories")
        .map(|categories| {
            categories
                .split('|')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(20)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(RemoteWallpaper {
        remote_id: page.pageid.to_string(),
        provider: "wikimedia_commons".into(),
        name: description.unwrap_or_else(|| title.to_owned()),
        source_page_url: Some(info.descriptionurl),
        original_url: Some(info.url),
        thumbnail_url: info.thumburl,
        thumbnail_local_path: None,
        local_path: None,
        width: Some(info.width),
        height: Some(info.height),
        resolution: Some(format!("{}x{}", info.width, info.height)),
        ratio: Some(reduced_ratio(info.width, info.height)),
        file_size: Some(info.size),
        mime_type: Some(info.mime),
        category: "nature".into(),
        purity: "sfw".into(),
        tags,
        created_at: info.timestamp,
        author,
        license_name,
        license_url,
        perceptual_hash: None,
    })
}

/// Removes simple HTML markup emitted by extmetadata without rendering remote content.
fn strip_html(value: &str) -> String {
    let mut result = String::new();
    let mut inside_tag = false;
    for character in value.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => result.push(character),
            _ => {}
        }
    }
    result.trim().to_owned()
}

/// Produces stable aspect-ratio labels without a floating-point dependency.
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

/// Chooses an extension from trusted MIME first and URL path only as a fallback.
fn extension_for(mime: Option<&str>, url: &str) -> &'static str {
    match mime {
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ if url.to_ascii_lowercase().ends_with(".png") => "png",
        _ if url.to_ascii_lowercase().ends_with(".webp") => "webp",
        _ => "jpg",
    }
}

#[cfg(test)]
mod tests {
    use super::{WikimediaCommonsProvider, extension_for, reduced_ratio, strip_html};
    use crate::provider::{WallpaperCategory, WallpaperProvider, WallpaperQuery, WallpaperSort};

    #[test]
    fn normalizes_commons_metadata_helpers() {
        assert_eq!(strip_html("<span>Jane Doe</span>"), "Jane Doe");
        assert_eq!(reduced_ratio(3840, 2160), "16:9");
        assert_eq!(extension_for(Some("image/png"), "https://x/image"), "png");
    }

    #[tokio::test]
    #[ignore = "calls the live Wikimedia Commons API"]
    async fn live_search_returns_licensed_high_resolution_images()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let provider = WikimediaCommonsProvider::new(directory.path().to_path_buf())?;
        let results = provider
            .search(WallpaperQuery {
                keyword: Some("featured landscape".into()),
                category: WallpaperCategory::Nature,
                min_width: 1920,
                min_height: 1080,
                aspect_ratio: None,
                page: 1,
                page_size: 20,
                sort: WallpaperSort::Popular,
                safety: "sfw".into(),
                providers: None,
            })
            .await?;
        assert!(!results.is_empty());
        assert!(results.iter().all(|wallpaper| {
            wallpaper.original_url.is_some()
                && wallpaper.source_page_url.is_some()
                && wallpaper.license_name.is_some()
        }));
        Ok(())
    }
}
