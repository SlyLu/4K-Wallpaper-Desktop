use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use serde::Serialize;

use crate::{
    db::Database,
    error::{AppError, AppResult},
    paths::AppPaths,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub total_bytes: u64,
    pub limit_bytes: u64,
    pub original_bytes: u64,
    pub thumbnail_bytes: u64,
    pub processed_bytes: u64,
    pub file_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCleanupResult {
    pub before_bytes: u64,
    pub after_bytes: u64,
    pub freed_bytes: u64,
    pub removed_files: usize,
    pub limit_bytes: u64,
}

#[derive(Clone)]
pub struct CacheService {
    database: Database,
    paths: AppPaths,
}

impl CacheService {
    /// Owns only application cache paths and a clone of the synchronized database handle.
    pub fn new(database: Database, paths: AppPaths) -> Self {
        Self { database, paths }
    }

    /// Calculates current cache usage without following symbolic links.
    pub fn info(&self, limit_bytes: u64) -> AppResult<CacheInfo> {
        let originals = collect_files(&self.paths.wallpapers_original_dir)?;
        let thumbnails = collect_files(&self.paths.thumbnails_dir)?;
        let processed = collect_files(&self.paths.processed_dir)?;
        let original_bytes = sum_sizes(&originals);
        let thumbnail_bytes = sum_sizes(&thumbnails);
        let processed_bytes = sum_sizes(&processed);
        Ok(CacheInfo {
            total_bytes: original_bytes
                .saturating_add(thumbnail_bytes)
                .saturating_add(processed_bytes),
            limit_bytes,
            original_bytes,
            thumbnail_bytes,
            processed_bytes,
            file_count: originals.len() + thumbnails.len() + processed.len(),
        })
    }

    /// Enforces a finite limit using processed-first and metadata-backed LRU ordering.
    pub fn enforce_limit(&self, limit_bytes: u64) -> AppResult<CacheCleanupResult> {
        if limit_bytes == 0 {
            let total = self.info(limit_bytes)?.total_bytes;
            return Ok(CacheCleanupResult {
                before_bytes: total,
                after_bytes: total,
                freed_bytes: 0,
                removed_files: 0,
                limit_bytes,
            });
        }
        self.cleanup_to(limit_bytes, limit_bytes)
    }

    /// Clears every removable application cache entry while retaining favorite originals.
    pub fn clear_removable(&self, limit_bytes: u64) -> AppResult<CacheCleanupResult> {
        self.cleanup_to(0, limit_bytes)
    }

    fn cleanup_to(
        &self,
        target_bytes: u64,
        configured_limit: u64,
    ) -> AppResult<CacheCleanupResult> {
        let before = self.info(configured_limit)?.total_bytes;
        let mut current = before;
        let mut removed_files = 0;

        let mut processed = collect_files(&self.paths.processed_dir)?;
        processed.sort_by_key(|file| file.modified_at);
        for file in processed {
            if current <= target_bytes {
                break;
            }
            if remove_owned_file(&self.paths.processed_dir, &file.path)? {
                current = current.saturating_sub(file.size);
                removed_files += 1;
            }
        }

        for path_text in self.database.cache_original_candidates()? {
            if current <= target_bytes {
                break;
            }
            let path = PathBuf::from(&path_text);
            let size = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            if !path.exists() || remove_owned_file(&self.paths.wallpapers_original_dir, &path)? {
                self.database.clear_downloaded_path(&path_text)?;
                current = current.saturating_sub(size);
                if size > 0 {
                    removed_files += 1;
                }
            }
        }

        let mut thumbnails = collect_files(&self.paths.thumbnails_dir)?;
        thumbnails.sort_by_key(|file| file.modified_at);
        for file in thumbnails {
            if current <= target_bytes {
                break;
            }
            let path_text = file.path.display().to_string();
            if remove_owned_file(&self.paths.thumbnails_dir, &file.path)? {
                self.database.clear_thumbnail_path(&path_text)?;
                current = current.saturating_sub(file.size);
                removed_files += 1;
            }
        }

        let after = self.info(configured_limit)?.total_bytes;
        Ok(CacheCleanupResult {
            before_bytes: before,
            after_bytes: after,
            freed_bytes: before.saturating_sub(after),
            removed_files,
            limit_bytes: configured_limit,
        })
    }
}

#[derive(Clone, Debug)]
struct CacheFile {
    path: PathBuf,
    size: u64,
    modified_at: u64,
}

/// Recursively enumerates regular files while never traversing directory symlinks.
fn collect_files(root: &Path) -> AppResult<Vec<CacheFile>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                let metadata = entry.metadata()?;
                let modified_at = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_secs())
                    .unwrap_or(0);
                files.push(CacheFile {
                    path: entry.path(),
                    size: metadata.len(),
                    modified_at,
                });
            }
        }
    }
    Ok(files)
}

