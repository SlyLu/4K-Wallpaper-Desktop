use std::{
    fs,
    fs::File,
    io::{BufReader, BufWriter, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use image::{
    DynamicImage, GenericImageView, ImageFormat, RgbImage, Rgba, RgbaImage,
    codecs::jpeg::JpegEncoder,
    imageops::{FilterType, overlay},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = 16_384;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_DECODE_ALLOC: u64 = 512 * 1024 * 1024;
const DEFAULT_THUMBNAIL_WIDTH: u32 = 480;
const DEFAULT_THUMBNAIL_HEIGHT: u32 = 270;
const PROCESSED_JPEG_QUALITY: u8 = 92;
const THUMBNAIL_JPEG_QUALITY: u8 = 84;
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Metadata is derived from decoded content rather than trusting a file extension.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageMetadata {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: String,
    pub file_size: u64,
    pub mime_type: &'static str,
    pub format: &'static str,
    pub sha256: String,
}

/// V1 display adaptation modes defined by the product requirements.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FitMode {
    Fill,
    Fit,
    Center,
    Stretch,
}

impl FitMode {
    /// Stable cache slug prevents display labels from becoming filesystem contracts.
    fn slug(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Fit => "fit",
            Self::Center => "center",
            Self::Stretch => "stretch",
        }
    }

    /// Returns the persisted V1 value used by scheduler configuration and cache keys.
    pub fn as_str(self) -> &'static str {
        self.slug()
    }
}

impl TryFrom<&str> for FitMode {
    type Error = AppError;

    /// Parses persisted scheduler values through one stable compatibility boundary.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "fill" => Ok(Self::Fill),
            "fit" => Ok(Self::Fit),
            "center" => Ok(Self::Center),
            "stretch" => Ok(Self::Stretch),
            _ => Err(AppError::Configuration(format!(
                "unsupported fit mode: {value}"
            ))),
        }
    }
}

/// Result returned by thumbnail and monitor adaptation operations.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessedImage {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub source_sha256: String,
    pub cache_hit: bool,
}

/// Pure geometry plan kept separate from pixel work for exhaustive unit testing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AdaptationPlan {
    resized_width: u32,
    resized_height: u32,
    crop_x: u32,
    crop_y: u32,
    crop_width: u32,
    crop_height: u32,
    destination_x: u32,
    destination_y: u32,
}

/// Owns writable cache roots while all input paths remain immutable.
#[derive(Clone)]
pub struct ImageProcessor {
    thumbnail_directory: PathBuf,
    processed_directory: PathBuf,
}

impl ImageProcessor {
    /// Creates the processor without touching disk; AppPaths owns directory initialization.
    pub fn new(thumbnail_directory: PathBuf, processed_directory: PathBuf) -> Self {
        Self {
            thumbnail_directory,
            processed_directory,
        }
    }

    /// Fully decodes an image under limits and returns content-derived metadata.
    pub fn inspect(&self, path: &Path) -> AppResult<ImageMetadata> {
        inspect_image(path)
    }

    /// Produces a proportional JPEG thumbnail and reuses it by source hash and dimensions.
    pub fn create_thumbnail(
        &self,
        path: &Path,
        max_width: Option<u32>,
        max_height: Option<u32>,
    ) -> AppResult<ProcessedImage> {
        let width = max_width.unwrap_or(DEFAULT_THUMBNAIL_WIDTH);
        let height = max_height.unwrap_or(DEFAULT_THUMBNAIL_HEIGHT);
        validate_output_dimensions(width, height)?;
        let (image, metadata) = load_validated_image(path)?;
        let thumbnail = image.thumbnail(width, height);
        let target = self.thumbnail_directory.join(format!(
            "generated-{}-{}x{}.jpg",
            metadata.sha256, width, height
        ));
        let cache_hit = target.is_file();
        if !cache_hit {
            fs::create_dir_all(&self.thumbnail_directory)?;
            let rgb = flatten_on_black(&thumbnail);
            write_jpeg_atomically(&rgb, &target, THUMBNAIL_JPEG_QUALITY)?;
        }
        let (output_width, output_height) = thumbnail.dimensions();
        Ok(ProcessedImage {
            path: target.display().to_string(),
            width: output_width,
            height: output_height,
            source_sha256: metadata.sha256,
            cache_hit,
        })
    }

