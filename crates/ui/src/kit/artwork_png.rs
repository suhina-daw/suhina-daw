// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded theme PNG artwork; SVG parsing never enables image or external-resource resolvers.
use std::io::Cursor;

fn reader(bytes: &[u8]) -> image::ImageReader<Cursor<&[u8]>> {
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    reader
}

pub fn decode(source: &str) -> Result<image::RgbaImage, String> {
    let bytes = super::skin::decode_png(source)?;
    let (width, height) = reader(&bytes).into_dimensions().map_err(|error| format!("invalid artwork PNG: {error}"))?;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 4 * 1024 * 1024
    {
        return Err("artwork PNG exceeds dimension/pixel limits".into());
    }
    reader(&bytes).decode().map(|image| image.to_rgba8()).map_err(|error| format!("invalid artwork PNG: {error}"))
}

pub fn raster(source: &str, width: u32, height: u32) -> Option<egui::ColorImage> {
    let image = decode(source).ok()?;
    let pixels = image::imageops::resize(
        &image,
        width.clamp(1, 1024),
        height.clamp(1, 1024),
        image::imageops::FilterType::Triangle,
    );
    Some(egui::ColorImage::from_rgba_unmultiplied([pixels.width() as usize, pixels.height() as usize], pixels.as_raw()))
}

#[cfg(test)]
pub(super) fn fixture(width: u32, height: u32, color: [u8; 4]) -> String {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba(color));
    let mut data = Cursor::new(Vec::new());
    image.write_to(&mut data, image::ImageFormat::Png).unwrap();
    super::skin::encode_png(&data.into_inner()).unwrap().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_pixels_keep_authored_colours_and_alpha_and_reject_bombs_or_corruption() {
        let source = fixture(2, 2, [20, 80, 160, 128]);
        let image = raster(&source, 4, 3).unwrap();
        assert_eq!(image.size, [4, 3]);
        assert!(image.pixels.iter().all(|pixel| *pixel == egui::Color32::from_rgba_unmultiplied(20, 80, 160, 128)));
        assert!(decode(&fixture(4097, 1, [0, 0, 0, 255])).is_err());
        assert!(decode(&fixture(2049, 2049, [0, 0, 0, 255])).is_err());
        let invalid = super::super::skin::encode_png(b"\x89PNG\r\n\x1a\ninvalid").unwrap();
        assert!(decode(&invalid).is_err());
    }
}
