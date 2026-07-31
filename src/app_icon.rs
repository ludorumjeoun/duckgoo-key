use std::cmp::Ordering;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};

use iced::widget::image::Handle;
use icns::{Encoding, IconFamily, IconType, PixelFormat};
use thiserror::Error;

/// Physical pixels retained for an icon shown in a 40 logical-pixel row.
///
/// Keeping decoded icons at 2x display density avoids storing large source
/// representations (often 512 or 1024 pixels wide) for every discovered app.
const TARGET_PIXEL_SIZE: u32 = 80;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedAppIcon {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl DecodedAppIcon {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    pub fn into_handle(self) -> Handle {
        Handle::from_rgba(self.width, self.height, self.rgba)
    }
}

#[derive(Debug, Error)]
pub enum AppIconError {
    #[error("failed to open ICNS file at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read ICNS file at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("ICNS file at {path} contains no decodable icon representations")]
    NoRepresentation { path: PathBuf },
    #[error("failed to decode {width}x{height} ICNS representation at {path}: {source}")]
    DecodeRepresentation {
        path: PathBuf,
        width: u32,
        height: u32,
        #[source]
        source: io::Error,
    },
    #[error(
        "decoded ICNS representation at {path} returned {actual} RGBA bytes; expected {expected}"
    )]
    InvalidPixelData {
        path: PathBuf,
        expected: usize,
        actual: usize,
    },
}

pub fn load_icns(path: &Path) -> Result<Handle, AppIconError> {
    decode_icns(path).map(DecodedAppIcon::into_handle)
}

pub fn decode_icns(path: &Path) -> Result<DecodedAppIcon, AppIconError> {
    let file = File::open(path).map_err(|source| AppIconError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let family = IconFamily::read(BufReader::new(file)).map_err(|source| AppIconError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let icon_type = choose_icon_type(&family.available_icons()).ok_or_else(|| {
        AppIconError::NoRepresentation {
            path: path.to_path_buf(),
        }
    })?;
    let image = family.get_icon_with_type(icon_type).map_err(|source| {
        AppIconError::DecodeRepresentation {
            path: path.to_path_buf(),
            width: icon_type.pixel_width(),
            height: icon_type.pixel_height(),
            source,
        }
    })?;
    let rgba = image.convert_to(PixelFormat::RGBA);
    let source_width = rgba.width();
    let source_height = rgba.height();
    let source_pixels = rgba.into_data().into_vec();

    validate_pixel_data(path, source_width, source_height, &source_pixels)?;

    let (width, height) = retained_dimensions(source_width, source_height);
    let rgba = if (width, height) == (source_width, source_height) {
        source_pixels
    } else {
        resize_rgba(&source_pixels, source_width, source_height, width, height)
    };

    Ok(DecodedAppIcon {
        width,
        height,
        rgba,
    })
}

fn choose_icon_type(icon_types: &[IconType]) -> Option<IconType> {
    icon_types.iter().copied().min_by(compare_icon_types)
}

fn compare_icon_types(left: &IconType, right: &IconType) -> Ordering {
    let left_width = left.pixel_width();
    let left_height = left.pixel_height();
    let right_width = right.pixel_width();
    let right_height = right.pixel_height();
    let left_is_large_enough = left_width >= TARGET_PIXEL_SIZE && left_height >= TARGET_PIXEL_SIZE;
    let right_is_large_enough =
        right_width >= TARGET_PIXEL_SIZE && right_height >= TARGET_PIXEL_SIZE;

    match (left_is_large_enough, right_is_large_enough) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (true, true) => left_width
            .max(left_height)
            .cmp(&right_width.max(right_height))
            .then_with(|| {
                pixel_area(left_width, left_height).cmp(&pixel_area(right_width, right_height))
            })
            .then_with(|| encoding_rank(*left).cmp(&encoding_rank(*right))),
        (false, false) => right_width
            .min(right_height)
            .cmp(&left_width.min(left_height))
            .then_with(|| {
                pixel_area(right_width, right_height).cmp(&pixel_area(left_width, left_height))
            })
            .then_with(|| encoding_rank(*left).cmp(&encoding_rank(*right))),
    }
}

fn encoding_rank(icon_type: IconType) -> u8 {
    if icon_type.encoding() == Encoding::JP2PNG {
        0
    } else {
        1
    }
}

fn pixel_area(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height)
}

