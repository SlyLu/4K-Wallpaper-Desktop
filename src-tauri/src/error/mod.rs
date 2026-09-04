use serde::Serialize;

pub type AppResult<T> = Result<T, AppError>;

/// Stable error categories exposed to commands and recorded by the Rust core.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("network error: {0}")]
    Network(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("file system error: {0}")]
    FileSystem(String),
    #[error("image error: {0}")]
    Image(String),
    #[error("thumbnail unavailable: {0}")]
    ThumbnailUnavailable(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("wallpaper error: {0}")]
    Wallpaper(String),
    #[error("monitor error: {0}")]
    Monitor(String),
    #[error("configuration error: {0}")]
    Configuration(String),
    #[error("unknown error: {0}")]
    Unknown(String),
}

impl AppError {
    /// Creates a platform error without leaking platform types into callers.
    pub fn platform(message: impl Into<String>) -> Self {
        Self::Platform(message.into())
    }

    /// Creates a monitor-specific error for native enumeration failures.
    pub fn monitor(message: impl Into<String>) -> Self {
        Self::Monitor(message.into())
    }

    /// Creates a configuration error for invalid or unavailable local settings.
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }

    /// Creates an unknown error only when no narrower category is applicable.
    pub fn unknown(message: impl Into<String>) -> Self {
        Self::Unknown(message.into())
    }

    /// Returns the stable machine-readable category for the UI boundary.
    fn code(&self) -> &'static str {
        match self {
            Self::Network(_) => "NETWORK",
            Self::Database(_) => "DATABASE",
            Self::FileSystem(_) => "FILE_SYSTEM",
            Self::Image(_) => "IMAGE",
            Self::ThumbnailUnavailable(_) => "THUMBNAIL_UNAVAILABLE",
            Self::Provider(_) => "PROVIDER",
            Self::Platform(_) => "PLATFORM",
            Self::Wallpaper(_) => "WALLPAPER",
            Self::Monitor(_) => "MONITOR",
            Self::Configuration(_) => "CONFIGURATION",
            Self::Unknown(_) => "UNKNOWN",
        }
    }
}

impl Serialize for AppError {
    /// Serializes errors as a stable object rather than exposing implementation details.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AppError", 2)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<std::io::Error> for AppError {
    /// Maps recoverable local I/O failures into the shared error model.
    fn from(error: std::io::Error) -> Self {
        Self::FileSystem(error.to_string())
    }
}

impl From<rusqlite::Error> for AppError {
    /// Maps SQLite failures into the shared error model.
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    /// Maps malformed local configuration into the shared error model.
    fn from(error: serde_json::Error) -> Self {
        Self::Configuration(error.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    /// Keeps provider transport failures recoverable and distinct from parsing failures.
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error.to_string())
    }
}

impl From<image::ImageError> for AppError {
    /// Converts malformed or unsupported images into the dedicated recoverable category.
    fn from(error: image::ImageError) -> Self {
        Self::Image(error.to_string())
    }
}
