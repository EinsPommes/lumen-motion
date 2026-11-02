#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use thiserror::Error;

/// File extensions accepted by [`load_image`].
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg"];

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("failed to load image {}: {source}", .path.display())]
    Image {
        path: PathBuf,
        #[source]
        source: image::ImageError,
    },
    #[error("image {} has zero size", .0.display())]
    Empty(PathBuf),
}

/// Decoded 8-bit RGBA image (sRGB-encoded, straight alpha).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Reads only the header to get the pixel size.
pub fn image_dimensions(path: &Path) -> Result<(u32, u32), MediaError> {
    let dims = image::image_dimensions(path).map_err(|source| MediaError::Image {
        path: path.to_owned(),
        source,
    })?;
    if dims.0 == 0 || dims.1 == 0 {
        return Err(MediaError::Empty(path.to_owned()));
    }
    Ok(dims)
}

/// Decodes an image, downscaling it if an edge exceeds `max_dim`.
pub fn load_image(path: &Path, max_dim: u32) -> Result<DecodedImage, MediaError> {
    let img = image::open(path).map_err(|source| MediaError::Image {
        path: path.to_owned(),
        source,
    })?;
    let mut rgba = img.into_rgba8();
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 {
        return Err(MediaError::Empty(path.to_owned()));
    }
    let (tw, th) = fit_within(w, h, max_dim.max(1));
    if (tw, th) != (w, h) {
        rgba = image::imageops::resize(&rgba, tw, th, image::imageops::FilterType::Triangle);
    }
    Ok(DecodedImage {
        width: tw,
        height: th,
        rgba: rgba.into_raw(),
    })
}

/// Scales `(w, h)` down proportionally so neither edge exceeds `max_dim`.
pub fn fit_within(w: u32, h: u32, max_dim: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= max_dim {
        return (w, h);
    }
    let scale = f64::from(max_dim) / f64::from(longest);
    let fit = |v: u32| ((f64::from(v) * scale).round() as u32).clamp(1, max_dim);
    (fit(w), fit(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_png(name: &str, w: u32, h: u32) -> PathBuf {
        let dir = std::env::temp_dir().join("lumen-media-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 0, 0, 128]));
        img.save(&path).unwrap();
        path
    }

    #[test]
    fn loads_png_and_reports_dimensions() {
        let path = temp_png("small.png", 4, 3);
        assert_eq!(image_dimensions(&path).unwrap(), (4, 3));
        let img = load_image(&path, 4096).unwrap();
        assert_eq!((img.width, img.height), (4, 3));
        assert_eq!(img.rgba.len(), 4 * 3 * 4);
        assert_eq!(&img.rgba[..4], &[255, 0, 0, 128]);
    }

    #[test]
    fn downscales_large_images() {
        let path = temp_png("wide.png", 400, 100);
        let img = load_image(&path, 100).unwrap();
        assert_eq!((img.width, img.height), (100, 25));
    }

    #[test]
    fn missing_file_is_an_error() {
        let err = load_image(Path::new("does/not/exist.png"), 64).unwrap_err();
        assert!(err.to_string().contains("exist.png"));
    }

    #[test]
    fn fit_within_keeps_aspect() {
        assert_eq!(fit_within(100, 50, 200), (100, 50));
        assert_eq!(fit_within(1000, 10, 100), (100, 1));
        assert_eq!(fit_within(10, 1000, 100), (1, 100));
    }
}
