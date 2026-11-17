// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded image decoding, independent of placement and effects.

/// The picture `path` names, a PNG, a JPEG or a WebP file (#1487 F1) or a theme's embedded PNG,
/// decoded within the size limits and scaled to fit 1600 points a side.
pub(super) fn load(path: &str) -> Result<image::RgbaImage, String> {
    let bytes = if path.starts_with("data:image/png;base64,") {
        crate::kit::skin::decode_png(path)?
    } else {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| format!("Background image: {e}"))?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("Background image must be smaller than 32 MiB".into());
        }
        let mut data = Vec::new();
        file.take(32 * 1024 * 1024 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > 32 * 1024 * 1024 {
            return Err("Background image exceeds its size limit".into());
        }
        data
    };
    // Its kind by its first bytes, whatever its name says; nothing but these three is read.
    let format =
        crate::kit::artwork_png::picture_format(&bytes).ok_or("Background image: not a PNG, JPEG or WebP picture")?;
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| format!("Background image: {error}"))?;
    let image = if image.width() > 1600 || image.height() > 1600 { image.thumbnail(1600, 1600) } else { image };
    Ok(image.into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decoding_preserves_small_image_and_rejects_invalid_data() {
        let path = std::env::temp_dir().join(format!("suhina-background-source-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(16, 8, image::Rgba([120, 80, 40, 128])).save(&path).unwrap();
        let image = load(path.to_str().unwrap()).unwrap();
        assert_eq!(image.dimensions(), (16, 8));
        assert_eq!(image.get_pixel(0, 0).0, [120, 80, 40, 128]);
        std::fs::write(&path, b"invalid").unwrap();
        assert!(load(path.to_str().unwrap()).is_err());
        std::fs::remove_file(&path).unwrap();
        assert!(load(path.to_str().unwrap()).is_err());
    }

    /// A JPEG or a WebP photo is read as a PNG is, by its first bytes, whatever its name says
    /// (#1487 F1); another kind of picture is not.
    #[test]
    fn a_jpeg_or_webp_photo_is_read_by_its_bytes() {
        let photo = image::RgbImage::from_pixel(32, 16, image::Rgb([200, 120, 40]));
        for (format, name) in [(image::ImageFormat::Jpeg, "jpg"), (image::ImageFormat::WebP, "png")] {
            let path = std::env::temp_dir().join(format!("suhina-background-photo-{}.{name}", std::process::id()));
            photo.save_with_format(&path, format).unwrap();
            let image = load(path.to_str().unwrap()).unwrap();
            assert_eq!(image.dimensions(), (32, 16), "{format:?}");
            let [r, g, b, a] = image.get_pixel(8, 8).0;
            assert!(r.abs_diff(200) < 8 && g.abs_diff(120) < 8 && b.abs_diff(40) < 8 && a == 255, "{format:?}");
            std::fs::remove_file(&path).unwrap();
        }
        let path = std::env::temp_dir().join(format!("suhina-background-photo-{}.gif", std::process::id()));
        std::fs::write(&path, b"GIF89a\x01\x00\x01\x00").unwrap();
        assert!(load(path.to_str().unwrap()).unwrap_err().contains("not a PNG, JPEG or WebP"));
        std::fs::remove_file(&path).unwrap();
    }
}