fn validate_pixel_data(
    path: &Path,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<(), AppIconError> {
    let expected = usize::try_from(pixel_area(width, height))
        .ok()
        .and_then(|area| area.checked_mul(4))
        .unwrap_or(usize::MAX);

    if width == 0 || height == 0 || pixels.len() != expected {
        return Err(AppIconError::InvalidPixelData {
            path: path.to_path_buf(),
            expected,
            actual: pixels.len(),
        });
    }

    Ok(())
}

fn retained_dimensions(width: u32, height: u32) -> (u32, u32) {
    let longest_side = width.max(height);
    if longest_side <= TARGET_PIXEL_SIZE {
        return (width, height);
    }

    if width >= height {
        let scaled_height = (u64::from(height) * u64::from(TARGET_PIXEL_SIZE)
            + u64::from(width) / 2)
            / u64::from(width);
        (TARGET_PIXEL_SIZE, scaled_height.max(1) as u32)
    } else {
        let scaled_width = (u64::from(width) * u64::from(TARGET_PIXEL_SIZE)
            + u64::from(height) / 2)
            / u64::from(height);
        (scaled_width.max(1) as u32, TARGET_PIXEL_SIZE)
    }
}

fn resize_rgba(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Vec<u8> {
    let target_len = usize::try_from(pixel_area(target_width, target_height) * 4)
        .expect("retained icon dimensions fit in memory");
    let mut target = vec![0; target_len];
    let scale_x = source_width as f32 / target_width as f32;
    let scale_y = source_height as f32 / target_height as f32;

    for target_y in 0..target_height {
        let source_y = ((target_y as f32 + 0.5) * scale_y - 0.5)
            .clamp(0.0, source_height.saturating_sub(1) as f32);
        let y0 = source_y.floor() as u32;
        let y1 = (y0 + 1).min(source_height - 1);
        let y_weight = source_y - y0 as f32;

        for target_x in 0..target_width {
            let source_x = ((target_x as f32 + 0.5) * scale_x - 0.5)
                .clamp(0.0, source_width.saturating_sub(1) as f32);
            let x0 = source_x.floor() as u32;
            let x1 = (x0 + 1).min(source_width - 1);
            let x_weight = source_x - x0 as f32;
            let samples = [
                (x0, y0, (1.0 - x_weight) * (1.0 - y_weight)),
                (x1, y0, x_weight * (1.0 - y_weight)),
                (x0, y1, (1.0 - x_weight) * y_weight),
                (x1, y1, x_weight * y_weight),
            ];

            let mut alpha = 0.0;
            let mut premultiplied = [0.0; 3];
            for (sample_x, sample_y, weight) in samples {
                let source_offset = pixel_offset(source_width, sample_x, sample_y);
                let sample_alpha = source[source_offset + 3] as f32;
                alpha += sample_alpha * weight;
                for channel in 0..3 {
                    premultiplied[channel] +=
                        source[source_offset + channel] as f32 * sample_alpha * weight;
                }
            }

            let target_offset = pixel_offset(target_width, target_x, target_y);
            if alpha > f32::EPSILON {
                for channel in 0..3 {
                    target[target_offset + channel] =
                        (premultiplied[channel] / alpha).round().clamp(0.0, 255.0) as u8;
                }
            }
            target[target_offset + 3] = alpha.round().clamp(0.0, 255.0) as u8;
        }
    }

    target
}

fn pixel_offset(width: u32, x: u32, y: u32) -> usize {
    (u64::from(y) * u64::from(width) + u64::from(x)) as usize * 4
}

#[cfg(test)]
mod tests {
    use std::io::Seek;

    use icns::{IconFamily, IconType, Image, PixelFormat};
    use tempfile::NamedTempFile;

    use super::{
        TARGET_PIXEL_SIZE, choose_icon_type, decode_icns, resize_rgba, retained_dimensions,
    };

    #[test]
    fn chooses_smallest_retina_sized_representation() {
        let icon_type = choose_icon_type(&[
            IconType::RGBA32_512x512,
            IconType::RGBA32_64x64,
            IconType::RGB24_128x128,
            IconType::RGBA32_128x128,
            IconType::RGBA32_256x256,
        ]);

        assert_eq!(icon_type, Some(IconType::RGBA32_128x128));
    }

    #[test]
    fn chooses_largest_representation_when_all_are_smaller_than_target() {
        let icon_type = choose_icon_type(&[
            IconType::RGBA32_16x16,
            IconType::RGBA32_64x64,
            IconType::RGBA32_32x32,
        ]);

        assert_eq!(icon_type, Some(IconType::RGBA32_64x64));
    }

    #[test]
    fn retains_aspect_ratio_and_does_not_upscale() {
        assert_eq!(retained_dimensions(160, 80), (80, 40));
        assert_eq!(retained_dimensions(80, 160), (40, 80));
        assert_eq!(retained_dimensions(64, 64), (64, 64));
    }

    #[test]
    fn resize_preserves_solid_semitransparent_color() {
        let pixel = [12, 48, 220, 128];
        let source = pixel.repeat(128 * 128);
        let resized = resize_rgba(&source, 128, 128, 80, 80);

        assert_eq!(resized.len(), 80 * 80 * 4);
        assert!(resized.chunks_exact(4).all(|value| value == pixel));
    }

    #[test]
    fn decodes_selected_representation_and_caps_retained_pixels() {
        let mut family = IconFamily::new();
        let small = solid_image(64, [220, 30, 40, 255]);
        let retina = solid_image(128, [20, 160, 70, 230]);
        family
            .add_icon_with_type(&small, IconType::RGBA32_64x64)
            .expect("encode 64px icon");
        family
            .add_icon_with_type(&retina, IconType::RGBA32_128x128)
            .expect("encode 128px icon");

        let mut file = NamedTempFile::new().expect("create temporary ICNS");
        family.write(&mut file).expect("write temporary ICNS");
        file.rewind().expect("rewind temporary ICNS");

        let decoded = decode_icns(file.path()).expect("decode temporary ICNS");

        assert_eq!(decoded.width(), TARGET_PIXEL_SIZE);
        assert_eq!(decoded.height(), TARGET_PIXEL_SIZE);
        assert_eq!(decoded.rgba().len(), 80 * 80 * 4);
        assert!(
            decoded
                .rgba()
                .chunks_exact(4)
                .all(|value| value == [20, 160, 70, 230])
        );
    }

    fn solid_image(size: u32, pixel: [u8; 4]) -> Image {
        let pixels = pixel.repeat((size * size) as usize);
        Image::from_data(PixelFormat::RGBA, size, size, pixels).expect("valid test image")
    }
}
