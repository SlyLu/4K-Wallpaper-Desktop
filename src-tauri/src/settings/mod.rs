use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Phase 0 persists the product defaults that later settings services will edit.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppConfig {
    pub online_provider: String,
    pub minimum_resolution: String,
    pub safety: String,
    pub resource_sync_enabled: bool,
    pub resource_sync_interval_seconds: u64,
    pub wallpaper_auto_change: bool,
    pub wallpaper_change_interval_seconds: u64,
    pub wallpaper_fit_mode: String,
    pub cache_limit_bytes: u64,
    pub close_to_tray: bool,
    pub auto_start: bool,
    pub local_directories: Vec<String>,
    pub theme_mode: String,
    pub theme_effect: String,
    pub theme_accent: String,
    pub theme_secondary: String,
    pub theme_background: String,
    pub theme_surface: String,
    pub theme_pack: String,
    pub theme_background_image: Option<String>,
    pub theme_background_fit: String,
    pub theme_background_overlay: f32,
    pub thegamesdb_api_key: Option<String>,
}

impl Default for AppConfig {
    /// Mirrors the V1 defaults specified in REQUIREMENTS.md section 50.
    fn default() -> Self {
        Self {
            online_provider: "wallhaven".into(),
            minimum_resolution: "3840x2160".into(),
            safety: "SFW".into(),
            resource_sync_enabled: true,
            resource_sync_interval_seconds: 24 * 60 * 60,
            wallpaper_auto_change: false,
            wallpaper_change_interval_seconds: 30 * 60,
            wallpaper_fit_mode: "fill".into(),
            cache_limit_bytes: 5 * 1024 * 1024 * 1024,
            close_to_tray: true,
            auto_start: false,
            local_directories: Vec::new(),
            theme_mode: "dark".into(),
            theme_effect: "solid".into(),
            theme_accent: "#64e8f5".into(),
            theme_secondary: "#4eb2f4".into(),
            theme_background: "#07111d".into(),
            theme_surface: "#0a1b29".into(),
            theme_pack: "classic".into(),
            theme_background_image: None,
            theme_background_fit: "fill".into(),
            theme_background_overlay: 0.35,
            thegamesdb_api_key: None,
        }
    }
}

impl AppConfig {
    /// Reads existing settings or atomically creates the default file on first launch.
    pub fn load_or_create(path: &Path) -> AppResult<Self> {
        if path.exists() {
            return Ok(serde_json::from_slice(&fs::read(path)?)?);
        }

        let config = Self::default();
        let temporary_path = path.with_extension("json.tmp");
        fs::write(&temporary_path, serde_json::to_vec_pretty(&config)?)?;
        fs::rename(temporary_path, path)?;
        Ok(config)
    }

    /// Atomically persists validated settings without leaving a partial JSON file.
    pub fn save(&self, path: &Path) -> AppResult<()> {
        let temporary_path = path.with_extension("json.tmp");
        fs::write(&temporary_path, serde_json::to_vec_pretty(self)?)?;
        fs::rename(temporary_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn defaults_follow_the_v1_baseline() {
        let config = AppConfig::default();
        assert_eq!(config.minimum_resolution, "3840x2160");
        assert_eq!(config.resource_sync_interval_seconds, 86_400);
        assert_eq!(config.cache_limit_bytes, 5 * 1024 * 1024 * 1024);
        assert!(!config.auto_start);
        assert!(config.local_directories.is_empty());
        assert_eq!(config.theme_mode, "dark");
        assert_eq!(config.theme_effect, "solid");
        assert_eq!(config.theme_pack, "classic");
        assert_eq!(config.theme_background_fit, "fill");
    }

    #[test]
    fn legacy_json_receives_theme_defaults() -> Result<(), Box<dyn std::error::Error>> {
        let config: AppConfig = serde_json::from_str("{}")?;
        assert_eq!(config.theme_accent, "#64e8f5");
        assert_eq!(config.theme_background, "#07111d");
        Ok(())
    }
}
