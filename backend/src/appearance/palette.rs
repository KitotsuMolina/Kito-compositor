use super::{PaletteCandidate, WallpaperPalette};
use std::collections::BTreeMap;
use std::path::Path;

const MAX_CANDIDATES: usize = 8;

#[derive(Debug, Clone, Copy)]
struct Color {
    r: f64,
    g: f64,
    b: f64,
}

#[derive(Debug, Clone)]
struct Bucket {
    color: Color,
    count: u64,
    weight: f64,
    luminance: f64,
    saturation: f64,
}

pub const ALGORITHM_VERSION: u32 = 2;
const FAMILY_NAMES: [&str; 6] = ["red", "yellow", "green", "cyan", "blue", "magenta"];

fn family(color: Color) -> Option<usize> {
    let max = color.r.max(color.g).max(color.b);
    let min = color.r.min(color.g).min(color.b);
    let delta = max - min;
    if max == 0.0 || delta / max < 0.35 || delta * 255.0 < 35.0 - 1e-9 || color.luminance() < 0.025
    {
        return None;
    }
    Some((((color.to_hsl().0 * 360.0 + 30.0) % 360.0) / 60.0).floor() as usize)
}

fn percentile(rows: &[&Bucket], percent: u64) -> Color {
    let total: u64 = rows.iter().map(|b| b.count).sum();
    let mut accumulated = 0;
    for row in rows {
        accumulated += row.count;
        if accumulated * 100 >= total * percent {
            return row.color;
        }
    }
    rows.last().unwrap().color
}