    /// Generates one display-sized JPEG in processed cache without modifying the source image.
    pub fn prepare_for_display(
        &self,
        path: &Path,
        target_width: u32,
        target_height: u32,
        mode: FitMode,
    ) -> AppResult<ProcessedImage> {
        validate_output_dimensions(target_width, target_height)?;
        let (image, metadata) = load_validated_image(path)?;
        let target = self.processed_directory.join(format!(
            "{}-{}x{}-{}.jpg",
            metadata.sha256,
            target_width,
            target_height,
            mode.slug()
        ));
        let cache_hit = target.is_file();
        if !cache_hit {
            fs::create_dir_all(&self.processed_directory)?;
            let plan = calculate_adaptation_plan(
                metadata.width,
                metadata.height,
                target_width,
                target_height,
                mode,
            )?;
            let rendered = render_adaptation(&image, target_width, target_height, plan);
            write_jpeg_atomically(&rendered, &target, PROCESSED_JPEG_QUALITY)?;
        }
        Ok(ProcessedImage {
            path: target.display().to_string(),
            width: target_width,
            height: target_height,
            source_sha256: metadata.sha256,
            cache_hit,
        })
    }
}

/// Public Core entry point reused by LocalProvider so format and safety rules stay identical.
pub fn inspect_image(path: &Path) -> AppResult<ImageMetadata> {
    let (_, metadata) = load_validated_image(path)?;
    Ok(metadata)
}

/// Opens and decodes one supported image with strict file, dimension, and allocation limits.
fn load_validated_image(path: &Path) -> AppResult<(DynamicImage, ImageMetadata)> {
    let file_metadata = fs::metadata(path)?;
    if !file_metadata.is_file() {
        return Err(AppError::Image(format!(
            "image path is not a file: {}",
            path.display()
        )));
    }
    if file_metadata.len() == 0 || file_metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::Image(format!(
            "image file size must be between 1 byte and {MAX_FILE_BYTES} bytes"
        )));
    }
    let file = File::open(path)?;
    let mut reader = image::ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    let format = reader
        .format()
        .ok_or_else(|| AppError::Image("unable to identify image format".into()))?;
    let (format_name, mime_type) = supported_format(format)?;
    reader.limits(image_limits());
    let image = reader.decode()?;
    let (width, height) = image.dimensions();
    validate_source_dimensions(width, height)?;
    let sha256 = hash_file(path)?;
    Ok((
        image,
        ImageMetadata {
            width,
            height,
            aspect_ratio: reduced_aspect_ratio(width, height),
            file_size: file_metadata.len(),
            mime_type,
            format: format_name,
            sha256,
        },
    ))
}

/// Limits are applied before decode to constrain decompression-bomb resource usage.
fn image_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

/// Restricts decoders to V1 formats even if the image crate supports additional codecs later.
fn supported_format(format: ImageFormat) -> AppResult<(&'static str, &'static str)> {
    match format {
        ImageFormat::Jpeg => Ok(("jpeg", "image/jpeg")),
        ImageFormat::Png => Ok(("png", "image/png")),
        ImageFormat::WebP => Ok(("webp", "image/webp")),
        ImageFormat::Bmp => Ok(("bmp", "image/bmp")),
        _ => Err(AppError::Image(format!(
            "unsupported V1 image format: {format:?}"
        ))),
    }
}

/// Enforces an aggregate pixel ceiling in addition to decoder width and height limits.
fn validate_source_dimensions(width: u32, height: u32) -> AppResult<()> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_SIDE
        || height > MAX_IMAGE_SIDE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(AppError::Image(format!(
            "image dimensions exceed the V1 safety limit: {width}x{height}"
        )));
    }
    Ok(())
}

