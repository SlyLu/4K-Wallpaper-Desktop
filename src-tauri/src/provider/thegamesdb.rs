use std::{collections::HashMap, path::PathBuf, sync::RwLock, time::Duration};

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::error::{AppError, AppResult};

use super::{RemoteWallpaper, WallpaperProvider, WallpaperQuery, download, provider_keywords};

const GAME_SEARCH_URL: &str = "https://api.thegamesdb.net/v1.1/Games/ByGameName";
const GAME_IMAGES_URL: &str = "https://api.thegamesdb.net/v1/Games/Images";

/// Official game-art adapter backed by a user-owned TheGamesDB API key.
pub struct TheGamesDbProvider {
    client: Client,
    download_directory: PathBuf,
    api_key: RwLock<Option<String>>,
}

impl TheGamesDbProvider {
    /// Creates a disabled-until-configured adapter without embedding credentials in the binary.
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
            api_key: RwLock::new(None),
        })
    }

    /// Replaces the runtime credential after settings are saved; empty values disable access.
    pub fn set_api_key(&self, api_key: Option<&str>) -> AppResult<()> {
        let normalized = api_key.map(str::trim).filter(|value| !value.is_empty());
        if normalized.is_some_and(|value| value.len() > 256) {
            return Err(AppError::Configuration(
                "TheGamesDB API key is longer than 256 characters".into(),
            ));
        }
        let mut active = self
            .api_key
            .write()
            .map_err(|_| AppError::Configuration("TheGamesDB API key lock was poisoned".into()))?;
        *active = normalized.map(str::to_owned);
        Ok(())
    }

    /// Returns a short-lived key copy so no lock is held across network awaits.
    fn api_key(&self) -> AppResult<String> {
        self.api_key
            .read()
            .map_err(|_| AppError::Configuration("TheGamesDB API key lock was poisoned".into()))?
            .clone()
            .ok_or_else(|| {
                AppError::Configuration(
                    "TheGamesDB API key is not configured; add it in Settings".into(),
                )
            })
    }

    /// Searches games first, then loads only fanart and screenshots for matching game IDs.
    async fn query(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        let Some(keyword) = query
            .keyword
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            // The API has no wallpaper-oriented latest feed, so scheduled generic refresh skips it.
            return Ok(Vec::new());
        };
        if !query.safety.eq_ignore_ascii_case("sfw") {
            return Err(AppError::Provider(
                "TheGamesDB requests are restricted to SFW results".into(),
            ));
        }

        let api_key = self.api_key()?;
        let expanded = provider_keywords(keyword);
        let search_term = expanded.last().map(String::as_str).unwrap_or(keyword);
        let games = self
            .search_games(&api_key, search_term, query.page.max(1))
            .await?;
        let game_limit = usize::try_from(query.page_size.clamp(1, 24)).unwrap_or(24);
        let games = games.into_iter().take(game_limit).collect::<Vec<_>>();
        if games.is_empty() {
            return Ok(Vec::new());
        }

        let game_titles = games
            .iter()
            .map(|game| (game.id.to_string(), game.game_title.clone()))
            .collect::<HashMap<_, _>>();
        let game_ids = games
            .iter()
            .map(|game| game.id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let images = self.fetch_images(&api_key, &game_ids).await?;
        Ok(map_images(images, &game_titles, &query))
    }

    /// Uses natural title matching while retaining the API's stable numeric game identifiers.
    async fn search_games(&self, api_key: &str, name: &str, page: u32) -> AppResult<Vec<Game>> {
        let response = self
            .client
            .get(GAME_SEARCH_URL)
            .query(&[
                ("apikey", api_key.to_owned()),
                ("name", name.to_owned()),
                ("fields", "alternates".to_owned()),
                ("mode", "natural".to_owned()),
                ("page", page.to_string()),
            ])
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json::<GameSearchResponse>().await?.data.games)
    }

    /// Fetches image metadata in one multi-game request to conserve the user's monthly allowance.
    async fn fetch_images(&self, api_key: &str, game_ids: &str) -> AppResult<GameImagesData> {
        let response = self
            .client
            .get(GAME_IMAGES_URL)
            .query(&[
                ("apikey", api_key),
                ("games_id", game_ids),
                ("filter[type]", "fanart,screenshot"),
            ])
            .send()
            .await?
            .error_for_status()?;
        Ok(response.json::<GameImagesResponse>().await?.data)
    }
}

