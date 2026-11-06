// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded image decoding, independent of placement and effects.

pub(super) fn load(path: &str) -> Result<image::RgbaImage, String> {
    let bytes = if path.starts_with("data:image/png;base64,") {
        crate::kit::skin::decode_png(path)?
    } else {
        use std::io::Read;
        let file = std::fs::File::open(path).map_err(|e| format!("Background image: {e}"))?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
            return Err("Background PNG must be smaller than 32 MiB".into());
        }
        let mut data = Vec::new();
        file.take(32 * 1024 * 1024 + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() > 32 * 1024 * 1024 {
            return Err("Background PNG exceeds its size limit".into());
        }
        data
    };
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    reader.set_format(image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| format!("Background PNG: {error}"))?;
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
}
