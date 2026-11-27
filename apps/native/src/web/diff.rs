//! The `screenshotDiff` command's comparison of two pictures: how many pixels differ, and a picture of where.

use image::{Rgba, RgbaImage};

/// What two pictures of the same size differ by.
pub struct Diff {
    pub changed: u64,
    pub total: u64,
    /// The changed pixels red, the rest of the current picture faded to light gray.
    pub image: RgbaImage,
}

/// Compares `current` with `baseline`: a pixel changed when any of its channels is more than `threshold` (0 to 1) of 255
/// away. Pictures of different sizes aren't compared.
pub fn compare(baseline: &RgbaImage, current: &RgbaImage, threshold: f64) -> Result<Diff, String> {
    if baseline.dimensions() != current.dimensions() {
        return Err(format!(
            "The pictures differ in size: the baseline is {}x{}, the page now {}x{}",
            baseline.width(),
            baseline.height(),
            current.width(),
            current.height()
        ));
    }
    let limit = (threshold.clamp(0.0, 1.0) * 255.0) as f32;
    let mut image = RgbaImage::new(current.width(), current.height());
    let mut changed = 0u64;
    for ((a, b), out) in baseline.pixels().zip(current.pixels()).zip(image.pixels_mut()) {
        let differs = a.0.iter().zip(b.0.iter()).any(|(a, b)| f32::from(a.abs_diff(*b)) > limit);
        *out = if differs {
            changed += 1;
            Rgba([255, 0, 0, 255])
        } else {
            let [r, g, bl, _] = b.0;
            let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(bl);
            let faded = (luma * 0.35 + 255.0 * 0.65) as u8;
            Rgba([faded, faded, faded, 255])
        };
    }
    Ok(Diff { changed, total: u64::from(current.width()) * u64::from(current.height()), image })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(width: u32, height: u32, color: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba(color))
    }

    #[test]
    fn identical_pictures_have_no_changed_pixels() {
        let (a, b) = (flat(4, 3, [10, 20, 30, 255]), flat(4, 3, [10, 20, 30, 255]));
        let diff = compare(&a, &b, 0.1).unwrap();
        assert_eq!((diff.changed, diff.total), (0, 12));
        assert!(diff.image.pixels().all(|pixel| pixel.0[0] == pixel.0[1] && pixel.0[1] == pixel.0[2] && pixel.0[0] > 150), "faded to light gray");
    }

    #[test]
    fn one_changed_pixel_is_counted_and_red_in_the_diff() {
        let a = flat(4, 3, [10, 20, 30, 255]);
        let mut b = a.clone();
        b.put_pixel(2, 1, Rgba([200, 20, 30, 255]));
        let diff = compare(&a, &b, 0.1).unwrap();
        assert_eq!(diff.changed, 1);
        assert_eq!(diff.image.get_pixel(2, 1).0, [255, 0, 0, 255]);
        assert_ne!(diff.image.get_pixel(1, 1).0, [255, 0, 0, 255]);
    }

    #[test]
    fn a_difference_within_the_threshold_is_not_a_change() {
        let a = flat(2, 2, [100, 100, 100, 255]);
        // 25 of 255 is a tenth of it: not more than it, but 26 is.
        assert_eq!(compare(&a, &flat(2, 2, [125, 100, 100, 255]), 0.1).unwrap().changed, 0);
        assert_eq!(compare(&a, &flat(2, 2, [127, 100, 100, 255]), 0.1).unwrap().changed, 4);
        assert_eq!(compare(&a, &flat(2, 2, [101, 100, 100, 255]), 0.0).unwrap().changed, 4, "any difference at none");
        assert_eq!(compare(&a, &flat(2, 2, [255, 255, 255, 255]), 1.0).unwrap().changed, 0, "none at all of the most");
    }

    #[test]
    fn pictures_of_other_sizes_say_both() {
        let error = compare(&flat(4, 3, [0; 4]), &flat(5, 3, [0; 4]), 0.1).err().unwrap();
        assert!(error.contains("4x3") && error.contains("5x3"), "{error}");
    }
}
