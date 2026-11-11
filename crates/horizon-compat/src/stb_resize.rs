//! `stbir_resize_uint8`, as the `stb_image_resize` LWJGL 3.3 carries (v0.9x) does it: what
//! `NativeImage.resizeSubRectTo` and a mod's own call of `STBImageResize` scale an image with
//! (JourneyMap fits its minimap's mask and rim to the minimap's size so, and takes a failed
//! resize for a reason not to make the minimap at all).
//!
//! The library's defaults: each axis filtered on its own, with Catmull-Rom where the image grows
//! and Mitchell where it shrinks or stays, the edge pixels repeated past the edges, every channel
//! filtered as it is (no colour space, no alpha weighting), in single precision. Its steps are
//! kept as they are, down to which pixels a filter is said to reach, so the bytes are its bytes.

/// Both filters reach two pixels each way (`stbir__support_two`).
const SUPPORT: f32 = 2.0;
/// The coefficients kept for one contributor: `ceil(support * 2)`.
const WIDTH: usize = 4;

#[derive(Clone, Copy)]
enum Filter {
    CatmullRom,
    Mitchell,
}

fn kernel(filter: Filter, x: f32) -> f32 {
    let x = x.abs();
    match filter {
        Filter::CatmullRom if x < 1.0 => 1.0 - x * x * (2.5 - 1.5 * x),
        Filter::CatmullRom if x < 2.0 => 2.0 - x * (4.0 + x * (0.5 * x - 2.5)),
        Filter::Mitchell if x < 1.0 => (16.0 + x * x * (21.0 * x - 36.0)) / 18.0,
        Filter::Mitchell if x < 2.0 => (32.0 + x * (-60.0 + x * (36.0 - 7.0 * x))) / 18.0,
        _ => 0.0,
    }
}

/// One axis of a resize: which pixels each contributor reaches and how much of it each gets.
/// Growing, a contributor is an output pixel and reaches input pixels; shrinking, it is an input
/// pixel (those of the margins too) and reaches output pixels.
struct Axis {
    grows: bool,
    /// How many pixels past each edge are read.
    margin: i32,
    /// The first and last pixel each contributor reaches (none when the last is before the first).
    reach: Vec<(i32, i32)>,
    /// [`WIDTH`] coefficients a contributor, from its first pixel on.
    coefficients: Vec<f32>,
}

fn axis(input: i32, output: i32) -> Axis {
    let scale = output as f32 / input as f32;
    let grows = scale > 1.0;
    let filter = if grows {
        Filter::CatmullRom
    } else {
        Filter::Mitchell
    };
    let pixel_width = if grows {
        f64::from(SUPPORT * 2.0).ceil() as i32
    } else {
        f64::from(SUPPORT * 2.0 / scale).ceil() as i32
    };
    let margin = pixel_width / 2;
    let count = if grows { output } else { input + margin * 2 } as usize;
    let mut reach = vec![(0, -1); count];
    let mut coefficients = vec![0.0f32; count * WIDTH];
    // (A filter's reach is at most a pixel more than is kept, and that one's share is nothing.)
    let mut group = [0.0f32; WIDTH + 4];
    for n in 0..count {
        group.fill(0.0);
        let (first, last) = if grows {
            let radius = SUPPORT * scale;
            let centre = n as f32 + 0.5;
            let centre_in = centre / scale;
            let mut first = (f64::from((centre - radius) / scale) + 0.5).floor() as i32;
            let last = (f64::from((centre + radius) / scale) - 0.5).floor() as i32;
            let mut total = 0.0f32;
            let mut i = 0;
            while i <= last - first {
                let share = kernel(filter, centre_in - ((i + first) as f32 + 0.5));
                // A first pixel with no share is left out.
                if i == 0 && share == 0.0 {
                    first += 1;
                    continue;
                }
                if let Some(slot) = group.get_mut(i as usize) {
                    *slot = share;
                }
                total += share;
                i += 1;
            }
            // The shares of one output pixel add up to one.
            let fit = 1.0 / total;
            for share in &mut group {
                *share *= fit;
            }
            (first, last)
        } else {
            let radius = SUPPORT / scale;
            let centre = (n as i32 - margin) as f32 + 0.5;
            let centre_out = centre * scale;
            let first = (f64::from((centre - radius) * scale) + 0.5).floor() as i32;
            let last = (f64::from((centre + radius) * scale) - 0.5).floor() as i32;
            for i in 0..=(last - first) {
                if let Some(slot) = group.get_mut(i as usize) {
                    *slot = kernel(filter, ((i + first) as f32 + 0.5) - centre_out) * scale;
                }
            }
            (first, last)
        };
        // The last pixels with no share are left out.
        let mut end = last;
        for i in (0..=(last - first)).rev() {
            if group.get(i as usize).is_some_and(|share| *share != 0.0) {
                break;
            }
            end = first + i - 1;
        }
        reach[n] = (first, end);
        coefficients[n * WIDTH..(n + 1) * WIDTH].copy_from_slice(&group[..WIDTH]);
    }
    if !grows {
        normalise(&mut reach, &mut coefficients, output);
    }
    Axis {
        grows,
        margin,
        reach,
        coefficients,
    }
}

