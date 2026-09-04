use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use futures_util::{StreamExt, future::join_all, stream};
use reqwest::Client;
use sha2::{Digest, Sha256};

use crate::{
    db::Database,
    error::{AppError, AppResult},
    image_processing::perceptual_hash_bytes,
};

use super::{ProviderServices, RemoteWallpaper, WallpaperProvider, WallpaperQuery};

static THUMBNAIL_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(3);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Partial-success result used to keep one provider failure isolated from other sources.
pub struct AggregatedProviderResult {
    pub wallpapers: Vec<RemoteWallpaper>,
    pub failures: Vec<(String, String)>,
}

/// Queries all enabled online adapters concurrently and fairly interleaves their results.
#[derive(Clone)]
pub struct AggregatedProviderService {
    database: Database,
    providers: ProviderServices,
    client: Client,
    thumbnail_directory: PathBuf,
}

impl AggregatedProviderService {
    /// Shares the built-in registry and persisted provider configuration.
    pub fn new(
        database: Database,
        providers: ProviderServices,
        thumbnail_directory: PathBuf,
    ) -> Self {
        Self {
            database,
            providers,
            client: Client::new(),
            thumbnail_directory,
        }
    }

    /// Searches every enabled compatible provider with independent failure reporting.
    pub async fn search(&self, query: WallpaperQuery) -> AppResult<AggregatedProviderResult> {
        let enabled: HashSet<_> = self
            .database
            .enabled_online_providers()?
            .into_iter()
            .collect();
        let requested = query.providers.as_ref().map(|providers| {
            providers
                .iter()
                .map(|provider| provider.to_ascii_lowercase())
                .collect::<HashSet<_>>()
        });
        let providers: Vec<Arc<dyn WallpaperProvider>> = self
            .providers
            .online()
            .into_iter()
            .filter(|provider| enabled.contains(provider.provider_name()))
            .filter(|provider| {
                requested
                    .as_ref()
                    .is_none_or(|scope| scope.contains(provider.provider_name()))
            })
            .collect();
        if providers.is_empty() {
            return Ok(AggregatedProviderResult {
                wallpapers: Vec::new(),
                failures: Vec::new(),
            });
        }

        let tasks = providers.into_iter().map(|provider| {
            let query = query.clone();
            async move {
                let name = provider.provider_name().to_owned();
                let started = Instant::now();
                let result = provider.search(query).await;
                (name, started.elapsed().as_millis(), result)
            }
        });
        let mut successful = Vec::new();
        let mut failures = Vec::new();
        for (provider, elapsed_ms, result) in join_all(tasks).await {
            match result {
                Ok(wallpapers) => {
                    self.database
                        .record_provider_health(&provider, elapsed_ms, None)?;
                    successful.push(wallpapers);
                }
                Err(error) => {
                    let message = error.to_string();
                    self.database
                        .record_provider_health(&provider, elapsed_ms, Some(&message))?;
                    failures.push((provider, message));
                }
            }
        }
        if successful.is_empty() && !failures.is_empty() {
            return Err(AppError::Provider(format!(
                "all enabled providers failed: {}",
                failures
                    .iter()
                    .map(|(provider, error)| format!("{provider}: {error}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }

        let wallpapers = self
            .attach_thumbnail_hashes(fair_interleave(successful))
            .await;
        Ok(AggregatedProviderResult {
            wallpapers,
            failures,
        })
    }

    /// Adds best-effort thumbnail fingerprints with bounded parallelism and response size.
    async fn attach_thumbnail_hashes(
        &self,
        wallpapers: Vec<RemoteWallpaper>,
    ) -> Vec<RemoteWallpaper> {
        let client = self.client.clone();
        let database = self.database.clone();
        let thumbnail_directory = self.thumbnail_directory.clone();
        let mut results = stream::iter(wallpapers.into_iter().enumerate().map(
            move |(position, mut wallpaper)| {
                let client = client.clone();
                let database = database.clone();
                let thumbnail_directory = thumbnail_directory.clone();
                async move {
                    if let Some(url) = wallpaper.thumbnail_url.as_deref() {
                        match cache_thumbnail(&client, &thumbnail_directory, url).await {
                            Ok((path, hash)) => {
                                wallpaper.thumbnail_local_path = Some(path);
                                wallpaper.perceptual_hash = Some(hash);
                            }
                            Err(error) => {
                                if matches!(error, AppError::Provider(_) | AppError::Image(_)) {
                                    if let Err(db_error) = database.mark_thumbnail_failed(&wallpaper.provider, &wallpaper.remote_id) {
                                        tracing::warn!(%db_error, "could not record thumbnail quarantine");
                                    }
                                }
                                tracing::warn!(
                                provider = wallpaper.provider,
                                remote_id = wallpaper.remote_id,
                                %error,
                                "thumbnail unavailable; retaining metadata without deleting user state"
                                );
                            }
                        }
                    }
                    (position, wallpaper)
                }
            },
        ))
        .buffer_unordered(3)
        .collect::<Vec<_>>()
        .await;
        results.sort_unstable_by_key(|(position, _)| *position);
        results
            .into_iter()
            // Do not import unusable new cards; later successful refreshes can restore them.
            .filter(|(_, wallpaper)| wallpaper.thumbnail_local_path.is_some())
            .map(|(_, wallpaper)| wallpaper)
            .collect()
    }
}

/// Reuses a validated local thumbnail before networking; all callers share three download slots.
pub(crate) async fn cache_thumbnail(
    client: &Client,
    directory: &Path,
    url: &str,
) -> AppResult<(PathBuf, String)> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| AppError::Provider("invalid thumbnail URL".into()))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Provider("thumbnail URLs must use HTTPS".into()));
    }
    let _slot = THUMBNAIL_SLOTS
        .acquire()
        .await
        .map_err(|_| AppError::Image("thumbnail queue is closed".into()))?;
    let key = format!("{:x}", Sha256::digest(url.as_bytes()));
    for extension in ["jpg", "png", "webp"] {
        let path = directory.join(format!("online-{key}.{extension}"));
        if let Ok(metadata) = tokio::fs::metadata(&path).await
            && metadata.len() <= 8 * 1024 * 1024
            && let Ok(bytes) = tokio::fs::read(&path).await
            && let Ok(hash) = perceptual_hash_bytes(&bytes)
        {
            return Ok((path, hash));
        }
    }
    // A transport failure gets one retry; access-denied responses must not be hammered.
    let bytes = match download_thumbnail(client, url).await {
        Ok(bytes) => bytes,
        Err(AppError::Network(_)) => {
            tokio::time::sleep(Duration::from_millis(300)).await;
            download_thumbnail(client, url).await?
        }
        Err(error) => return Err(error),
    };
    let hash = perceptual_hash_bytes(&bytes)?;
    let path = persist_thumbnail(directory, url, &bytes).await?;
    Ok((path, hash))
}

/// Persists validated provider bytes atomically under a URL-derived stable cache key.
async fn persist_thumbnail(
    directory: &std::path::Path,
    url: &str,
    bytes: &[u8],
) -> AppResult<PathBuf> {
    let format = image::guess_format(bytes)?;
    let extension = match format {
        image::ImageFormat::Jpeg => "jpg",
        image::ImageFormat::Png => "png",
        image::ImageFormat::WebP => "webp",
        _ => {
            return Err(AppError::Image(
                "unsupported provider thumbnail format".into(),
            ));
        }
    };
    let key = format!("{:x}", Sha256::digest(url.as_bytes()));
    let target = directory.join(format!("online-{key}.{extension}"));
    tokio::fs::create_dir_all(directory).await?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = directory.join(format!(
        "online-{key}-{}-{sequence}.tmp",
        std::process::id()
    ));
    tokio::fs::write(&temporary, bytes).await?;
    if let Err(error) = tokio::fs::rename(&temporary, &target).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    Ok(target)
}

/// Streams at most 8 MiB so a provider cannot force an unbounded thumbnail allocation.
async fn download_thumbnail(client: &Client, url: &str) -> AppResult<Vec<u8>> {
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    let response = client
        .get(url)
        .timeout(Duration::from_secs(8))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(AppError::Provider(format!(
            "thumbnail server returned HTTP {}",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err(AppError::Provider(
            "provider thumbnail exceeds 8 MiB".into(),
        ));
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > MAX_BYTES {
            return Err(AppError::Provider(
                "provider thumbnail exceeds 8 MiB".into(),
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Interleaves one result at a time from each source so a large provider cannot own page one.
fn fair_interleave(mut sources: Vec<Vec<RemoteWallpaper>>) -> Vec<RemoteWallpaper> {
    let total = sources.iter().map(Vec::len).sum();
    let mut merged = Vec::with_capacity(total);
    let mut positions = vec![0_usize; sources.len()];
    loop {
        let mut added = false;
        for (index, source) in sources.iter_mut().enumerate() {
            if positions[index] < source.len() {
                merged.push(source[positions[index]].clone());
                positions[index] += 1;
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{cache_thumbnail, fair_interleave, persist_thumbnail};
    use crate::provider::RemoteWallpaper;

    /// Creates provider-tagged fixtures without involving network adapters.
    fn wallpaper(provider: &str, id: &str) -> RemoteWallpaper {
        RemoteWallpaper {
            remote_id: id.into(),
            provider: provider.into(),
            name: id.into(),
            source_page_url: None,
            original_url: None,
            thumbnail_url: None,
            thumbnail_local_path: None,
            local_path: None,
            width: Some(3840),
            height: Some(2160),
            resolution: Some("3840x2160".into()),
            ratio: Some("16:9".into()),
            file_size: None,
            mime_type: Some("image/jpeg".into()),
            category: "nature".into(),
            purity: "sfw".into(),
            tags: Vec::new(),
            created_at: None,
            author: None,
            license_name: None,
            license_url: None,
            perceptual_hash: None,
        }
    }

    #[test]
    fn fairly_interleaves_uneven_provider_pages() {
        let merged = fair_interleave(vec![
            vec![
                wallpaper("a", "a1"),
                wallpaper("a", "a2"),
                wallpaper("a", "a3"),
            ],
            vec![wallpaper("b", "b1")],
        ]);
        let ids: Vec<_> = merged
            .iter()
            .map(|wallpaper| wallpaper.remote_id.as_str())
            .collect();
        assert_eq!(ids, ["a1", "b1", "a2", "a3"]);
    }

    /// A cached image must remain usable when its remote host is offline or blocked.
    #[tokio::test]
    async fn reuses_valid_cache_without_network_and_rejects_html()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let url = "https://thumbnail.invalid/image.png";
        let mut output = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(32, 18).write_to(&mut output, image::ImageFormat::Png)?;
        let bytes = output.into_inner();
        let path = persist_thumbnail(directory.path(), url, &bytes).await?;
        let (cached, hash) =
            cache_thumbnail(&reqwest::Client::new(), directory.path(), url).await?;
        assert_eq!(cached, path);
        assert!(!hash.is_empty());
        assert_eq!(std::fs::read(&cached)?, bytes);
        assert!(
            persist_thumbnail(
                directory.path(),
                "https://thumbnail.invalid/html",
                b"<html>403</html>"
            )
            .await
            .is_err()
        );
        assert_eq!(std::fs::read_dir(directory.path())?.count(), 1);
        assert!(
            cache_thumbnail(
                &reqwest::Client::new(),
                directory.path(),
                "file:///private.png"
            )
            .await
            .is_err()
        );
        Ok(())
    }
}