fn sum_sizes(files: &[CacheFile]) -> u64 {
    files
        .iter()
        .fold(0, |total, file| total.saturating_add(file.size))
}

/// Deletes a regular file only after its canonical path is proven inside the exact cache root.
fn remove_owned_file(root: &Path, path: &Path) -> AppResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let canonical_root = fs::canonicalize(root)?;
    let canonical_path = fs::canonicalize(path)?;
    if !canonical_path.starts_with(&canonical_root) || !canonical_path.is_file() {
        return Err(AppError::FileSystem(
            "cache file escaped its owned directory".into(),
        ));
    }
    fs::remove_file(canonical_path)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::CacheService;
    use crate::{db::Database, models::NewWallpaper, paths::AppPaths};

    /// Builds an isolated AppData-shaped tree so cleanup boundaries are exercised realistically.
    fn test_paths(root: &std::path::Path) -> AppPaths {
        AppPaths {
            root: root.to_path_buf(),
            data_dir: root.join("data"),
            database_file: root.join("data/wallpaper.db"),
            wallpapers_original_dir: root.join("wallpapers/original"),
            thumbnails_dir: root.join("cache/thumbnails"),
            processed_dir: root.join("cache/processed"),
            logs_dir: root.join("logs"),
            config_dir: root.join("config"),
            config_file: root.join("config/settings.json"),
        }
    }

    fn remote_record(id: &str, path: &std::path::Path) -> NewWallpaper {
        NewWallpaper {
            provider: "wallhaven".into(),
            remote_id: id.into(),
            name: id.into(),
            source_page_url: None,
            original_url: Some(format!("https://example.com/{id}.jpg")),
            thumbnail_url: None,
            thumbnail_local_path: None,
            local_path: Some(path.display().to_string()),
            width: 3840,
            height: 2160,
            aspect_ratio: Some("16:9".into()),
            file_size: Some(8),
            mime_type: Some("image/jpeg".into()),
            category: "nature".into(),
            purity: "sfw".into(),
            hash: Some(format!("hash-{id}")),
            download_status: "downloaded".into(),
            preset: false,
            created_at: None,
            synced_at: "2026-01-01T00:00:00Z".into(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn clear_removable_keeps_favorite_originals_and_user_files()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let paths = test_paths(directory.path());
        paths.ensure_directories()?;
        let database = Database::open(&paths.database_file)?;
        let removable = paths.wallpapers_original_dir.join("removable.jpg");
        let favorite = paths.wallpapers_original_dir.join("favorite.jpg");
        std::fs::write(&removable, b"remote-a")?;
        std::fs::write(&favorite, b"remote-b")?;
        std::fs::write(paths.processed_dir.join("processed.jpg"), b"processed")?;
        database.upsert_wallpapers(&[
            remote_record("removable", &removable),
            remote_record("favorite", &favorite),
        ])?;
        let favorite_id = database
            .search_wallpapers(&crate::models::CatalogQuery {
                keyword: Some("favorite".into()),
                page: 1,
                page_size: 10,
                ..Default::default()
            })?
            .items[0]
            .id;
        database.set_wallpaper_favorite(favorite_id, true)?;

        let result = CacheService::new(database.clone(), paths.clone()).clear_removable(1024)?;
        assert!(result.removed_files >= 2);
        assert!(!removable.exists());
        assert!(favorite.exists());
        assert!(database.get_wallpaper(favorite_id)?.local_path.is_some());
        Ok(())
    }
}