/// Prevents invalid display sizes and oversized output allocations.
fn validate_output_dimensions(width: u32, height: u32) -> AppResult<()> {
    if width == 0
        || height == 0
        || width > MAX_IMAGE_SIDE
        || height > MAX_IMAGE_SIDE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
    {
        return Err(AppError::Image(format!(
            "invalid output dimensions: {width}x{height}"
        )));
    }
    Ok(())
}

/// Streams SHA-256 calculation so large 8K files are not duplicated in memory.
fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Uses the greatest common divisor so ratios are stable for arbitrary monitor sizes.
fn reduced_aspect_ratio(width: u32, height: u32) -> String {
    let divisor = greatest_common_divisor(width, height);
    format!("{}:{}", width / divisor, height / divisor)
}

/// Euclidean GCD avoids floating-point ratio labels.
fn greatest_common_divisor(mut left: u32, mut right: u32) -> u32 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left.max(1)
}

/// Calculates all scaling, crop, and placement geometry without decoding pixels.
fn calculate_adaptation_plan(
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
    mode: FitMode,
) -> AppResult<AdaptationPlan> {
    validate_source_dimensions(source_width, source_height)?;
    validate_output_dimensions(target_width, target_height)?;
    let source_ratio_cross = u64::from(source_width) * u64::from(target_height);
    let target_ratio_cross = u64::from(target_width) * u64::from(source_height);

    let plan = match mode {
        FitMode::Fill => {
            let (resized_width, resized_height) = if target_ratio_cross >= source_ratio_cross {
                (
                    target_width,
                    mul_div_ceil(source_height, target_width, source_width)?,
                )
            } else {
                (
                    mul_div_ceil(source_width, target_height, source_height)?,
                    target_height,
                )
            };
            AdaptationPlan {
                resized_width,
                resized_height,
                crop_x: resized_width.saturating_sub(target_width) / 2,
                crop_y: resized_height.saturating_sub(target_height) / 2,
                crop_width: target_width,
                crop_height: target_height,
                destination_x: 0,
                destination_y: 0,
            }
        }
        FitMode::Fit => {
            let (resized_width, resized_height) = if target_ratio_cross >= source_ratio_cross {
                (
                    mul_div_floor(source_width, target_height, source_height)?,
                    target_height,
                )
            } else {
                (
                    target_width,
                    mul_div_floor(source_height, target_width, source_width)?,
                )
            };
            AdaptationPlan {
                resized_width,
                resized_height,
                crop_x: 0,
                crop_y: 0,
                crop_width: resized_width,
                crop_height: resized_height,
                destination_x: target_width.saturating_sub(resized_width) / 2,
                destination_y: target_height.saturating_sub(resized_height) / 2,
            }
        }
        FitMode::Center => {
            let visible_width = source_width.min(target_width);
            let visible_height = source_height.min(target_height);
            AdaptationPlan {
                resized_width: source_width,
                resized_height: source_height,
                crop_x: source_width.saturating_sub(visible_width) / 2,
                crop_y: source_height.saturating_sub(visible_height) / 2,
                crop_width: visible_width,
                crop_height: visible_height,
                destination_x: target_width.saturating_sub(visible_width) / 2,
                destination_y: target_height.saturating_sub(visible_height) / 2,
            }
        }
        FitMode::Stretch => AdaptationPlan {
            resized_width: target_width,
            resized_height: target_height,
            crop_x: 0,
            crop_y: 0,
            crop_width: target_width,
            crop_height: target_height,
            destination_x: 0,
            destination_y: 0,
        },
    };
    Ok(plan)
}

/// Integer ceiling division guarantees Fill never leaves a one-pixel gap.
fn mul_div_ceil(value: u32, multiplier: u32, divisor: u32) -> AppResult<u32> {
    let numerator = u64::from(value) * u64::from(multiplier);
    let result = numerator.div_ceil(u64::from(divisor));
    u32::try_from(result.max(1))
        .map_err(|_| AppError::Image("calculated image dimension overflowed".into()))
}

