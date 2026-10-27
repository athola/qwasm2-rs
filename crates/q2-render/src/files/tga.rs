//! Targa loader for sky boxes (C `LoadSTB(..., "tga")` in `refresh/files/stb.c`).
//!
//! Supports the two variants Quake 2 content ships: uncompressed (type 2) and
//! RLE (type 10) truecolour, 24 or 32 bits per pixel. Output is RGBA with the
//! first row at the top, matching the other loaders.

use super::{malformed, slice_at};
use q2_common::error::Q2Result;

const WHAT: &str = "tga";
const HEADER_SIZE: usize = 18;
const TYPE_TRUECOLOR: u8 = 2;
const TYPE_TRUECOLOR_RLE: u8 = 10;
/// Image descriptor bit 5: rows are stored top-to-bottom.
const TOP_ORIGIN_BIT: u8 = 0x20;

/// Decoded truecolour image.
#[derive(Debug, Clone)]
pub struct TgaImage {
    pub width: usize,
    pub height: usize,
    /// RGBA8, row-major, top row first, `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

pub fn load_tga(data: &[u8]) -> Q2Result<TgaImage> {
    let header = slice_at(WHAT, data, 0, HEADER_SIZE)?;
    let id_length = usize::from(header[0]);
    let colormap_type = header[1];
    let image_type = header[2];
    let width = usize::from(u16::from_le_bytes([header[12], header[13]]));
    let height = usize::from(u16::from_le_bytes([header[14], header[15]]));
    let bits = header[16];
    let descriptor = header[17];

    if colormap_type != 0 || !matches!(image_type, TYPE_TRUECOLOR | TYPE_TRUECOLOR_RLE) {
        return Err(malformed(WHAT, "only truecolour images are supported"));
    }
    if bits != 24 && bits != 32 {
        return Err(malformed(WHAT, format_args!("unsupported depth {bits}")));
    }
    if width == 0 || height == 0 {
        return Err(malformed(WHAT, "zero dimensions"));
    }
    let bytes_per_pixel = usize::from(bits / 8);
    let pixel_count = width * height;
    let body_start = HEADER_SIZE + id_length;

    let packed = if image_type == TYPE_TRUECOLOR {
        slice_at(WHAT, data, body_start, pixel_count * bytes_per_pixel)?.to_vec()
    } else {
        decode_rle(data, body_start, pixel_count, bytes_per_pixel)?
    };

    let mut rgba = Vec::with_capacity(pixel_count * 4);
    for pixel in packed.chunks_exact(bytes_per_pixel) {
        let alpha = if bytes_per_pixel == 4 { pixel[3] } else { 255 };
        rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], alpha]);
    }

    if descriptor & TOP_ORIGIN_BIT == 0 {
        rgba = rgba
            .chunks_exact(width * 4)
            .rev()
            .flatten()
            .copied()
            .collect();
    }
    Ok(TgaImage {
        width,
        height,
        rgba,
    })
}

/// Expand RLE packets to raw pixels; a packet running past the image is malformed.
fn decode_rle(
    data: &[u8],
    start: usize,
    pixel_count: usize,
    bytes_per_pixel: usize,
) -> Q2Result<Vec<u8>> {
    let total = pixel_count * bytes_per_pixel;
    let mut out = Vec::with_capacity(total);
    let mut at = start;
    while out.len() < total {
        let packet = slice_at(WHAT, data, at, 1)?[0];
        at += 1;
        let run = usize::from(packet & 0x7f) + 1;
        if out.len() + run * bytes_per_pixel > total {
            return Err(malformed(WHAT, "rle packet overruns image"));
        }
        if packet & 0x80 != 0 {
            let pixel = slice_at(WHAT, data, at, bytes_per_pixel)?;
            at += bytes_per_pixel;
            for _ in 0..run {
                out.extend_from_slice(pixel);
            }
        } else {
            let raw = slice_at(WHAT, data, at, run * bytes_per_pixel)?;
            at += run * bytes_per_pixel;
            out.extend_from_slice(raw);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(image_type: u8, width: u16, height: u16, bits: u8, descriptor: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[2] = image_type;
        bytes[12..14].copy_from_slice(&width.to_le_bytes());
        bytes[14..16].copy_from_slice(&height.to_le_bytes());
        bytes[16] = bits;
        bytes[17] = descriptor;
        bytes
    }

    #[test]
    fn uncompressed_24_bit_swaps_bgr_to_rgba_and_flips_bottom_origin() {
        // Bottom-origin file: first stored row is the bottom row of the image.
        let mut file = header(2, 1, 2, 24, 0);
        file.extend_from_slice(&[3, 2, 1]); // bottom row, BGR -> RGB 1,2,3
        file.extend_from_slice(&[6, 5, 4]); // top row, RGB 4,5,6
        let image = load_tga(&file).unwrap();
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(image.rgba, vec![4, 5, 6, 255, 1, 2, 3, 255]);
    }

    #[test]
    fn top_origin_keeps_row_order_and_32_bit_keeps_alpha() {
        let mut file = header(2, 1, 2, 32, TOP_ORIGIN_BIT);
        file.extend_from_slice(&[3, 2, 1, 10]);
        file.extend_from_slice(&[6, 5, 4, 20]);
        let image = load_tga(&file).unwrap();
        assert_eq!(image.rgba, vec![1, 2, 3, 10, 4, 5, 6, 20]);
    }

    #[test]
    fn rle_expands_run_and_raw_packets() {
        let mut file = header(10, 3, 1, 24, TOP_ORIGIN_BIT);
        file.extend_from_slice(&[0x81, 3, 2, 1]); // run of 2 identical pixels
        file.extend_from_slice(&[0x00, 6, 5, 4]); // one raw pixel
        let image = load_tga(&file).unwrap();
        assert_eq!(image.rgba, vec![1, 2, 3, 255, 1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn rle_packet_past_image_end_is_rejected() {
        let mut file = header(10, 1, 1, 24, TOP_ORIGIN_BIT);
        file.extend_from_slice(&[0x81, 3, 2, 1]); // run of 2 into 1 pixel
        assert!(load_tga(&file).is_err());
    }

    #[test]
    fn rejects_truncated_unsupported_and_empty_images() {
        assert!(load_tga(&[0u8; 5]).is_err());
        let mut short = header(2, 2, 2, 24, 0);
        short.extend_from_slice(&[0; 6]);
        assert!(load_tga(&short).is_err());
        assert!(load_tga(&header(1, 1, 1, 8, 0)).is_err()); // colour-mapped
        assert!(load_tga(&header(2, 1, 1, 16, 0)).is_err()); // 16-bit
        assert!(load_tga(&header(2, 0, 1, 24, 0)).is_err());
    }

    #[test]
    fn id_field_is_skipped() {
        let mut file = header(2, 1, 1, 24, TOP_ORIGIN_BIT);
        file[0] = 3;
        file.extend_from_slice(&[9, 9, 9]);
        file.extend_from_slice(&[3, 2, 1]);
        assert_eq!(load_tga(&file).unwrap().rgba, vec![1, 2, 3, 255]);
    }
}