/// Shrinking: the shares each output pixel is given add up to one, and a contributor starts at
/// its first share that is something, inside the image (`stbir__normalize_downsample_coefficients`).
fn normalise(reach: &mut [(i32, i32)], coefficients: &mut [f32], output: i32) {
    for i in 0..output {
        let within = |r: &(i32, i32)| i >= r.0 && i <= r.1;
        let mut total = 0.0f32;
        for (j, r) in reach.iter().enumerate() {
            if within(r) {
                total += coefficients[j * WIDTH + (i - r.0) as usize];
            } else if i < r.0 {
                break;
            }
        }
        let fit = 1.0 / total;
        for (j, r) in reach.iter().enumerate() {
            if within(r) {
                coefficients[j * WIDTH + (i - r.0) as usize] *= fit;
            } else if i < r.0 {
                break;
            }
        }
    }
    for (j, r) in reach.iter_mut().enumerate() {
        let group = &mut coefficients[j * WIDTH..(j + 1) * WIDTH];
        let mut skip = group.iter().take_while(|share| **share == 0.0).count();
        if skip == WIDTH {
            // (Nothing of it reaches the image.)
            *r = (0, -1);
            continue;
        }
        r.0 += skip as i32;
        while r.0 < 0 {
            r.0 += 1;
            skip += 1;
        }
        let kept = (r.1 - r.0 + 1).clamp(0, WIDTH as i32) as usize;
        for i in 0..kept {
            if i + skip >= WIDTH {
                break;
            }
            group[i] = group[i + skip];
        }
        r.1 = r.1.min(output - 1);
    }
}

/// A row of `input` (its pixel at `x` past either edge is the edge's), each channel in 0..=1,
/// filtered to the output's width.
fn row(axis: &Axis, input: &[u8], channels: usize, wide: i32, output_wide: i32) -> Vec<f32> {
    let margin = axis.margin;
    let decoded: Vec<f32> = (-margin..wide + margin)
        .flat_map(|x| {
            let at = x.clamp(0, wide - 1) as usize * channels;
            input[at..at + channels]
                .iter()
                .map(|b| f32::from(*b) / 255.0)
        })
        .collect();
    let pixel = |x: i32| {
        let at = (x + margin) as usize * channels;
        decoded.get(at..at + channels)
    };
    let mut out = vec![0.0f32; output_wide as usize * channels];
    for (n, &(first, last)) in axis.reach.iter().enumerate() {
        for k in first..=last {
            let share = axis.coefficients[n * WIDTH + (k - first) as usize];
            // Growing, output pixel `n` takes of input pixel `k`; shrinking, input pixel `n`
            // (counted from the margin) gives to output pixel `k`.
            let (from, to) = if axis.grows {
                (k, n as i32)
            } else {
                (n as i32 - margin, k)
            };
            let (Some(from), Some(to)) = (
                pixel(from),
                out.get_mut(to as usize * channels..(to as usize + 1) * channels),
            ) else {
                continue;
            };
            for (t, f) in to.iter_mut().zip(from) {
                *t += f * share;
            }
        }
    }
    out
}