/// Integer floor division keeps Fit within the target canvas.
fn mul_div_floor(value: u32, multiplier: u32, divisor: u32) -> AppResult<u32> {
    let result = (u64::from(value) * u64::from(multiplier)) / u64::from(divisor);
    u32::try_from(result.max(1))
        .map_err(|_| AppError::Image("calculated image dimension overflowed".into()))
}

/// Executes a tested plan and composites transparency or letterboxing onto opaque black.
fn render_adaptation(
    source: &DynamicImage,
    target_width: u32,
    target_height: u32,
    plan: AdaptationPlan,
) -> RgbImage {
    let resized = if source.dimensions() == (plan.resized_width, plan.resized_height) {
        source.clone()
    } else {
        source.resize_exact(
            plan.resized_width,
            plan.resized_height,
            FilterType::Lanczos3,
        )
    };
    let visible = resized.crop_imm(plan.crop_x, plan.crop_y, plan.crop_width, plan.crop_height);
    let mut canvas = RgbaImage::from_pixel(target_width, target_height, Rgba([0, 0, 0, 255]));
    overlay(
        &mut canvas,
        &visible.to_rgba8(),
        i64::from(plan.destination_x),
        i64::from(plan.destination_y),
    );
    DynamicImage::ImageRgba8(canvas).to_rgb8()
}

/// Flattens transparent thumbnails onto black for deterministic JPEG output.
fn flatten_on_black(image: &DynamicImage) -> RgbImage {
    let (width, height) = image.dimensions();
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    overlay(&mut canvas, &image.to_rgba8(), 0, 0);
    DynamicImage::ImageRgba8(canvas).to_rgb8()
}

