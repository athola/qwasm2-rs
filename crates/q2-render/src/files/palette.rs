//! 256-colour palette from `pics/colormap.pcx` (C `R_GetPalette` / `GetPCXPalette`).

use super::{malformed, slice_at};
use q2_common::error::Q2Result;

/// Palette index that is transparent in 32-bit conversion (`d_8to24table[255]`).
pub const TRANSPARENT_INDEX: u8 = 255;

const PALETTE_BYTES: usize = 768;

/// 256 RGB entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(pub [[u8; 3]; 256]);

impl Palette {
    /// Read the 768-byte palette stored at the very end of a PCX file.
    pub fn from_pcx(pcx: &[u8]) -> Q2Result<Palette> {
        const WHAT: &str = "palette";
        // C GetPCXPalette requires len > 768 so the image header precedes it.
        if pcx.len() <= PALETTE_BYTES {
            return Err(malformed(WHAT, "pcx too small to hold a palette"));
        }
        let raw = slice_at(WHAT, pcx, pcx.len() - PALETTE_BYTES, PALETTE_BYTES)?;
        Ok(Palette(rgb_entries(raw)))
    }

    /// RGBA for an index; index 255 has alpha 0, all others 255.
    pub fn rgba(&self, index: u8) -> [u8; 4] {
        let [r, g, b] = self.0[usize::from(index)];
        let alpha = if index == TRANSPARENT_INDEX { 0 } else { 255 };
        [r, g, b, alpha]
    }

    /// C `GL3_Upload8`: RGBA for every pixel of a `width`-wide 8-bit image. A
    /// transparent pixel keeps alpha 0 but takes the RGB of an opaque
    /// neighbour (above, below, left, right in C's order and bounds, else
    /// index 0), so bilinear filtering and mipmaps do not pull palette 255's
    /// own colour in as a fringe.
    pub fn indexed_to_rgba(&self, pixels: &[u8], width: usize) -> Vec<u8> {
        let count = pixels.len();
        let opaque = |i: usize| Some(pixels[i]).filter(|&p| p != TRANSPARENT_INDEX);
        let mut rgba = Vec::with_capacity(count * 4);
        for (i, &index) in pixels.iter().enumerate() {
            let mut color = self.rgba(index);
            if index == TRANSPARENT_INDEX {
                let neighbour = (i > width)
                    .then(|| opaque(i - width))
                    .flatten()
                    .or_else(|| (i + width < count).then(|| opaque(i + width)).flatten())
                    .or_else(|| (i > 0).then(|| opaque(i - 1)).flatten())
                    .or_else(|| (i + 1 < count).then(|| opaque(i + 1)).flatten())
                    .unwrap_or(0);
                color[..3].copy_from_slice(&self.0[usize::from(neighbour)]);
            }
            rgba.extend_from_slice(&color);
        }
        rgba
    }
}

/// Split exactly 768 bytes into 256 RGB triples.
pub(crate) fn rgb_entries(raw: &[u8]) -> [[u8; 3]; 256] {
    let mut entries = [[0u8; 3]; 256];
    for (entry, rgb) in entries.iter_mut().zip(raw.chunks_exact(3)) {
        entry.copy_from_slice(rgb);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcx_with_palette(prefix_len: usize) -> Vec<u8> {
        let mut bytes = vec![0xAAu8; prefix_len];
        for i in 0..=255u8 {
            bytes.extend_from_slice(&[i, i.wrapping_add(1), i.wrapping_add(2)]);
        }
        bytes
    }

    #[test]
    fn reads_palette_from_file_tail() {
        let palette = Palette::from_pcx(&pcx_with_palette(10)).unwrap();
        assert_eq!(palette.0[0], [0, 1, 2]);
        assert_eq!(palette.0[200], [200, 201, 202]);
    }

    #[test]
    fn rejects_file_no_larger_than_palette() {
        assert!(Palette::from_pcx(&pcx_with_palette(0)).is_err());
        assert!(Palette::from_pcx(&[]).is_err());
    }

    #[test]
    fn transparent_pixels_take_the_colour_of_a_neighbour_like_c_upload8() {
        let palette = Palette::from_pcx(&pcx_with_palette(1)).unwrap();
        // 3x3, C order: above, below, left, right, else index 0.
        #[rustfmt::skip]
        let pixels = [
            10, 255, 12,
            255, 255, 30,
            40, 255, 255,
        ];
        let rgba = palette.indexed_to_rgba(&pixels, 3);
        let at = |i: usize| {
            [
                rgba[i * 4],
                rgba[i * 4 + 1],
                rgba[i * 4 + 2],
                rgba[i * 4 + 3],
            ]
        };
        assert_eq!(at(0), [10, 11, 12, 255]);
        // i = 1: not `i > width`, below is 255, so left (10).
        assert_eq!(at(1), [10, 11, 12, 0]);
        // i = 3: not `i > width` either, so below (40).
        assert_eq!(at(3), [40, 41, 42, 0]);
        // i = 4: above is 255, below is 255, left is 255, right is 30.
        assert_eq!(at(4), [30, 31, 32, 0]);
        // i = 7: above (i > width) is 255, no row below, left is 40.
        assert_eq!(at(7), [40, 41, 42, 0]);
        // i = 8: above is 30.
        assert_eq!(at(8), [30, 31, 32, 0]);
        let lone = palette.indexed_to_rgba(&[255], 1);
        assert_eq!(lone, vec![0, 1, 2, 0]);
    }

    #[test]
    fn index_255_is_transparent_others_opaque() {
        let palette = Palette::from_pcx(&pcx_with_palette(1)).unwrap();
        assert_eq!(palette.rgba(255), [255, 0, 1, 0]);
        assert_eq!(palette.rgba(7), [7, 8, 9, 255]);
    }
}
