use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::{
    RemoteWallpaper, WallpaperCategory, WallpaperProvider, WallpaperQuery, download,
    provider_keywords,
};

const API_ROOT: &str = "https://api.artic.edu/api/v1";
const IIIF_ROOT: &str = "https://www.artic.edu/iiif/2";

/// Public-domain artwork provider backed by the Art Institute of Chicago API and IIIF service.
pub struct ArtInstituteChicagoProvider {
    client: Client,
    download_directory: PathBuf,
}

impl ArtInstituteChicagoProvider {
    /// Builds one identifiable client for both metadata and IIIF dimension requests.
    pub fn new(download_directory: PathBuf) -> AppResult<Self> {
        Ok(Self {
            client: Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(40))
                .user_agent(
                    "4K-Wallpaper-Desktop/0.2 (+https://github.com/SlyLu/4K-Wallpaper-Desktop)",
                )
                .build()?,
            download_directory,
        })
    }

    /// Searches public-domain artworks, then verifies actual IIIF pixel dimensions in parallel.
    async fn query(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        if query.category == WallpaperCategory::Games {
            return Ok(Vec::new());
        }
        let mut keywords = query
            .keyword
            .as_deref()
            .map(provider_keywords)
            .unwrap_or_default();
        if keywords.is_empty() {
            let fallback = match query.category {
                WallpaperCategory::Nature => "landscape nature",
                WallpaperCategory::People => "portrait",
                WallpaperCategory::Anime => "Japanese print",
                WallpaperCategory::Games => return Ok(Vec::new()),
                WallpaperCategory::All => "landscape",
                WallpaperCategory::Local => {
                    return Err(AppError::Provider(
                        "the local category must use LocalProvider".into(),
                    ));
                }
            };
            keywords.push(fallback.into());
        }
        let response = self
            .client
            .get(format!("{API_ROOT}/artworks/search"))
            .query(&[
                // The museum search endpoint treats extra translated tokens as mandatory terms.
                ("q", keywords.last().cloned().unwrap_or_default()),
                ("page", query.page.max(1).to_string()),
                ("limit", query.page_size.clamp(1, 30).to_string()),
                (
                    "fields",
                    "id,title,image_id,thumbnail,artist_display,date_display,is_public_domain,classification_title,subject_titles".into(),
                ),
            ])
            .send()
            .await?
            .error_for_status()?;
        let payload: SearchResponse = response.json().await?;
        Ok(payload
            .data
            .into_iter()
            .filter(|artwork| artwork.is_public_domain)
            .filter_map(|artwork| map_artwork(artwork, query.min_width, query.min_height))
            .collect())
    }

    /// Loads one artwork and its IIIF dimensions for detail/download recovery.
    async fn load_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        let response = self
            .client
            .get(format!("{API_ROOT}/artworks/{remote_id}"))
            .query(&[("fields", "id,title,image_id,thumbnail,artist_display,date_display,is_public_domain,classification_title,subject_titles")])
            .send()
            .await?
            .error_for_status()?;
        let payload: DetailResponse = response.json().await?;
        if !payload.data.is_public_domain {
            return Err(AppError::Provider(
                "artwork is not available as public domain".into(),
            ));
        }
        map_artwork(payload.data, 0, 0)
            .ok_or_else(|| AppError::Provider("artwork image is not usable".into()))
    }
}

#[async_trait]
impl WallpaperProvider for ArtInstituteChicagoProvider {
    fn provider_name(&self) -> &'static str {
        "art_institute_chicago"
    }

    async fn latest(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
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
                "invalid Art Institute artwork id".into(),
            ));
        }
        self.load_detail(remote_id).await
    }

    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name()
            || !wallpaper
                .remote_id
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err(AppError::Provider(
                "wallpaper does not belong to ArtInstituteChicagoProvider".into(),
            ));
        }
        let detail = self.load_detail(&wallpaper.remote_id).await?;
        let url = detail
            .original_url
            .as_deref()
            .ok_or_else(|| AppError::Provider("artwork has no original URL".into()))?;
        download::download_original(
            &self.client,
            &self.download_directory,
            self.provider_name(),
            &wallpaper.remote_id,
            url,
            Some("image/jpeg"),
        )
        .await
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    data: Vec<Artwork>,
}

