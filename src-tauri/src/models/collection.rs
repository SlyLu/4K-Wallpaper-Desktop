use serde::{Deserialize, Serialize};

/// Persisted collection summary used by navigation and rotation configuration.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub cover_wallpaper_id: Option<i64>,
    pub position: i64,
    pub wallpaper_count: u64,
    pub smart: bool,
}

/// Versioned allow-list of filters for smart collections; arbitrary SQL is never accepted.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SmartCollectionRule {
    pub version: u32,
    pub provider: Option<String>,
    pub category: Option<String>,
    pub favorite: Option<bool>,
    pub file_availability: Option<String>,
    pub min_width: Option<u32>,
    pub min_height: Option<u32>,
    pub aspect_ratio: Option<String>,
    pub tags: Vec<String>,
}
