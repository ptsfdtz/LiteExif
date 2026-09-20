//! Pillow-compatible resampling and blur coefficients, executed by Rust/Rayon.
//! Algorithm reference: Pillow 12.1.0 libImaging (see THIRD_PARTY_NOTICES.md).
use image::RgbaImage;
use rayon::prelude::*;

const PRECISION: i64 = 1 << 22;

fn weights(input: u32, output: u32) -> Vec<(usize, Vec<i64>)> {
    let ratio = input as f64 / output as f64;
    let scale = ratio.max(1.0);
    let support = 3.0 * scale;
    (0..output)
        .map(|out| {
            let center = (out as f64 + 0.5) * ratio;
            let left = ((center - support + 0.5) as i64).max(0) as usize;
            let right = ((center + support + 0.5) as usize).min(input as usize);
            let mut kernel: Vec<f64> = (left..right)
                .map(|x| {
                    let t = (x as f64 - center + 0.5) / scale;
                    let sinc = |v: f64| {
                        if v == 0.0 {
                            1.0
                        } else {
                            let a = v * std::f64::consts::PI;
                            a.sin() / a
                        }
                    };
                    if t.abs() < 3.0 {
                        sinc(t) * sinc(t / 3.0)
                    } else {
                        0.0
                    }
                })
                .collect();
            let sum: f64 = kernel.iter().sum();
            kernel.iter_mut().for_each(|w| *w /= sum);
            (
                left,
                kernel
                    .into_iter()
                    .map(|w| (w * PRECISION as f64).round() as i64)
                    .collect(),
            )
        })
        .collect()
}

fn premultiply(image: &RgbaImage) -> RgbaImage {
    let mut source = image.clone();
    source.as_mut().par_chunks_mut(4).for_each(|p| {
        for c in 0..3 {
            p[c] = ((p[c] as u32 * p[3] as u32 + 127) / 255) as u8;
        }
    });
    source
}

pub fn resize(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if image.dimensions() == (width, height) {
        return image.clone();
    }
    // Pillow resizes RGBA in premultiplied RGBa, quantizing after each pass.
    // Both premultiply and unpremultiply are identity operations when every
    // source pixel is opaque, which is the common case for photographs, so
    // detect that and skip two full-frame passes.
    let opaque = image.as_raw().par_chunks(4).all(|pixel| pixel[3] == 255);
    let premultiplied;
    let source: &RgbaImage = if opaque {
        image
    } else {
        premultiplied = premultiply(image);
        &premultiplied
    };
    let source_width = image.width() as usize;
    let horizontal = if width == image.width() {
        source.clone()
    } else {
        let kernel = weights(image.width(), width);
        let row_stride = width as usize * 4;
        let mut result = RgbaImage::new(width, image.height());
        result
            .as_mut()
            .par_chunks_mut(row_stride)
            .enumerate()
            .for_each(|(y, row)| {
                let input =
                    &source.as_raw()[y * source_width * 4..(y + 1) * source_width * 4];
                for (x, (left, weights)) in kernel.iter().enumerate() {
                    let mut sums = [PRECISION / 2; 4];
                    let mut index = left * 4;
                    for &weight in weights {
                        let pixel = &input[index..index + 4];
                        for c in 0..4 {
                            sums[c] += pixel[c] as i64 * weight;
                        }
                        index += 4;
                    }
                    let out = &mut row[x * 4..x * 4 + 4];
                    for c in 0..4 {
                        out[c] = (sums[c] >> 22).clamp(0, 255) as u8;
                    }
                }
            });
        result
    };
    let mut result = if height == image.height() {
        horizontal
    } else {
        let kernel = weights(image.height(), height);
        let row_stride = width as usize * 4;
        let mut result = RgbaImage::new(width, height);
        result
            .as_mut()
            .par_chunks_mut(row_stride)
            .enumerate()
            .for_each_init(
                || vec![0i64; row_stride],
                |accumulator, (y, row)| {
                    accumulator.fill(0);
                    let (top, weights) = &kernel[y];
                    for (k, &weight) in weights.iter().enumerate() {
                        let start = (top + k) * row_stride;
                        let line = &horizontal.as_raw()[start..start + row_stride];
                        for (accumulator, sample) in accumulator.iter_mut().zip(line) {
                            *accumulator += *sample as i64 * weight;
                        }
                    }
                    for (value, sum) in row.iter_mut().zip(accumulator.iter()) {
                        *value = ((*sum + PRECISION / 2) >> 22).clamp(0, 255) as u8;
                    }
                },
            );
        result
    };
    if !opaque {
        result.as_mut().par_chunks_mut(4).for_each(|p| {
            if p[3] > 0 && p[3] < 255 {
                for c in 0..3 {
                    p[c] = (p[c] as u32 * 255 / p[3] as u32).min(255) as u8;
                }
            }
        });
    }
    result
}

