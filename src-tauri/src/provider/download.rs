use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use reqwest::Client;
use tokio::{fs, io::AsyncWriteExt};

use crate::{
    error::{AppError, AppResult},
    image_processing::inspect_image,
};

const MAX_DOWNLOAD_BYTES: u64 = 120 * 1024 * 1024;

/// Downloads one provider original atomically and validates it before it enters AppData.
pub(crate) async fn download_original(
    client: &Client,
    directory: &Path,
    provider: &str,
    remote_id: &str,
    url: &str,
    mime_type: Option<&str>,
) -> AppResult<PathBuf> {
    if !url.starts_with("https://") {
        return Err(AppError::Provider(
            "provider originals must use HTTPS".into(),
        ));
    }
    if !remote_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(AppError::Provider("invalid provider remote id".into()));
    }
    fs::create_dir_all(directory).await?;
    let extension = extension_for(mime_type, url);
    let target = directory.join(format!("{provider}-{remote_id}.{extension}"));
    if target.is_file() {
        inspect_image(&target)?;
        return Ok(target);
    }
    let temporary = target.with_extension(format!("{extension}.part"));
    let response = client.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES)
    {
        return Err(AppError::Provider(
            "provider original exceeds the 120 MiB safety limit".into(),
        ));
    }
    let mut file = fs::File::create(&temporary).await?;
    let mut received = 0_u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        received = received.saturating_add(chunk.len() as u64);
        if received > MAX_DOWNLOAD_BYTES {
            let _ = fs::remove_file(&temporary).await;
            return Err(AppError::Provider(
                "provider original exceeds the 120 MiB safety limit".into(),
            ));
        }
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if let Err(error) = inspect_image(&temporary) {
        let _ = fs::remove_file(&temporary).await;
        return Err(error);
    }
    fs::rename(&temporary, &target).await?;
    Ok(target)
}

/// Uses trusted MIME metadata before falling back to a conservative URL extension check.
fn extension_for(mime_type: Option<&str>, url: &str) -> &'static str {
    match mime_type {
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ if url
            .to_ascii_lowercase()
            .split('?')
            .next()
            .is_some_and(|path| path.ends_with(".png")) =>
        {
            "png"
        }
        _ if url
            .to_ascii_lowercase()
            .split('?')
            .next()
            .is_some_and(|path| path.ends_with(".webp")) =>
        {
            "webp"
        }
        _ => "jpg",
    }
}
