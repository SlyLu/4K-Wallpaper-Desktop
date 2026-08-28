use std::{collections::HashSet, sync::Arc, time::Instant};

use futures_util::{StreamExt, future::join_all, stream};
use reqwest::Client;

use crate::{
    db::Database,
    error::{AppError, AppResult},
    image_processing::perceptual_hash_bytes,
};

use super::{ProviderServices, RemoteWallpaper, WallpaperProvider, WallpaperQuery};

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
}

impl AggregatedProviderService {
    /// Shares the built-in registry and persisted provider configuration.
    pub fn new(database: Database, providers: ProviderServices) -> Self {
        Self {
            database,
            providers,
            client: Client::new(),
        }
    }

    /// Searches every enabled compatible provider with independent failure reporting.
    pub async fn search(&self, query: WallpaperQuery) -> AppResult<AggregatedProviderResult> {
        let enabled: HashSet<_> = self
            .database
            .enabled_online_providers()?
            .into_iter()
            .collect();
        let providers: Vec<Arc<dyn WallpaperProvider>> = self
            .providers
            .online()
            .into_iter()
            .filter(|provider| enabled.contains(provider.provider_name()))
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
        let mut results = stream::iter(wallpapers.into_iter().enumerate().map(
            move |(position, mut wallpaper)| {
                let client = client.clone();
                async move {
                    if let Some(url) = wallpaper.thumbnail_url.as_deref()
                        && let Ok(bytes) = download_thumbnail(&client, url).await
                    {
                        wallpaper.perceptual_hash = perceptual_hash_bytes(&bytes).ok();
                    }
                    (position, wallpaper)
                }
            },
        ))
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;
        results.sort_unstable_by_key(|(position, _)| *position);
        results
            .into_iter()
            .map(|(_, wallpaper)| wallpaper)
            .collect()
    }
}

/// Streams at most 8 MiB so a provider cannot force an unbounded thumbnail allocation.
async fn download_thumbnail(client: &Client, url: &str) -> AppResult<Vec<u8>> {
    const MAX_BYTES: usize = 8 * 1024 * 1024;
    let response = client.get(url).send().await?.error_for_status()?;
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
    use super::fair_interleave;
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
}