#[derive(Deserialize)]
struct DetailResponse {
    data: Artwork,
}

#[derive(Deserialize)]
struct Artwork {
    id: i64,
    title: String,
    image_id: Option<String>,
    thumbnail: Option<ArtworkThumbnail>,
    artist_display: Option<String>,
    date_display: Option<String>,
    #[serde(default)]
    is_public_domain: bool,
    classification_title: Option<String>,
    #[serde(default)]
    subject_titles: Vec<String>,
}

#[derive(Deserialize)]
struct ArtworkThumbnail {
    width: u32,
    height: u32,
}

/// Maps verified public-domain artwork while retaining institution attribution and subjects.
fn map_artwork(artwork: Artwork, min_width: u32, min_height: u32) -> Option<RemoteWallpaper> {
    let image_id = artwork.image_id?;
    let dimensions = artwork.thumbnail?;
    if dimensions.width < min_width || dimensions.height < min_height {
        return None;
    }
    let mut tags = artwork.subject_titles;
    if let Some(classification) = artwork.classification_title {
        tags.push(classification);
    }
    Some(RemoteWallpaper {
        remote_id: artwork.id.to_string(),
        provider: "art_institute_chicago".into(),
        name: artwork.title,
        source_page_url: Some(format!("https://www.artic.edu/artworks/{}", artwork.id)),
        original_url: Some(format!("{IIIF_ROOT}/{image_id}/full/full/0/default.jpg")),
        thumbnail_url: Some(format!("{IIIF_ROOT}/{image_id}/full/843,/0/default.jpg")),
        local_path: None,
        width: Some(dimensions.width),
        height: Some(dimensions.height),
        resolution: Some(format!("{}x{}", dimensions.width, dimensions.height)),
        ratio: Some(reduced_ratio(dimensions.width, dimensions.height)),
        file_size: None,
        mime_type: Some("image/jpeg".into()),
        category: "all".into(),
        purity: "sfw".into(),
        tags,
        created_at: artwork.date_display,
        author: artwork.artist_display,
        license_name: Some("Public Domain".into()),
        license_url: Some("https://creativecommons.org/publicdomain/mark/1.0/".into()),
        perceptual_hash: None,
    })
}

/// Produces stable aspect labels without floating-point rounding differences.
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
    use super::{ArtInstituteChicagoProvider, Artwork, ArtworkThumbnail, map_artwork};
    use crate::provider::{WallpaperProvider, WallpaperQuery};

    #[test]
    fn excludes_undersized_artwork_and_maps_public_domain_metadata() {
        let artwork = Artwork {
            id: 42,
            title: "Lake".into(),
            image_id: Some("abc".into()),
            thumbnail: Some(ArtworkThumbnail {
                width: 3000,
                height: 2000,
            }),
            artist_display: Some("Artist".into()),
            date_display: Some("1900".into()),
            is_public_domain: true,
            classification_title: Some("painting".into()),
            subject_titles: vec!["landscape".into()],
        };
        assert!(map_artwork(artwork, 3840, 2160).is_none());
    }

    #[tokio::test]
    #[ignore = "calls the live Art Institute of Chicago API"]
    async fn live_search_returns_public_domain_iiif_images()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let provider = ArtInstituteChicagoProvider::new(directory.path().to_path_buf())?;
        let results = provider
            .search(WallpaperQuery {
                keyword: Some("landscape".into()),
                min_width: 1920,
                min_height: 1080,
                page_size: 20,
                ..WallpaperQuery::default()
            })
            .await?;
        assert!(!results.is_empty());
        assert!(results.iter().all(|wallpaper| {
            wallpaper.license_name.as_deref() == Some("Public Domain")
                && wallpaper.original_url.is_some()
        }));
        Ok(())
    }
}
