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
    fn index_255_is_transparent_others_opaque() {
        let palette = Palette::from_pcx(&pcx_with_palette(1)).unwrap();
        assert_eq!(palette.rgba(255), [255, 0, 1, 0]);
        assert_eq!(palette.rgba(7), [7, 8, 9, 255]);
    }
}