/// `stbir_resize_uint8(input, input_w, input_h, input_stride, output, output_w, output_h,
/// output_stride, channels)`: the image scaled to the output's size. A stride of 0 is a row of
/// pixels with nothing between rows. False for sizes or buffers that hold no such images, as the
/// library refuses them.
pub(crate) fn resize(
    input: &[u8],
    size: (i32, i32),
    stride: usize,
    output: &mut [u8],
    output_size: (i32, i32),
    output_stride: usize,
    channels: usize,
) -> bool {
    let ((w, h), (ow, oh)) = (size, output_size);
    if w <= 0 || h <= 0 || ow <= 0 || oh <= 0 || channels == 0 {
        return false;
    }
    let stride = if stride == 0 {
        w as usize * channels
    } else {
        stride
    };
    let output_stride = if output_stride == 0 {
        ow as usize * channels
    } else {
        output_stride
    };
    let holds = |len: usize, rows: i32, stride: usize, wide: i32| {
        stride >= wide as usize * channels
            && len >= (rows as usize - 1) * stride + wide as usize * channels
    };
    if !holds(input.len(), h, stride, w) || !holds(output.len(), oh, output_stride, ow) {
        return false;
    }
    let (across, down) = (axis(w, ow), axis(h, oh));
    // An input row filtered across, by its number (one past an edge is the edge's row).
    let mut rows: std::collections::HashMap<i32, Vec<f32>> = std::collections::HashMap::new();
    let mut filtered = |y: i32| {
        let y = y.clamp(0, h - 1);
        rows.entry(y)
            .or_insert_with(|| row(&across, &input[y as usize * stride..], channels, w, ow))
            .clone()
    };
    let wide = ow as usize * channels;
    let mut out = vec![vec![0.0f32; wide]; oh as usize];
    for (n, &(first, last)) in down.reach.iter().enumerate() {
        for k in first..=last {
            let share = down.coefficients[n * WIDTH + (k - first) as usize];
            let (from, to) = if down.grows {
                (k, n as i32)
            } else {
                (n as i32 - down.margin, k)
            };
            let Some(to) = out.get_mut(to as usize) else {
                continue;
            };
            for (t, f) in to.iter_mut().zip(filtered(from)) {
                *t += f * share;
            }
        }
    }
    for (y, line) in out.iter().enumerate() {
        for (x, value) in line.iter().enumerate() {
            output[y * output_stride + x] = (f64::from(value.clamp(0.0, 1.0) * 255.0) + 0.5) as u8;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    /// The bytes LWJGL 3.3.3's own `stbir_resize_uint8` gives (the library a 1.21.1 install
    /// runs, asked once on this machine) for images grown, shrunk, both ways at once, kept at
    /// their size, of one pixel, of one, three and four channels: `width height to-width
    /// to-height channels input output`.
    const CASES: &str = include_str!("stb_resize_cases.txt");

    #[test]
    fn an_image_is_scaled_to_the_bytes_the_library_gives() {
        let mut seen = 0;
        for line in CASES
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        {
            let part: Vec<&str> = line.split_whitespace().collect();
            let n = |i: usize| part[i].parse::<i32>().unwrap();
            let (size, to, channels) = ((n(0), n(1)), (n(2), n(3)), n(4) as usize);
            let (input, expected) = (bytes(part[5]), bytes(part[6]));
            let mut output = vec![0u8; expected.len()];
            assert!(
                resize(&input, size, 0, &mut output, to, 0, channels),
                "{size:?} to {to:?}"
            );
            assert_eq!(output, expected, "{size:?} to {to:?}, {channels} channels");
            seen += 1;
        }
        assert!(seen >= 10, "{seen} cases");
        // What holds no image is refused.
        assert!(!resize(&[0; 4], (1, 1), 0, &mut [0; 4], (0, 1), 0, 4));
        assert!(!resize(&[0; 3], (1, 1), 0, &mut [0; 4], (1, 1), 0, 4));
    }
}