#[async_trait]
impl WallpaperProvider for TheGamesDbProvider {
    fn provider_name(&self) -> &'static str {
        "thegamesdb"
    }

    async fn latest(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        let (game_id, image_id) = parse_remote_id(remote_id)?;
        let api_key = self.api_key()?;
        let images = self.fetch_images(&api_key, &game_id.to_string()).await?;
        let titles = HashMap::from([(game_id.to_string(), format!("Game {game_id}"))]);
        map_images(
            images,
            &titles,
            &WallpaperQuery {
                keyword: Some(game_id.to_string()),
                min_width: 0,
                min_height: 0,
                aspect_ratio: None,
                ..WallpaperQuery::default()
            },
        )
        .into_iter()
        .find(|wallpaper| wallpaper.remote_id == format!("{game_id}:{image_id}"))
        .ok_or_else(|| AppError::Provider("TheGamesDB image was not found".into()))
    }

    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name() {
            return Err(AppError::Provider(
                "wallpaper does not belong to TheGamesDbProvider".into(),
            ));
        }
        parse_remote_id(&wallpaper.remote_id)?;
        let url = wallpaper
            .original_url
            .as_deref()
            .ok_or_else(|| AppError::Provider("TheGamesDB image has no original URL".into()))?;
        download::download_original(
            &self.client,
            &self.download_directory,
            self.provider_name(),
            &wallpaper.remote_id.replace(':', "-"),
            url,
            wallpaper.mime_type.as_deref(),
        )
        .await
    }
}

#[derive(Deserialize)]
struct GameSearchResponse {
    data: GameSearchData,
}

#[derive(Deserialize)]
struct GameSearchData {
    #[serde(default)]
    games: Vec<Game>,
}

#[derive(Deserialize)]
struct Game {
    id: u64,
    game_title: String,
}

#[derive(Deserialize)]
struct GameImagesResponse {
    data: GameImagesData,
}

#[derive(Deserialize)]
struct GameImagesData {
    base_url: ImageBaseUrls,
    #[serde(default)]
    images: HashMap<String, Vec<GameImage>>,
}

#[derive(Deserialize)]
struct ImageBaseUrls {
    original: String,
    large: String,
}

#[derive(Deserialize)]
struct GameImage {
    id: u64,
    #[serde(rename = "type")]
    image_type: String,
    filename: String,
    resolution: Option<String>,
}

/// Maps only safe landscape raster paths with real dimensions from the provider response.
fn map_images(
    data: GameImagesData,
    game_titles: &HashMap<String, String>,
    query: &WallpaperQuery,
) -> Vec<RemoteWallpaper> {
    if !valid_base_url(&data.base_url.original) || !valid_base_url(&data.base_url.large) {
        return Vec::new();
    }
    let mut mapped = Vec::new();
    for (game_id, images) in data.images {
        let Some(game_title) = game_titles.get(&game_id) else {
            continue;
        };
        for image in images {
            let Some((width, height)) = image.resolution.as_deref().and_then(parse_resolution)
            else {
                continue;
            };
            if width < query.min_width
                || height < query.min_height
                || width <= height
                || !valid_image_path(&image.filename)
            {
                continue;
            }
            let Some(mime_type) = mime_type(&image.filename) else {
                continue;
            };
            mapped.push(RemoteWallpaper {
                remote_id: format!("{game_id}:{}", image.id),
                provider: "thegamesdb".into(),
                name: format!("{game_title} · {}", image.image_type),
                source_page_url: Some(format!("https://thegamesdb.net/game.php?id={game_id}")),
                original_url: Some(format!("{}{}", data.base_url.original, image.filename)),
                thumbnail_url: Some(format!("{}{}", data.base_url.large, image.filename)),
                local_path: None,
                width: Some(width),
                height: Some(height),
                resolution: Some(format!("{width}x{height}")),
                ratio: Some(reduced_ratio(width, height)),
                file_size: None,
                mime_type: Some(mime_type.into()),
                category: "games".into(),
                purity: "sfw".into(),
                tags: vec![game_title.clone(), "game".into(), image.image_type],
                created_at: None,
                author: None,
                license_name: None,
                license_url: None,
                perceptual_hash: None,
            });
        }
    }
    mapped.truncate(usize::try_from(query.page_size).unwrap_or(24));
    mapped
}