pub fn extract(path: &Path) -> Result<WallpaperPalette, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("appearance image must be an existing absolute file".into());
    }
    let (width, height) = image::image_dimensions(path)
        .map_err(|e| format!("failed to inspect appearance image: {e}"))?;
    if u64::from(width) * u64::from(height) > 32_000_000 {
        return Err("appearance image exceeds 32 million pixels".into());
    }
    let image = image::open(path)
        .map_err(|e| format!("failed to decode appearance image: {e}"))?
        .into_rgba8();
    let mut histogram = BTreeMap::<[u8; 3], u64>::new();
    let mut excluded_transparent_pixels = 0;
    for pixel in image.pixels() {
        let [r, g, b, a] = pixel.0;
        // Partial transparency has no unique displayed RGB without the underlying background.
        if a != 255 {
            excluded_transparent_pixels += 1;
            continue;
        }
        *histogram.entry([r, g, b]).or_default() += 1;
        if histogram.len() > 2_000_000 {
            return Err("appearance image exceeds two million distinct opaque colors".into());
        }
    }
    let pixel_count: u64 = histogram.values().sum();
    if pixel_count == 0 {
        return Err("appearance image has no opaque pixels".into());
    }
    let mut buckets: Vec<_> = histogram
        .into_iter()
        .map(|([r, g, b], count)| {
            let color = Color::from_rgb8(r, g, b);
            let max = color.r.max(color.g).max(color.b);
            let min = color.r.min(color.g).min(color.b);
            Bucket {
                color,
                count,
                weight: count as f64 / pixel_count as f64,
                luminance: color.luminance(),
                saturation: if max == 0.0 { 0.0 } else { (max - min) / max },
            }
        })
        .collect();
    let background_luminance = buckets.iter().map(|b| b.luminance * b.weight).sum();
    buckets.sort_by(|a, b| {
        b.count.cmp(&a.count).then_with(|| {
            a.color
                .r
                .total_cmp(&b.color.r)
                .then(a.color.g.total_cmp(&b.color.g))
                .then(a.color.b.total_cmp(&b.color.b))
        })
    });
    let dominant = buckets[0].color;
    let mut ordered: Vec<_> = buckets.iter().collect();
    ordered.sort_by(|a, b| {
        a.luminance.total_cmp(&b.luminance).then_with(|| {
            a.color
                .r
                .total_cmp(&b.color.r)
                .then(a.color.g.total_cmp(&b.color.g))
                .then(a.color.b.total_cmp(&b.color.b))
        })
    });
    let mut families = BTreeMap::new();
    let mut family_colors = Vec::new();
    let mut eligible = Vec::new();
    for (index, name) in FAMILY_NAMES.iter().enumerate() {
        let rows: Vec<_> = ordered
            .iter()
            .copied()
            .filter(|b| family(b.color) == Some(index))
            .collect();
        let entry = if rows.is_empty() {
            None
        } else {
            let light = percentile(&rows, 90);
            let mid = percentile(&rows, 50);
            let dark = percentile(&rows, 10);
            let count = rows.iter().map(|b| b.count).sum::<u64>();
            family_colors.push(mid.to_hex());
            eligible.extend(rows);
            Some(super::model::ColorFamily {
                light: light.to_hex(),
                mid: mid.to_hex(),
                dark: dark.to_hex(),
                pixel_count: count,
                coverage: round(count as f64 / pixel_count as f64),
            })
        };
        families.insert((*name).to_string(), entry);
    }
    eligible.sort_by(|a, b| {
        a.luminance.total_cmp(&b.luminance).then_with(|| {
            a.color
                .r
                .total_cmp(&b.color.r)
                .then(a.color.g.total_cmp(&b.color.g))
                .then(a.color.b.total_cmp(&b.color.b))
        })
    });
    let role_pool = if eligible.is_empty() {
        &ordered
    } else {
        &eligible
    };
    let accent_light = percentile(role_pool, 90);
    let accent_mid = percentile(role_pool, 50);
    let accent_dark = percentile(role_pool, 10);
    let vibrant = eligible
        .iter()
        .max_by(|a, b| {
            a.saturation
                .total_cmp(&b.saturation)
                .then(a.count.cmp(&b.count))
        })
        .map_or(dominant, |b| b.color);
    let muted = ordered
        .iter()
        .min_by(|a, b| {
            a.saturation
                .total_cmp(&b.saturation)
                .then(b.count.cmp(&a.count))
        })
        .unwrap()
        .color;
    let foreground = if accent_mid.contrast(Color::BLACK) >= accent_mid.contrast(Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    };
    // Reserve slots for rare families before adding frequent scene colors.
    let mut selected = Vec::new();
    for hex in family_colors
        .into_iter()
        .chain(buckets.iter().map(|b| b.color.to_hex()))
    {
        if !selected.contains(&hex) {
            selected.push(hex);
        }
        if selected.len() == MAX_CANDIDATES {
            break;
        }
    }
    let candidates = selected
        .into_iter()
        .map(|hex| {
            let bucket = buckets.iter().find(|b| b.color.to_hex() == hex).unwrap();
            PaletteCandidate {
                color: hex,
                weight: round(bucket.weight),
                luminance: round(bucket.luminance),
                saturation: round(bucket.saturation),
            }
        })
        .collect();
    Ok(WallpaperPalette {
        algorithm_version: ALGORITHM_VERSION,
        families,
        sampled_pixels: pixel_count,
        excluded_transparent_pixels,
        dominant: dominant.to_hex(),
        vibrant: vibrant.to_hex(),
        muted: muted.to_hex(),
        accent_light: accent_light.to_hex(),
        accent_mid: accent_mid.to_hex(),
        accent_dark: accent_dark.to_hex(),
        foreground: foreground.to_hex(),
        background_luminance: round(background_luminance),
        candidates,
    })
}

fn round(value: f64) -> f64 {
    (value * 100_000_000.0).round() / 100_000_000.0
}