pub fn blur_parameters(radius: u32) -> (u32, u32, u32) {
    let sigma2 = (radius as f32).powi(2) / 3.0;
    let length = (12.0 * sigma2 as f64 + 1.0).sqrt() as f32;
    let l = ((length - 1.0) / 2.0).floor();
    let a = ((2.0 * l + 1.0) * (l * (l + 1.0) - 3.0 * sigma2))
        / (6.0 * (sigma2 - (l + 1.0) * (l + 1.0)));
    let fractional = l + a;
    let integer = fractional as u32;
    let weight = ((1u32 << 24) as f32 / (fractional * 2.0 + 1.0)) as u32;
    let fringe = ((1u32 << 24) - (integer * 2 + 1) * weight) / 2;
    (integer, weight, fringe)
}

fn horizontal_blur(source: &RgbaImage, radius: u32, weight: u32, fringe: u32) -> RgbaImage {
    let width = source.width() as usize;
    let radius = radius as usize;
    let mut output = RgbaImage::new(source.width(), source.height());
    output
        .as_mut()
        .par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let input = &source.as_raw()[y * width * 4..(y + 1) * width * 4];
            let mut sum = [0u32; 4];
            let first = &input[..4];
            for c in 0..4 {
                sum[c] = first[c] as u32 * (radius + 1) as u32;
            }
            for x in 1..=radius.min(width - 1) {
                let pixel = &input[x * 4..x * 4 + 4];
                for c in 0..4 {
                    sum[c] += pixel[c] as u32;
                }
            }
            let last = &input[(width - 1) * 4..width * 4];
            let extra = radius.saturating_sub(width - 1) as u32;
            for c in 0..4 {
                sum[c] += last[c] as u32 * extra;
            }
            for x in 0..width {
                let left_edge = x.saturating_sub(radius + 1);
                let right_edge = (x + radius + 1).min(width - 1);
                let subtract = x.saturating_sub(radius);
                let left_pixel = &input[left_edge * 4..left_edge * 4 + 4];
                let right_pixel = &input[right_edge * 4..right_edge * 4 + 4];
                let subtract_pixel = &input[subtract * 4..subtract * 4 + 4];
                let out = &mut row[x * 4..x * 4 + 4];
                for c in 0..4 {
                    let edges = left_pixel[c] as u32 + right_pixel[c] as u32;
                    out[c] = ((sum[c] * weight + edges * fringe + (1 << 23)) >> 24) as u8;
                    sum[c] = sum[c] + right_pixel[c] as u32 - subtract_pixel[c] as u32;
                }
            }
        });
    output
}

pub fn gaussian_blur(source: &RgbaImage, radius: u32) -> RgbaImage {
    if radius == 0 {
        return source.clone();
    }
    let (r, w, f) = blur_parameters(radius);
    let mut output = source.clone();
    for _ in 0..3 {
        output = horizontal_blur(&output, r, w, f);
    }
    output = image::imageops::rotate90(&output);
    for _ in 0..3 {
        output = horizontal_blur(&output, r, w, f);
    }
    image::imageops::rotate270(&output)
}

