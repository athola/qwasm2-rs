//! ZSoft PCX loader (C `LoadPCX` in `refresh/files/pcx.c`). Not ported:
//! `fixQuitScreen`, which patches 11 rows of the stock baseq2 `pics/quit.pcx`
//! identified by name and checksum.

use super::palette::rgb_entries;
use super::{malformed, slice_at};
use q2_common::binary::try_read_u16;
use q2_common::error::Q2Result;

const WHAT: &str = "pcx";
/// Fixed 128-byte PCX header (`pcx_t` minus its trailing `data` byte).
const HEADER_SIZE: usize = 128;
const PALETTE_BYTES: usize = 768;
/// C rejects `xmax - xmin >= 4096`.
const MAX_DIMENSION: i32 = 4096;

/// Decoded 8-bit PCX image.
#[derive(Debug, Clone)]
pub struct PcxImage {
    pub width: usize,
    pub height: usize,
    /// Palette indices, row-major, `width * height` bytes.
    pub pixels: Vec<u8>,
    /// Trailing 768-byte palette, when the file is long enough to hold one.
    pub palette: Option<[[u8; 3]; 256]>,
}

/// Decode a PCX file. Truncated RLE data is an error (C marks it "malformed").
pub fn load_pcx(data: &[u8]) -> Q2Result<PcxImage> {
    // C: `len < sizeof(pcx_t)` where pcx_t includes the 1-byte `data` field.
    if data.len() < HEADER_SIZE + 1 {
        return Err(malformed(WHAT, "file smaller than header"));
    }
    let manufacturer = data[0];
    let version = data[1];
    let encoding = data[2];
    let bits_per_pixel = data[3];
    let xmin = i32::from(try_read_u16(data, 4)?);
    let ymin = i32::from(try_read_u16(data, 6)?);
    let xmax = i32::from(try_read_u16(data, 8)?);
    let ymax = i32::from(try_read_u16(data, 10)?);
    let span_x = xmax - xmin;
    let span_y = ymax - ymin;

    if manufacturer != 0x0a
        || version != 5
        || encoding != 1
        || bits_per_pixel != 8
        || !(0..MAX_DIMENSION).contains(&span_x)
        || !(0..MAX_DIMENSION).contains(&span_y)
    {
        return Err(malformed(WHAT, "unsupported or corrupt header"));
    }
    // Both spans are in 0..4096 here, so these conversions cannot fail.
    let width = usize::try_from(span_x + 1).map_err(|e| malformed(WHAT, e))?;
    let height = usize::try_from(span_y + 1).map_err(|e| malformed(WHAT, e))?;
    let pixel_count = width * height;

    let palette = if data.len() > PALETTE_BYTES {
        let raw = slice_at(WHAT, data, data.len() - PALETTE_BYTES, PALETTE_BYTES)?;
        Some(rgb_entries(raw))
    } else {
        None
    };

    let mut pixels = Vec::with_capacity(pixel_count);
    let mut cursor = HEADER_SIZE;
    for row in 0..height {
        let row_end = (row + 1) * width;
        while pixels.len() < row_end {
            let mut byte = *data
                .get(cursor)
                .ok_or_else(|| malformed(WHAT, "truncated RLE data"))?;
            cursor += 1;
            let mut run = 1usize;
            if byte & 0xC0 == 0xC0 {
                run = usize::from(byte & 0x3F);
                byte = *data
                    .get(cursor)
                    .ok_or_else(|| malformed(WHAT, "truncated RLE run"))?;
                cursor += 1;
            }
            // What a run leaves past the row end is dropped: C writes it at
            // the start of the next row, which then overwrites it, and past
            // the last row it has "no place for write".
            let room = row_end - pixels.len();
            pixels.resize(pixels.len() + run.min(room), byte);
        }
    }

    Ok(PcxImage {
        width,
        height,
        pixels,
        palette,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[0] = 0x0a;
        bytes[1] = 5;
        bytes[2] = 1;
        bytes[3] = 8;
        bytes[8..10].copy_from_slice(&(width - 1).to_le_bytes());
        bytes[10..12].copy_from_slice(&(height - 1).to_le_bytes());
        bytes
    }

    fn with_palette(mut bytes: Vec<u8>) -> Vec<u8> {
        for i in 0..=255u8 {
            bytes.extend_from_slice(&[i, 0, 255 - i]);
        }
        bytes
    }

    #[test]
    fn decodes_literals_and_rle_run() {
        let mut bytes = header(4, 2);
        // literal 5, run of 3 x 9, run of 4 x 7
        bytes.extend_from_slice(&[5, 0xC3, 9, 0xC4, 7]);
        let image = load_pcx(&with_palette(bytes)).unwrap();
        assert_eq!((image.width, image.height), (4, 2));
        assert_eq!(image.pixels, vec![5, 9, 9, 9, 7, 7, 7, 7]);
        let palette = image.palette.unwrap();
        assert_eq!(palette[3], [3, 0, 252]);
    }

    #[test]
    fn run_past_end_of_image_is_clipped() {
        let mut bytes = header(2, 1);
        bytes.extend_from_slice(&[0xFF, 4]); // run of 63 into a 2-pixel image
        let image = load_pcx(&bytes).unwrap();
        assert_eq!(image.pixels, vec![4, 4]);
        assert!(image.palette.is_none());
    }

    // C LoadPCX decodes row by row: a run's overflow lands at the start of
    // the next row, which that row then overwrites from x = 0.
    #[test]
    fn run_crossing_a_row_end_is_dropped_at_the_row_end_like_c() {
        let mut bytes = header(2, 2);
        bytes.extend_from_slice(&[0xC3, 9, 1, 2]); // 3 x 9 into a 2-wide row
        assert_eq!(load_pcx(&bytes).unwrap().pixels, vec![9, 9, 1, 2]);
    }

    #[test]
    fn rejects_bad_header_fields() {
        for (index, bad) in [(0, 0x0b), (1, 4), (2, 0), (3, 4)] {
            let mut bytes = header(2, 1);
            bytes[index] = bad;
            bytes.extend_from_slice(&[1, 2]);
            assert!(load_pcx(&bytes).is_err(), "header byte {index}");
        }
    }

    #[test]
    fn rejects_oversized_and_inverted_dimensions() {
        let mut bytes = header(2, 1);
        bytes[8..10].copy_from_slice(&4096u16.to_le_bytes());
        bytes.extend_from_slice(&[1, 2]);
        assert!(load_pcx(&bytes).is_err());

        let mut bytes = header(2, 1);
        bytes[4..6].copy_from_slice(&9u16.to_le_bytes()); // xmin > xmax
        bytes.extend_from_slice(&[1, 2]);
        assert!(load_pcx(&bytes).is_err());
    }

    #[test]
    fn rejects_truncated_file_and_header() {
        assert!(load_pcx(&[]).is_err());
        assert!(load_pcx(&header(2, 1)).is_err()); // header only
    }

    #[test]
    fn rejects_truncated_rle_stream() {
        let mut bytes = header(4, 1);
        bytes.extend_from_slice(&[1, 2]); // only 2 of 4 pixels
        assert!(load_pcx(&bytes).is_err());

        let mut bytes = header(4, 1);
        bytes.push(0xC4); // run count with no value byte
        assert!(load_pcx(&bytes).is_err());
    }
}
