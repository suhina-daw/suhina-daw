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

/// A background PNG brought within the theme's limits: one larger than 8 MiB or 4 megapixels (a
/// 4K wallpaper) is scaled down to fit 2560 by 1600 and written again; one already within them is
/// kept as it is. Up to 64 MiB and 8192 pixels a side are read.
pub fn fit_background(bytes: &[u8]) -> Result<Vec<u8>, String> {
    const MAX_FILE: usize = 64 * 1024 * 1024;
    if bytes.len() > MAX_FILE || !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("the background must be a PNG of at most 64 MiB".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(300 * 1024 * 1024);
    reader.limits(limits.clone());
    let within = bytes.len() <= super::skin::MAX_PNG;
    let (width, height) = match reader.into_dimensions() {
        Ok(size) => size,
        // Within the limits it is kept as it was; drawing it says when it does not read.
        Err(_) if within => return Ok(bytes.to_vec()),
        Err(error) => return Err(format!("the background PNG does not read: {error}")),
    };
    if within && u64::from(width) * u64::from(height) <= 4 * 1024 * 1024 {
        return Ok(bytes.to_vec());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| format!("the background PNG does not read: {error}"))?;
    let scale = (2560.0 / f64::from(width)).min(1600.0 / f64::from(height)).min(1.0);
    let (w, h) =
        (((f64::from(width) * scale).round() as u32).max(1), ((f64::from(height) * scale).round() as u32).max(1));
    let small = image.resize(w, h, image::imageops::FilterType::Triangle).to_rgb8();
    let mut out = Cursor::new(Vec::new());
    small.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let out = out.into_inner();
    if out.len() > super::skin::MAX_PNG {
        return Err("the background is too large even when scaled down".into());
    }
    Ok(out)
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
