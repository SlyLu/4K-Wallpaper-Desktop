use std::{fs, path::PathBuf};

use directories::BaseDirs;

use crate::error::{AppError, AppResult};

const APP_DIRECTORY_NAME: &str = "4K Wallpaper Desktop";

/// All writable paths are derived from the operating system's per-user data location.
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub database_file: PathBuf,
    pub wallpapers_original_dir: PathBuf,
    pub thumbnails_dir: PathBuf,
    pub processed_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
}

impl AppPaths {
    /// Resolves the platform data root using system-known folders without hardcoded users.
    pub fn discover() -> AppResult<Self> {
        let base = BaseDirs::new().ok_or_else(|| {
            AppError::configuration("the operating system did not provide a user data directory")
        })?;
        let root = base.data_local_dir().join(APP_DIRECTORY_NAME);
        let data_dir = root.join("data");
        let config_dir = root.join("config");
        Ok(Self {
            database_file: data_dir.join("wallpaper.db"),
            wallpapers_original_dir: root.join("wallpapers").join("original"),
            thumbnails_dir: root.join("cache").join("thumbnails"),
            processed_dir: root.join("cache").join("processed"),
            logs_dir: root.join("logs"),
            config_file: config_dir.join("settings.json"),
            config_dir,
            data_dir,
            root,
        })
    }

    /// Creates the required data tree idempotently on first and later starts.
    pub fn ensure_directories(&self) -> AppResult<()> {
        for directory in [
            &self.data_dir,
            &self.wallpapers_original_dir,
            &self.thumbnails_dir,
            &self.processed_dir,
            &self.logs_dir,
            &self.config_dir,
        ] {
            fs::create_dir_all(directory)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::AppPaths;

    #[test]
    fn discovered_paths_keep_runtime_data_outside_the_install_directory()
    -> Result<(), Box<dyn std::error::Error>> {
        let paths = AppPaths::discover()?;
        assert!(paths.database_file.starts_with(&paths.root));
        assert!(paths.logs_dir.starts_with(&paths.root));
        assert!(paths.processed_dir.ends_with("processed"));
        Ok(())
    }
}