/// Accepts only HTTPS CDN bases with a trailing slash before joining provider paths.
fn valid_base_url(value: &str) -> bool {
    value.starts_with("https://") && value.ends_with('/')
}

/// Prevents path traversal and limits downloads to image formats decoded by the application.
fn valid_image_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains("..")
        && mime_type(value).is_some()
}

/// Infers the expected raster MIME type from a provider-controlled relative path.
fn mime_type(path: &str) -> Option<&'static str> {
    let path = path.split('?').next()?.to_ascii_lowercase();
    if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if path.ends_with(".png") {
        Some("image/png")
    } else if path.ends_with(".webp") {
        Some("image/webp")
    } else {
        None
    }
}

/// Parses the documented WIDTHxHEIGHT value without accepting zero-sized images.
fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let normalized = value.to_ascii_lowercase();
    let (width, height) = normalized.split_once('x')?;
    let width = width.trim().parse().ok()?;
    let height = height.trim().parse().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

/// Validates the composite game/image identifier used by detail and download operations.
fn parse_remote_id(value: &str) -> AppResult<(u64, u64)> {
    let (game_id, image_id) = value
        .split_once(':')
        .ok_or_else(|| AppError::Provider("invalid TheGamesDB image id".into()))?;
    let game_id = game_id
        .parse()
        .map_err(|_| AppError::Provider("invalid TheGamesDB game id".into()))?;
    let image_id = image_id
        .parse()
        .map_err(|_| AppError::Provider("invalid TheGamesDB image id".into()))?;
    Ok((game_id, image_id))
}

/// Produces a stable exact aspect label for the unified metadata model.
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
    use std::collections::HashMap;

    use super::{
        GameImage, GameImagesData, ImageBaseUrls, TheGamesDbProvider, map_images, parse_remote_id,
        parse_resolution,
    };
    use crate::provider::WallpaperQuery;

    #[test]
    fn maps_only_large_landscape_game_art() {
        let data = GameImagesData {
            base_url: ImageBaseUrls {
                original: "https://cdn.example.test/original/".into(),
                large: "https://cdn.example.test/large/".into(),
            },
            images: HashMap::from([(
                "42".into(),
                vec![
                    GameImage {
                        id: 7,
                        image_type: "fanart".into(),
                        filename: "fanart/42-1.jpg".into(),
                        resolution: Some("3840x2160".into()),
                    },
                    GameImage {
                        id: 8,
                        image_type: "boxart".into(),
                        filename: "../secret.jpg".into(),
                        resolution: Some("4000x6000".into()),
                    },
                ],
            )]),
        };
        let titles = HashMap::from([("42".into(), "Example Game".into())]);
        let mapped = map_images(data, &titles, &WallpaperQuery::default());
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].provider, "thegamesdb");
        assert_eq!(mapped[0].category, "games");
        assert_eq!(mapped[0].remote_id, "42:7");
    }

    #[test]
    fn validates_dimensions_ids_and_runtime_key() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse_resolution("3840x2160"), Some((3840, 2160)));
        assert!(parse_remote_id("42:7").is_ok());
        assert!(parse_remote_id("42/7").is_err());
        let directory = tempfile::tempdir()?;
        let provider = TheGamesDbProvider::new(directory.path().to_path_buf())?;
        assert!(provider.api_key().is_err());
        provider.set_api_key(Some(" user-key "))?;
        assert_eq!(provider.api_key()?, "user-key");
        Ok(())
    }
}
