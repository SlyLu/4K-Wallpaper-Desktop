mod collection;
mod monitor;
mod provider;
mod scheduler;
mod wallpaper;

pub use collection::{CollectionRecord, SmartCollectionRule};
pub use monitor::MonitorInfo;
pub use provider::{ProviderStatus, WallpaperProviderSource};
pub use scheduler::{RotationExplanation, RotationRules, ScheduleRecord};
#[cfg(not(test))]
pub use wallpaper::AppliedWallpaper;
pub use wallpaper::{
    CatalogQuery, DuplicateFileCopy, DuplicateFileGroup, NewWallpaper, WallpaperPage,
    WallpaperRecord,
};
