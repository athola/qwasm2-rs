//! Quake 2 `.wal` texture loader (C `LoadWal` in `refresh/files/wal.c`).

use super::{fixed_cstring, malformed};
use q2_common::binary::{try_read_i32, try_read_u32};
use q2_common::error::Q2Result;

const WHAT: &str = "wal";
/// `sizeof(miptex_t)`: 32 name + 2*4 size + 4*4 offsets + 32 animname + 3*4.
const HEADER_SIZE: usize = 100;

/// WAL texture; only mip level 0 is kept (the GPU generates the rest).
#[derive(Debug, Clone)]
pub struct WalTexture {
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Palette indices of mip 0, `width * height` bytes.
    pub pixels: Vec<u8>,
    /// Next texture in the animation chain, empty for none.
    pub animname: String,
    pub flags: i32,
    pub contents: i32,
    pub value: i32,
}

pub fn load_wal(data: &[u8]) -> Q2Result<WalTexture> {
    if data.len() < HEADER_SIZE {
        return Err(malformed(WHAT, "small header"));
    }
    let width = try_read_u32(data, 32)?;
    let height = try_read_u32(data, 36)?;
    let offset = try_read_u32(data, 40)?;

    // C reads these as signed ints, so values above i32::MAX count as negative.
    let (width, height, offset) = match (
        i32::try_from(width),
        i32::try_from(height),
        i32::try_from(offset),
    ) {
        (Ok(w), Ok(h), Ok(o)) if w > 0 && h > 0 && o > 0 => (w, h, o),
        _ => return Err(malformed(WHAT, "bad dimensions or offset")),
    };
    let width = usize::try_from(width).map_err(|e| malformed(WHAT, e))?;
    let height = usize::try_from(height).map_err(|e| malformed(WHAT, e))?;
    let offset = usize::try_from(offset).map_err(|e| malformed(WHAT, e))?;

    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| malformed(WHAT, "dimensions overflow"))?;
    let end = offset
        .checked_add(pixel_count)
        .filter(|&end| end <= data.len())
        .ok_or_else(|| malformed(WHAT, "small body"))?;

    Ok(WalTexture {
        name: fixed_cstring(&data[0..32]),
        width,
        height,
        pixels: data[offset..end].to_vec(),
        animname: fixed_cstring(&data[56..88]),
        flags: try_read_i32(data, 88)?,
        contents: try_read_i32(data, 92)?,
        value: try_read_i32(data, 96)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wal(width: u32, height: u32, offset: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; HEADER_SIZE];
        bytes[..9].copy_from_slice(b"e1u1/test");
        bytes[32..36].copy_from_slice(&width.to_le_bytes());
        bytes[36..40].copy_from_slice(&height.to_le_bytes());
        bytes[40..44].copy_from_slice(&offset.to_le_bytes());
        bytes[56..61].copy_from_slice(b"next1");
        bytes[88..92].copy_from_slice(&5i32.to_le_bytes());
        bytes[92..96].copy_from_slice(&(-2i32).to_le_bytes());
        bytes[96..100].copy_from_slice(&7i32.to_le_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    #[test]
    fn parses_header_and_first_mip() {
        let texture = load_wal(&wal(2, 2, 100, &[1, 2, 3, 4, 9, 9])).unwrap();
        assert_eq!(texture.name, "e1u1/test");
        assert_eq!((texture.width, texture.height), (2, 2));
        assert_eq!(texture.pixels, vec![1, 2, 3, 4]);
        assert_eq!(texture.animname, "next1");
        assert_eq!((texture.flags, texture.contents, texture.value), (5, -2, 7));
    }

    #[test]
    fn rejects_small_header() {
        assert!(load_wal(&[0u8; 99]).is_err());
    }

    #[test]
    fn rejects_zero_dimensions_and_zero_offset() {
        assert!(load_wal(&wal(0, 2, 100, &[0; 4])).is_err());
        assert!(load_wal(&wal(2, 0, 100, &[0; 4])).is_err());
        assert!(load_wal(&wal(2, 2, 0, &[0; 4])).is_err());
    }

    #[test]
    fn rejects_body_shorter_than_pixels() {
        assert!(load_wal(&wal(2, 2, 100, &[0; 3])).is_err());
    }

    #[test]
    fn rejects_offset_past_end_and_overflowing_sizes() {
        assert!(load_wal(&wal(1, 1, 5000, &[0; 4])).is_err());
        assert!(load_wal(&wal(u32::MAX, u32::MAX, 100, &[0; 4])).is_err());
        assert!(load_wal(&wal(0x7fff_ffff, 0x7fff_ffff, 100, &[0; 4])).is_err());
        assert!(load_wal(&wal(2, 2, u32::MAX, &[0; 4])).is_err());
    }
}
