use std::{
    fs,
    path::{Path, PathBuf},
};

use async_trait::async_trait;

use crate::{
    error::{AppError, AppResult},
    image_processing::inspect_image,
};

use super::{RemoteWallpaper, WallpaperCategory, WallpaperProvider, WallpaperQuery};

/// Read-only adapter for directories explicitly selected by the user.
pub struct LocalProvider {
    roots: Vec<PathBuf>,
}

impl LocalProvider {
    /// Stores only caller-approved roots; scanning never broadens access beyond them.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// Returns the complete approved-root snapshot for adapter-level tests.
    #[cfg(test)]
    pub async fn scan_all(&self) -> AppResult<Vec<RemoteWallpaper>> {
        self.scan().await
    }

    /// Enumerates supported files without decoding them so unchanged snapshots can be skipped.
    pub async fn discover_files(&self) -> AppResult<Vec<PathBuf>> {
        let roots = self.roots.clone();
        tokio::task::spawn_blocking(move || discover_paths(&roots))
            .await
            .map_err(|error| AppError::Provider(format!("local discovery task failed: {error}")))?
    }

    /// Validates and hashes one changed file through the shared image safety policy.
    pub fn inspect_file(path: &Path) -> AppResult<RemoteWallpaper> {
        read_local_metadata(path)
    }

    /// Performs blocking filesystem traversal away from the async application runtime.
    async fn scan(&self) -> AppResult<Vec<RemoteWallpaper>> {
        let paths = self.discover_files().await?;
        tokio::task::spawn_blocking(move || {
            let mut wallpapers = Vec::with_capacity(paths.len());
            for path in paths {
                match read_local_metadata(&path) {
                    Ok(wallpaper) => wallpapers.push(wallpaper),
                    Err(error) => {
                        tracing::warn!(path = %path.display(), %error, "invalid local image was skipped");
                    }
                }
            }
            Ok(wallpapers)
        })
        .await
        .map_err(|error| AppError::Provider(format!("local scan task failed: {error}")))?
    }

    /// Applies provider-neutral search and bounded pagination to the local index snapshot.
    async fn query(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        if !matches!(
            query.category,
            WallpaperCategory::All | WallpaperCategory::Local
        ) {
            return Ok(Vec::new());
        }
        let keyword = query.keyword.unwrap_or_default().to_ascii_lowercase();
        let mut matches: Vec<_> = self
            .scan()
            .await?
            .into_iter()
            .filter(|wallpaper| {
                keyword.is_empty() || wallpaper.name.to_ascii_lowercase().contains(&keyword)
            })
            .collect();
        matches.sort_by(|left, right| left.name.cmp(&right.name));
        let page_size = query.page_size.clamp(1, 100) as usize;
        let offset = query.page.max(1).saturating_sub(1) as usize * page_size;
        Ok(matches.into_iter().skip(offset).take(page_size).collect())
    }
}

#[async_trait]
impl WallpaperProvider for LocalProvider {
    fn provider_name(&self) -> &'static str {
        "local"
    }

    async fn latest(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn search(&self, query: WallpaperQuery) -> AppResult<Vec<RemoteWallpaper>> {
        self.query(query).await
    }

    async fn get_detail(&self, remote_id: &str) -> AppResult<RemoteWallpaper> {
        self.scan()
            .await?
            .into_iter()
            .find(|wallpaper| wallpaper.remote_id == remote_id)
            .ok_or_else(|| AppError::Provider("local wallpaper no longer exists".into()))
    }

    /// Local images are already downloaded, so this validates and returns their original path.
    async fn download(&self, wallpaper: &RemoteWallpaper) -> AppResult<PathBuf> {
        if wallpaper.provider != self.provider_name() {
            return Err(AppError::Provider(
                "wallpaper does not belong to LocalProvider".into(),
            ));
        }
        let path = wallpaper.local_path.as_ref().ok_or_else(|| {
            AppError::Provider("local wallpaper metadata has no file path".into())
        })?;
        let canonical = fs::canonicalize(path)?;
        let allowed = self.roots.iter().any(|root| {
            fs::canonicalize(root)
                .map(|canonical_root| canonical.starts_with(canonical_root))
                .unwrap_or(false)
        });
        if !allowed || !canonical.is_file() || !is_supported_image(&canonical) {
            return Err(AppError::Provider(
                "local wallpaper is outside the selected directories or unsupported".into(),
            ));
        }
        Ok(canonical)
    }
}

/// Recursively discovers supported files without following directory symlinks.
fn discover_paths(roots: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending: Vec<PathBuf> = roots.to_vec();
    while let Some(path) = pending.pop() {
        if path.is_file() {
            if is_supported_image(&path) {
                files.push(path);
            }
            continue;
        }
        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "local directory was skipped");
                continue;
            }
        };
        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_supported_image(&path) {
                files.push(path);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Reuses the Phase 4 decoder so local dimensions, format, and hash follow one safety policy.
fn read_local_metadata(path: &Path) -> AppResult<RemoteWallpaper> {
    let metadata = inspect_image(path)?;
    let name = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    let file_size = i64::try_from(metadata.file_size)
        .map_err(|_| AppError::FileSystem("local wallpaper is too large to index".into()))?;
    Ok(RemoteWallpaper {
        remote_id: metadata.sha256,
        provider: "local".into(),
        name,
        source_page_url: None,
        original_url: None,
        thumbnail_url: None,
        local_path: Some(path.to_path_buf()),
        width: Some(metadata.width),
        height: Some(metadata.height),
        resolution: Some(format!("{}x{}", metadata.width, metadata.height)),
        ratio: Some(metadata.aspect_ratio),
        file_size: Some(file_size),
        mime_type: Some(metadata.mime_type.to_owned()),
        category: "local".into(),
        purity: "local".into(),
        tags: Vec::new(),
        created_at: None,
        author: None,
        license_name: None,
        license_url: None,
        perceptual_hash: Some(metadata.perceptual_hash),
    })
}

/// Keeps extension policy centralized and case-insensitive.
fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::{LocalProvider, is_supported_image};
    use crate::provider::{WallpaperCategory, WallpaperProvider, WallpaperQuery};

    #[test]
    fn recognizes_supported_extensions_case_insensitively() {
        assert!(is_supported_image(std::path::Path::new("photo.JPEG")));
        assert!(!is_supported_image(std::path::Path::new("notes.txt")));
    }

    #[tokio::test]
    async fn scans_selected_directory_and_hashes_content() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let image = directory.path().join("sample.jpg");
        image::RgbImage::from_pixel(80, 45, image::Rgb([20, 40, 60])).save(&image)?;
        let provider = LocalProvider::new(vec![directory.path().to_path_buf()]);
        let results = provider
            .search(WallpaperQuery {
                category: WallpaperCategory::Local,
                ..WallpaperQuery::default()
            })
            .await?;
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "sample");
        assert_eq!((results[0].width, results[0].height), (Some(80), Some(45)));
        assert_eq!(provider.download(&results[0]).await?, image.canonicalize()?);
        Ok(())
    }

    #[tokio::test]
    async fn imports_one_explicitly_dropped_file() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let image = directory.path().join("dropped.png");
        image::RgbImage::from_pixel(80, 45, image::Rgb([60, 30, 90])).save(&image)?;
        let provider = LocalProvider::new(vec![image.clone()]);

        let results = provider.scan_all().await?;

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "dropped");
        Ok(())
    }
}
