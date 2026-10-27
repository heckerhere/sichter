//! Wahrnehmungs-Hash (dHash, 64 Bit) für die Serien-Erkennung.
//!
//! Bild auf 9×8 Graustufen verkleinern, je Zeile benachbarte Pixel vergleichen.
//! Ähnliche Bilder unterscheiden sich in wenigen Bits (Hamming-Abstand).

use image::{DynamicImage, imageops::FilterType};

pub fn dhash(img: &DynamicImage) -> u64 {
    let small = img.resize_exact(9, 8, FilterType::Triangle).to_luma8();
    let mut bits = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            bits <<= 1;
            if small.get_pixel(x, y)[0] < small.get_pixel(x + 1, y)[0] {
                bits |= 1;
            }
        }
    }
    bits
}

pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn gradient(shift: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(64, 48, |x, y| Rgb([((x * 4 + shift) % 256) as u8, (y * 5) as u8, 90])))
    }

    #[test]
    fn similar_images_are_close_different_are_far() {
        let a = dhash(&gradient(0));
        let b = dhash(&gradient(3)); // leicht verschoben
        let c = dhash(&DynamicImage::ImageRgb8(RgbImage::from_fn(64, 48, |x, y| Rgb([((x * y) % 256) as u8, 255 - (x * 3) as u8, (y * 7) as u8]))));
        assert!(distance(a, b) <= 12, "{}", distance(a, b));
        assert!(distance(a, c) > 12, "{}", distance(a, c));
        assert_eq!(distance(a, a), 0);
    }
}
