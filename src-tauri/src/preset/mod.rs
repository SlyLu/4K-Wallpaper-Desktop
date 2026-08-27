use std::{collections::HashSet, fs, path::Path};

use serde::Deserialize;

use crate::{
    db::Database,
    error::{AppError, AppResult},
    models::NewWallpaper,
};

const EXPECTED_PRESET_COUNT: usize = 30;

#[derive(Deserialize)]
struct PresetManifest {
    version: u32,
    source: String,
    wallpapers: Vec<PresetEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresetEntry {
    #[serde(flatten)]
    metadata: NewWallpaper,
    thumbnail_file: String,
}

/// Copies bundled thumbnails into writable AppData and imports metadata idempotently.
pub fn import_bundled_presets(
    database: &Database,
    preset_root: &Path,
    thumbnail_cache: &Path,
) -> AppResult<usize> {
    let manifest_path = preset_root.join("manifest.json");
    let manifest: PresetManifest = serde_json::from_str(&fs::read_to_string(&manifest_path)?)?;
    validate_manifest(&manifest)?;
    fs::create_dir_all(thumbnail_cache)?;

    let mut wallpapers = Vec::with_capacity(manifest.wallpapers.len());
    for mut entry in manifest.wallpapers {
        let source = preset_root.join(&entry.thumbnail_file);
        validate_thumbnail_path(preset_root, &source)?;
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .ok_or_else(|| AppError::Image("preset thumbnail has no extension".into()))?;
        let destination =
            thumbnail_cache.join(format!("preset-{}.{}", entry.metadata.remote_id, extension));
        if !destination.is_file() {
            let temporary = destination.with_extension(format!("{extension}.part"));
            fs::copy(&source, &temporary)?;
            validate_image_signature(&temporary)?;
            fs::rename(&temporary, &destination)?;
        } else {
            validate_image_signature(&destination)?;
        }
        entry.metadata.thumbnail_local_path = Some(destination.display().to_string());
        wallpapers.push(entry.metadata);
    }
    database.upsert_wallpapers(&wallpapers)
}

/// Enforces the selected product promise before any records reach persistence.
fn validate_manifest(manifest: &PresetManifest) -> AppResult<()> {
    if manifest.version != 1 || manifest.source != "Wallhaven API v1" {
        return Err(AppError::Configuration(
            "unsupported preset manifest version or source".into(),
        ));
    }
    if manifest.wallpapers.len() != EXPECTED_PRESET_COUNT {
        return Err(AppError::Configuration(format!(
            "preset manifest must contain exactly {EXPECTED_PRESET_COUNT} wallpapers"
        )));
    }
    let mut ids = HashSet::with_capacity(EXPECTED_PRESET_COUNT);
    for entry in &manifest.wallpapers {
        let metadata = &entry.metadata;
        if metadata.provider != "wallhaven"
            || metadata.width < 3840
            || metadata.height < 2160
            || metadata.purity != "sfw"
            || !metadata.preset
            || !ids.insert(metadata.remote_id.as_str())
        {
            return Err(AppError::Configuration(format!(
                "invalid or duplicate preset wallpaper: {}",
                metadata.remote_id
            )));
        }
    }
    Ok(())
}

/// Prevents a malformed manifest from reading outside the bundled preset directory.
fn validate_thumbnail_path(preset_root: &Path, thumbnail: &Path) -> AppResult<()> {
    let canonical_root = fs::canonicalize(preset_root)?;
    let canonical_thumbnail = fs::canonicalize(thumbnail)?;
    if !canonical_thumbnail.starts_with(canonical_root) || !canonical_thumbnail.is_file() {
        return Err(AppError::Configuration(
            "preset thumbnail escaped the bundled resource directory".into(),
        ));
    }
    Ok(())
}

/// Verifies a small magic signature so cached provider error pages are never shown as images.
fn validate_image_signature(path: &Path) -> AppResult<()> {
    let bytes = fs::read(path)?;
    let jpeg = bytes.starts_with(&[0xFF, 0xD8, 0xFF]);
    let png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    if !jpeg && !png {
        return Err(AppError::Image(format!(
            "invalid preset thumbnail: {}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::import_bundled_presets;
    use crate::db::Database;

    #[test]
    fn imports_exactly_thirty_presets_idempotently() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("presets");
        let thumbnails = directory.path().join("thumbnails");
        assert_eq!(import_bundled_presets(&database, &root, &thumbnails)?, 30);
        assert_eq!(import_bundled_presets(&database, &root, &thumbnails)?, 30);
        let page = database.list_wallpapers(1, 100, true)?;
        assert_eq!(page.total, 30);
        assert!(page.items.iter().all(|wallpaper| {
            wallpaper
                .thumbnail_local_path
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).is_file())
        }));
        Ok(())
    }
}