impl Color {
    const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    };
    const WHITE: Self = Self {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    };

    fn from_rgb8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: f64::from(r) / 255.0,
            g: f64::from(g) / 255.0,
            b: f64::from(b) / 255.0,
        }
    }

    fn to_hex(self) -> String {
        format!(
            "#{:02X}{:02X}{:02X}",
            (self.r.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.g.clamp(0.0, 1.0) * 255.0).round() as u8,
            (self.b.clamp(0.0, 1.0) * 255.0).round() as u8
        )
    }

    fn luminance(self) -> f64 {
        fn channel(value: f64) -> f64 {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        }
        (0.2126 * channel(self.r)) + (0.7152 * channel(self.g)) + (0.0722 * channel(self.b))
    }

    fn contrast(self, other: Self) -> f64 {
        let a = self.luminance();
        let b = other.luminance();
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn to_hsl(self) -> (f64, f64, f64) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let lightness = (max + min) / 2.0;
        if (max - min).abs() < f64::EPSILON {
            return (0.0, 0.0, lightness);
        }
        let delta = max - min;
        let saturation = if lightness > 0.5 {
            delta / (2.0 - max - min)
        } else {
            delta / (max + min)
        };
        let hue = if (max - self.r).abs() < f64::EPSILON {
            ((self.g - self.b) / delta + if self.g < self.b { 6.0 } else { 0.0 }) / 6.0
        } else if (max - self.g).abs() < f64::EPSILON {
            ((self.b - self.r) / delta + 2.0) / 6.0
        } else {
            ((self.r - self.g) / delta + 4.0) / 6.0
        };
        (hue, saturation, lightness)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(image: image::RgbaImage) -> WallpaperPalette {
        let path = std::env::temp_dir().join(format!(
            "palette-v2-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        image.save(&path).unwrap();
        let result = extract(&path);
        std::fs::remove_file(path).unwrap();
        result.unwrap()
    }
    #[test]
    fn retains_tiny_accents_and_all_six_families_without_inventing_colors() {
        let colors = [
            [190, 60, 60, 255],
            [190, 170, 60, 255],
            [60, 150, 60, 255],
            [60, 150, 150, 255],
            [60, 60, 190, 255],
            [180, 60, 180, 255],
        ];
        let mut image = image::RgbaImage::from_pixel(1000, 1000, image::Rgba([25, 25, 25, 255]));
        for (i, c) in colors.iter().enumerate() {
            image.put_pixel(i as u32, 0, image::Rgba(*c));
        }
        let palette = fixture(image);
        for (i, name) in FAMILY_NAMES.iter().enumerate() {
            let family = palette.families[*name].as_ref().unwrap();
            let expected = Color::from_rgb8(colors[i][0], colors[i][1], colors[i][2]).to_hex();
            assert_eq!(family.light, expected);
            assert_eq!(family.mid, expected);
            assert_eq!(family.dark, expected);
            assert_eq!(family.pixel_count, 1);
            assert!(palette.candidates.iter().any(|c| c.color == expected));
        }
        assert_eq!(palette.algorithm_version, 2);
    }
    #[test]
    fn representative_magenta_rejects_near_neutral_extremes_and_uses_weighted_percentiles() {
        let colors = [
            [100, 40, 100, 255],
            [145, 50, 145, 255],
            [215, 75, 215, 255],
            [255, 254, 255, 255],
            [1, 0, 1, 255],
        ];
        let image = image::RgbaImage::from_fn(100, 1, |x, _| {
            image::Rgba(
                colors[if x < 30 {
                    0
                } else if x < 60 {
                    1
                } else if x < 90 {
                    2
                } else if x < 95 {
                    3
                } else {
                    4
                }],
            )
        });
        let palette = fixture(image);
        let family = palette.families["magenta"].as_ref().unwrap();
        assert_eq!(family.dark, "#642864");
        assert_eq!(family.mid, "#913291");
        assert_eq!(family.light, "#D74BD7");
        assert_eq!(family.pixel_count, 90);
        assert!(palette.families["cyan"].is_none());
    }
    #[test]
    fn grayscale_has_no_families_and_transparent_hidden_colors_are_excluded() {
        let mut image = image::RgbaImage::from_pixel(10, 10, image::Rgba([90, 90, 90, 255]));
        image.put_pixel(0, 0, image::Rgba([255, 0, 255, 0]));
        image.put_pixel(1, 0, image::Rgba([0, 255, 255, 120]));
        let palette = fixture(image);
        assert!(palette.families.values().all(Option::is_none));
        assert_eq!(palette.accent_light, "#5A5A5A");
        assert_eq!(palette.excluded_transparent_pixels, 2);
        assert_eq!(palette.sampled_pixels, 98);
    }

    #[test]
    fn extracts_deterministic_roles_and_contrasting_foreground() {
        let path = std::env::temp_dir().join(format!(
            "kitsune-palette-{}-{}.png",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let image = ImageBuffer::from_fn(64, 64, |x, _| {
            if x < 48 {
                Rgb([30_u8, 80, 210])
            } else {
                Rgb([245_u8, 80, 170])
            }
        });
        image.save(&path).unwrap();
        let first = extract(&path).unwrap();
        let second = extract(&path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.dominant, "#1E50D2");
        assert!(first.candidates.len() >= 2);
        assert!(matches!(first.foreground.as_str(), "#000000" | "#FFFFFF"));
        let _ = std::fs::remove_file(path);
    }
}
