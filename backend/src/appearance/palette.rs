use super::{PaletteCandidate, WallpaperPalette};
use image::imageops::FilterType;
use std::cmp::{Ordering, Reverse};
use std::collections::BTreeMap;
use std::path::Path;

const SAMPLE_SIZE: u32 = 96;
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

pub fn extract(path: &Path) -> Result<WallpaperPalette, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("appearance image must be an existing absolute file".into());
    }
    let image =
        image::open(path).map_err(|error| format!("failed to decode appearance image: {error}"))?;
    let rgb = image
        .resize(SAMPLE_SIZE, SAMPLE_SIZE, FilterType::Triangle)
        .into_rgb8();
    let pixel_count = u64::from(rgb.width()) * u64::from(rgb.height());
    if pixel_count == 0 {
        return Err("appearance image has no pixels".into());
    }

    let mut histogram = BTreeMap::<u16, (u64, u64, u64, u64)>::new();
    let mut luma_sum = 0.0;
    for pixel in rgb.pixels() {
        let [r, g, b] = pixel.0;
        let color = Color::from_rgb8(r, g, b);
        luma_sum += color.luminance();
        let key = (u16::from(r >> 3) << 10) | (u16::from(g >> 3) << 5) | u16::from(b >> 3);
        let entry = histogram.entry(key).or_default();
        entry.0 += 1;
        entry.1 += u64::from(r);
        entry.2 += u64::from(g);
        entry.3 += u64::from(b);
    }
    let background_luminance = luma_sum / pixel_count as f64;
    let mut buckets = histogram
        .into_values()
        .map(|(count, r, g, b)| {
            let color = Color {
                r: r as f64 / count as f64 / 255.0,
                g: g as f64 / count as f64 / 255.0,
                b: b as f64 / count as f64 / 255.0,
            };
            let (_, saturation, _) = color.to_hsl();
            Bucket {
                color,
                count,
                weight: count as f64 / pixel_count as f64,
                luminance: color.luminance(),
                saturation,
            }
        })
        .collect::<Vec<_>>();
    buckets.sort_by_key(|bucket| Reverse(bucket.count));

    let dominant = buckets
        .first()
        .ok_or_else(|| "appearance image produced no palette candidates".to_string())?
        .color;
    let vibrant = buckets
        .iter()
        .filter(|bucket| (0.08..=0.92).contains(&bucket.luminance))
        .max_by(|a, b| score(a).partial_cmp(&score(b)).unwrap_or(Ordering::Equal))
        .map(|bucket| bucket.color)
        .unwrap_or(dominant);
    let muted = buckets
        .iter()
        .filter(|bucket| (0.12..=0.88).contains(&bucket.luminance))
        .min_by(|a, b| {
            muted_score(a)
                .partial_cmp(&muted_score(b))
                .unwrap_or(Ordering::Equal)
        })
        .map(|bucket| bucket.color)
        .unwrap_or(dominant);

    let (_, saturation, _) = vibrant.to_hsl();
    let saturation = saturation.clamp(0.50, 0.92);
    let mid_luma = if background_luminance < 0.45 {
        0.68
    } else if background_luminance > 0.62 {
        0.34
    } else {
        0.52
    };
    let hue = vibrant.to_hsl().0;
    let accent_light = Color::from_hsl(hue, (saturation * 0.92).clamp(0.48, 0.90), 0.78);
    let accent_mid = Color::from_hsl(hue, saturation, mid_luma);
    let accent_dark = Color::from_hsl(hue, (saturation * 0.86).clamp(0.42, 0.88), 0.26);
    let foreground = if accent_mid.contrast(Color::BLACK) >= accent_mid.contrast(Color::WHITE) {
        Color::BLACK
    } else {
        Color::WHITE
    };
    let candidates = buckets
        .iter()
        .take(MAX_CANDIDATES)
        .map(|bucket| PaletteCandidate {
            color: bucket.color.to_hex(),
            weight: round(bucket.weight),
            luminance: round(bucket.luminance),
            saturation: round(bucket.saturation),
        })
        .collect();

    Ok(WallpaperPalette {
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

fn score(bucket: &Bucket) -> f64 {
    (bucket.saturation * 0.65) + bucket.weight.min(0.55) - ((bucket.luminance - 0.52).abs() * 0.35)
}

fn muted_score(bucket: &Bucket) -> f64 {
    bucket.saturation + ((bucket.luminance - 0.50).abs() * 0.4) - bucket.weight.min(0.3)
}

fn round(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
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

    fn from_hsl(hue: f64, saturation: f64, lightness: f64) -> Self {
        if saturation == 0.0 {
            return Self {
                r: lightness,
                g: lightness,
                b: lightness,
            };
        }
        let q = if lightness < 0.5 {
            lightness * (1.0 + saturation)
        } else {
            lightness + saturation - (lightness * saturation)
        };
        let p = (2.0 * lightness) - q;
        Self {
            r: hue_to_rgb(p, q, hue + (1.0 / 3.0)),
            g: hue_to_rgb(p, q, hue),
            b: hue_to_rgb(p, q, hue - (1.0 / 3.0)),
        }
    }
}

fn hue_to_rgb(p: f64, q: f64, mut value: f64) -> f64 {
    if value < 0.0 {
        value += 1.0;
    }
    if value > 1.0 {
        value -= 1.0;
    }
    if value < 1.0 / 6.0 {
        p + ((q - p) * 6.0 * value)
    } else if value < 0.5 {
        q
    } else if value < 2.0 / 3.0 {
        p + ((q - p) * ((2.0 / 3.0) - value) * 6.0)
    } else {
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use std::time::{SystemTime, UNIX_EPOCH};

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
