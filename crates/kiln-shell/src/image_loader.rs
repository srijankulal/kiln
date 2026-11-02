//! Off-thread image decoder for game poster artwork.

use image::GenericImageView;
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::path::Path;

/// Decodes an image file from disk off the UI thread and converts to a `slint::Image`.
pub fn load_image_off_thread(path: &Path, max_w: u32, max_h: u32) -> Option<Image> {
    if !path.exists() {
        return None;
    }

    let dynamic_img = image::open(path).ok()?;
    let (orig_w, orig_h) = dynamic_img.dimensions();

    // Scale down if larger than target to keep memory small
    let resized = if orig_w > max_w || orig_h > max_h {
        dynamic_img.thumbnail(max_w, max_h)
    } else {
        dynamic_img
    };

    let rgba = resized.to_rgba8();
    let (width, height) = rgba.dimensions();

    let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    buffer.make_mut_bytes().copy_from_slice(rgba.as_raw());
    Some(Image::from_rgba8(buffer))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixel_buffer_creation() {
        let mut buffer = SharedPixelBuffer::<Rgba8Pixel>::new(2, 2);
        let raw = [
            255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
        ];
        buffer.make_mut_bytes().copy_from_slice(&raw);
        let img = Image::from_rgba8(buffer);
        assert_eq!(img.size().width, 2);
        assert_eq!(img.size().height, 2);
    }
}