// Pillow's integer ellipse walk (inclusive bounding box), rather than a
// Euclidean distance test at pixel centers. Each entry is the left filled x.
fn ellipse_insets(a: i64, b: i64) -> Vec<i64> {
    let mut insets = vec![a / 2; b as usize + 1];
    let (mut x, mut y) = (a, b % 2);
    let delta = |x: i64, y: i64| (a * a * y * y + b * b * x * x - a * a * b * b).abs();
    loop {
        let inset = (a - x) / 2;
        for row in [(b - y) / 2, (b + y) / 2] {
            insets[row as usize] = insets[row as usize].min(inset);
        }
        if x == a % 2 && y == b {
            break;
        }
        let (mut nx, mut ny) = (x, y + 2);
        let mut distance = delta(nx, ny);
        if nx > 1 {
            let diagonal = delta(x - 2, y + 2);
            if distance > diagonal {
                nx = x - 2;
                ny = y + 2;
                distance = diagonal;
            }
            if distance > delta(x - 2, y) {
                nx = x - 2;
                ny = y;
            }
        }
        x = nx;
        y = ny;
    }
    insets
}

pub fn rounded_corner(source: &RgbaImage, radius: i64) -> RgbaImage {
    let w = source.width() as i64;
    let h = source.height() as i64;
    let mut diameter = radius.max(0) * 2;
    let full_x = diameter >= w - 1;
    if full_x {
        diameter = w;
    }
    let full_y = diameter >= h - 1;
    if full_y {
        diameter = h;
    }
    let insets = if full_x && full_y {
        ellipse_insets(w, h)
    } else {
        ellipse_insets(diameter, diameter)
    };
    let mut output = source.clone();
    output
        .as_mut()
        .par_chunks_mut(w as usize * 4)
        .enumerate()
        .for_each(|(y, row)| {
            let y = y as i64;
            let inset = if full_x && full_y {
                insets[y as usize]
            } else if y < diameter / 2 {
                insets[y as usize]
            } else if y > h - diameter / 2 {
                insets[(y - h + diameter) as usize]
            } else {
                0
            };
            for (x, p) in row.chunks_mut(4).enumerate() {
                p[3] = if (x as i64) >= inset && (x as i64) <= w - inset {
                    255
                } else {
                    0
                };
            }
        });
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }
    // Golden images are generated by scripts/generate-raster-fixtures.py and may
    // be absent in a fresh checkout; skip instead of failing when they are.
    fn fixtures_available() -> bool {
        fixtures_dir().join("source.png").is_file()
    }
    fn fixture(name: &str) -> RgbaImage {
        image::open(fixtures_dir().join(name)).unwrap().into_rgba8()
    }
    #[test]
    fn resampling_matches_pillow_including_transparent_edges() {
        if !fixtures_available() {
            eprintln!("skipping: run scripts/generate-raster-fixtures.py first");
            return;
        }
        let input = fixture("source.png");
        for (w, h) in [(74, 46), (51, 31), (19, 11), (13, 41)] {
            assert_eq!(
                resize(&input, w, h),
                fixture(&format!("resize-{w}-{h}.png")),
                "{w}x{h}"
            );
        }
    }
    #[test]
    fn blur_matches_pillow_including_radius_larger_than_image() {
        if !fixtures_available() {
            eprintln!("skipping: run scripts/generate-raster-fixtures.py first");
            return;
        }
        let input = fixture("source.png");
        for radius in [1, 3, 9, 35] {
            assert_eq!(
                gaussian_blur(&input, radius),
                fixture(&format!("blur-{radius}.png")),
                "radius {radius}"
            );
        }
    }
    #[test]
    fn rounded_corners_match_pillow_including_alpha_replacement() {
        if !fixtures_available() {
            eprintln!("skipping: run scripts/generate-raster-fixtures.py first");
            return;
        }
        let input = fixture("source.png");
        for radius in [0, 3, 6, 37] {
            assert_eq!(
                rounded_corner(&input, radius),
                fixture(&format!("round-{radius}.png")),
                "radius {radius}"
            );
        }
    }
}