/// Encodes beside the target and atomically renames only after a complete successful write.
fn write_jpeg_atomically(image: &RgbImage, target: &Path, quality: u8) -> AppResult<()> {
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = target.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
    let write_result = (|| -> AppResult<()> {
        let file = File::create(&temporary)?;
        let mut encoder = JpegEncoder::new_with_quality(BufWriter::new(file), quality);
        encoder.encode_image(image)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if target.is_file() {
        fs::remove_file(&temporary)?;
        return Ok(());
    }
    fs::rename(&temporary, target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

    use super::{
        FitMode, ImageProcessor, calculate_adaptation_plan, inspect_image, reduced_aspect_ratio,
    };

    #[test]
    fn calculates_fill_and_fit_for_ultrawide_display() -> Result<(), Box<dyn std::error::Error>> {
        let fill = calculate_adaptation_plan(3840, 2160, 3440, 1440, FitMode::Fill)?;
        assert_eq!((fill.resized_width, fill.resized_height), (3440, 1935));
        assert_eq!((fill.crop_x, fill.crop_y), (0, 247));
        let fit = calculate_adaptation_plan(3840, 2160, 3440, 1440, FitMode::Fit)?;
        assert_eq!((fit.resized_width, fit.resized_height), (2560, 1440));
        assert_eq!((fit.destination_x, fit.destination_y), (440, 0));
        Ok(())
    }

    #[test]
    fn calculates_center_and_stretch_without_implicit_center_scaling()
    -> Result<(), Box<dyn std::error::Error>> {
        let center = calculate_adaptation_plan(3000, 1000, 1920, 1080, FitMode::Center)?;
        assert_eq!((center.resized_width, center.resized_height), (3000, 1000));
        assert_eq!((center.crop_x, center.destination_y), (540, 40));
        let stretch = calculate_adaptation_plan(800, 1200, 1920, 1080, FitMode::Stretch)?;
        assert_eq!(
            (stretch.resized_width, stretch.resized_height),
            (1920, 1080)
        );
        Ok(())
    }

    #[test]
    fn reports_reduced_ratios_for_required_shapes() {
        assert_eq!(reduced_aspect_ratio(1920, 1080), "16:9");
        assert_eq!(reduced_aspect_ratio(2560, 1600), "8:5");
        assert_eq!(reduced_aspect_ratio(3440, 1440), "43:18");
        assert_eq!(reduced_aspect_ratio(5120, 1440), "32:9");
    }

    #[test]
    fn plans_every_required_source_shape_for_all_modes() -> Result<(), Box<dyn std::error::Error>> {
        let sources = [
            (1920, 1080),
            (2560, 1440),
            (3840, 2160),
            (5120, 2880),
            (7680, 4320),
            (2560, 1600),
            (3440, 1440),
            (5120, 1440),
            (1080, 1920),
        ];
        for (source_width, source_height) in sources {
            for mode in [
                FitMode::Fill,
                FitMode::Fit,
                FitMode::Center,
                FitMode::Stretch,
            ] {
                let plan =
                    calculate_adaptation_plan(source_width, source_height, 2560, 1600, mode)?;
                assert!(plan.crop_width > 0 && plan.crop_height > 0);
                assert!(plan.crop_x + plan.crop_width <= plan.resized_width);
                assert!(plan.crop_y + plan.crop_height <= plan.resized_height);
                assert!(plan.destination_x + plan.crop_width <= 2560);
                assert!(plan.destination_y + plan.crop_height <= 1600);
            }
        }
        Ok(())
    }

    #[test]
    fn reads_content_format_and_rejects_invalid_images() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let disguised = directory.path().join("actually-png.jpg");
        DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 32, Rgb([10, 20, 30])))
            .save_with_format(&disguised, ImageFormat::Png)?;
        let metadata = inspect_image(&disguised)?;
        assert_eq!(metadata.format, "png");
        assert_eq!(metadata.aspect_ratio, "2:1");

        let invalid = directory.path().join("invalid.jpg");
        std::fs::write(&invalid, b"not an image")?;
        assert!(inspect_image(&invalid).is_err());
        Ok(())
    }

    #[test]
    fn generates_all_modes_thumbnail_and_reuses_cache() -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source.png");
        DynamicImage::ImageRgb8(RgbImage::from_pixel(320, 180, Rgb([30, 100, 200])))
            .save_with_format(&source, ImageFormat::Png)?;
        let original = std::fs::read(&source)?;
        let processor = ImageProcessor::new(
            directory.path().join("thumbnails"),
            directory.path().join("processed"),
        );
        for mode in [
            FitMode::Fill,
            FitMode::Fit,
            FitMode::Center,
            FitMode::Stretch,
        ] {
            let output = processor.prepare_for_display(&source, 200, 120, mode)?;
            assert_eq!(image::image_dimensions(&output.path)?, (200, 120));
        }
        let first = processor.create_thumbnail(&source, Some(160), Some(90))?;
        let second = processor.create_thumbnail(&source, Some(160), Some(90))?;
        assert_eq!((first.width, first.height), (160, 90));
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
        assert_eq!(std::fs::read(&source)?, original);
        Ok(())
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "generates 4K-derived outputs for every currently attached Windows monitor"]
    fn native_monitors_accept_all_modes_from_a_4k_source() -> Result<(), Box<dyn std::error::Error>>
    {
        let directory = tempfile::tempdir()?;
        let source = directory.path().join("source-4k.jpg");
        RgbImage::from_pixel(3840, 2160, Rgb([24, 80, 140])).save(&source)?;
        let processor = ImageProcessor::new(
            directory.path().join("thumbnails"),
            directory.path().join("processed"),
        );
        let services = crate::platform::create_platform_services()?;
        let monitors = services.monitors.get_monitors()?;
        assert!(!monitors.is_empty());
        for monitor in monitors {
            for mode in [
                FitMode::Fill,
                FitMode::Fit,
                FitMode::Center,
                FitMode::Stretch,
            ] {
                let output =
                    processor.prepare_for_display(&source, monitor.width, monitor.height, mode)?;
                assert_eq!(
                    image::image_dimensions(output.path)?,
                    (monitor.width, monitor.height)
                );
            }
        }
        Ok(())
    }
}
