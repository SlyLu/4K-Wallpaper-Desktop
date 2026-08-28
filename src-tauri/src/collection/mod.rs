use crate::{
    db::Database,
    error::{AppError, AppResult},
    models::{CollectionRecord, SmartCollectionRule, WallpaperPage},
};

/// Coordinates collection validation while SQLite owns transactional persistence.
#[derive(Clone)]
pub struct CollectionService {
    database: Database,
}

impl CollectionService {
    /// Creates the service over the same local database used by the wallpaper catalog.
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    /// Returns ordered manual and smart collection summaries.
    pub fn list(&self) -> AppResult<Vec<CollectionRecord>> {
        let mut collections = self.database.list_collections()?;
        for collection in collections.iter_mut().filter(|collection| collection.smart) {
            collection.wallpaper_count = self
                .database
                .query_collection_wallpapers(collection.id, 1, 1)?
                .total;
        }
        Ok(collections)
    }

    /// Creates one named manual collection after enforcing a concise non-empty name.
    pub fn create(&self, name: &str, description: &str) -> AppResult<CollectionRecord> {
        validate_name(name)?;
        self.database
            .create_collection(name.trim(), description.trim())
    }

    /// Updates user-editable collection metadata without changing member files.
    pub fn update(
        &self,
        collection_id: i64,
        name: &str,
        description: &str,
        cover_wallpaper_id: Option<i64>,
        position: i64,
    ) -> AppResult<CollectionRecord> {
        validate_name(name)?;
        self.database.update_collection(
            collection_id,
            name.trim(),
            description.trim(),
            cover_wallpaper_id,
            position.max(0),
        )
    }

    /// Deletes only the collection and membership links, never wallpaper data or files.
    pub fn delete(&self, collection_id: i64) -> AppResult<()> {
        self.database.delete_collection(collection_id)
    }

    /// Adds a bounded unique member set in one transaction.
    pub fn add_wallpapers(&self, collection_id: i64, wallpaper_ids: &[i64]) -> AppResult<usize> {
        self.database
            .add_collection_wallpapers(collection_id, wallpaper_ids)
    }

    /// Removes membership links without mutating the referenced wallpapers.
    pub fn remove_wallpapers(&self, collection_id: i64, wallpaper_ids: &[i64]) -> AppResult<usize> {
        self.database
            .remove_collection_wallpapers(collection_id, wallpaper_ids)
    }

    /// Saves a schema-validated smart rule and returns its first preview page.
    pub fn set_smart_rule(
        &self,
        collection_id: i64,
        rule: &SmartCollectionRule,
    ) -> AppResult<WallpaperPage> {
        if rule.version != 1 {
            return Err(AppError::Configuration(
                "unsupported smart collection rule version".into(),
            ));
        }
        self.database
            .set_smart_collection_rule(collection_id, rule)?;
        self.database.preview_smart_collection(rule, 1, 60)
    }

    /// Evaluates an allow-listed smart rule through the shared parameterized catalog query.
    pub fn preview_smart_rule(
        &self,
        rule: &SmartCollectionRule,
        page: u32,
        page_size: u32,
    ) -> AppResult<WallpaperPage> {
        if rule.version != 1 {
            return Err(AppError::Configuration(
                "unsupported smart collection rule version".into(),
            ));
        }
        self.database
            .preview_smart_collection(rule, page, page_size)
    }

    /// Returns one bounded page from either manual membership or a saved smart rule.
    pub fn wallpapers(
        &self,
        collection_id: i64,
        page: u32,
        page_size: u32,
    ) -> AppResult<WallpaperPage> {
        self.database
            .query_collection_wallpapers(collection_id, page, page_size)
    }

    /// Resolves multiple manual or smart collections into one unique candidate set.
    pub fn resolve_wallpaper_ids(&self, collection_ids: &[i64]) -> AppResult<Vec<i64>> {
        let mut ids = Vec::new();
        for collection_id in collection_ids.iter().copied() {
            let mut page = 1_u32;
            loop {
                let result = self
                    .database
                    .query_collection_wallpapers(collection_id, page, 100)?;
                ids.extend(result.items.into_iter().map(|wallpaper| wallpaper.id));
                if u64::from(page) * u64::from(result.page_size) >= result.total {
                    break;
                }
                page = page.saturating_add(1);
            }
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }
}

/// Keeps collection names useful in compact navigation and prevents empty identifiers.
fn validate_name(name: &str) -> AppResult<()> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Configuration(
            "collection name must contain 1 to 80 characters".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CollectionService;
    use crate::{
        db::Database,
        models::{NewWallpaper, SmartCollectionRule},
    };

    /// Builds one catalog record used to verify collection relationships and smart filters.
    fn wallpaper(remote_id: &str, category: &str) -> NewWallpaper {
        NewWallpaper {
            provider: "wallhaven".into(),
            remote_id: remote_id.into(),
            name: remote_id.into(),
            source_page_url: None,
            original_url: Some(format!("https://example.invalid/{remote_id}.jpg")),
            thumbnail_url: None,
            thumbnail_local_path: None,
            local_path: None,
            width: 3840,
            height: 2160,
            aspect_ratio: Some("16:9".into()),
            file_size: None,
            mime_type: Some("image/jpeg".into()),
            category: category.into(),
            purity: "sfw".into(),
            hash: None,
            perceptual_hash: None,
            download_status: "remote".into(),
            preset: false,
            created_at: None,
            author: None,
            license_name: None,
            license_url: None,
            synced_at: "unix:00000000000000000001".into(),
            tags: vec![category.into()],
        }
    }

    #[test]
    fn manual_and_smart_collections_preserve_wallpaper_records()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let database = Database::open(&directory.path().join("test.db"))?;
        database
            .upsert_wallpapers(&[wallpaper("nature", "nature"), wallpaper("anime", "anime")])?;
        let service = CollectionService::new(database.clone());

        let manual = service.create("工作", "工作屏轮换")?;
        let nature_id = database.search_wallpapers(&Default::default())?.items[0].id;
        assert_eq!(
            service.add_wallpapers(manual.id, &[nature_id, nature_id])?,
            1
        );
        assert_eq!(service.wallpapers(manual.id, 1, 10)?.total, 1);

        let smart = service.create("自然", "自动筛选")?;
        service.set_smart_rule(
            smart.id,
            &SmartCollectionRule {
                version: 1,
                category: Some("nature".into()),
                ..SmartCollectionRule::default()
            },
        )?;
        assert_eq!(service.wallpapers(smart.id, 1, 10)?.total, 1);

        service.delete(manual.id)?;
        assert_eq!(database.search_wallpapers(&Default::default())?.total, 2);
        Ok(())
    }
}
