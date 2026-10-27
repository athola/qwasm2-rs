//! Sprite (`.sp2`) loader (C `Mod_LoadSP2` in `refresh/files/models.c`).

use super::{fixed_cstring, malformed, non_negative, slice_at};
use q2_common::binary::{try_read_i32, try_read_u32};
use q2_common::error::Q2Result;

const WHAT: &str = "sp2";
/// Little-endian "IDS2".
pub const SP2_IDENT: u32 = 0x3253_4449;
pub const SP2_VERSION: i32 = 2;
const HEADER_SIZE: usize = 12;
const MAX_SKINNAME: usize = 64;
const FRAME_SIZE: usize = 16 + MAX_SKINNAME;
/// C `MAX_MD2SKINS`: frames beyond this are ignored.
const MAX_FRAMES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sp2Frame {
    pub width: i32,
    pub height: i32,
    pub origin_x: i32,
    pub origin_y: i32,
    /// Name of the PCX holding this frame's pixels.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sp2Sprite {
    pub frames: Vec<Sp2Frame>,
}

pub fn load_sp2(data: &[u8]) -> Q2Result<Sp2Sprite> {
    if data.len() < HEADER_SIZE {
        return Err(malformed(WHAT, "file smaller than header"));
    }
    if try_read_u32(data, 0)? != SP2_IDENT {
        return Err(malformed(WHAT, "bad ident"));
    }
    let version = try_read_i32(data, 4)?;
    if version != SP2_VERSION {
        return Err(malformed(
            WHAT,
            format_args!("version {version}, expected {SP2_VERSION}"),
        ));
    }
    // C clamps an excessive count and ignores the extra frames.
    let count = non_negative(WHAT, "numframes", try_read_i32(data, 8)?)?.min(MAX_FRAMES);

    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let raw = slice_at(WHAT, data, HEADER_SIZE + index * FRAME_SIZE, FRAME_SIZE)?;
        frames.push(Sp2Frame {
            width: try_read_i32(raw, 0)?,
            height: try_read_i32(raw, 4)?,
            origin_x: try_read_i32(raw, 8)?,
            origin_y: try_read_i32(raw, 12)?,
            name: fixed_cstring(&raw[16..]),
        });
    }
    Ok(Sp2Sprite { frames })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp2(ident: &[u8; 4], version: i32, count: i32, frames: &[(i32, i32, &str)]) -> Vec<u8> {
        let mut bytes = ident.to_vec();
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        for &(width, height, name) in frames {
            for value in [width, height, width / 2, height / 2] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let mut field = [0u8; MAX_SKINNAME];
            field[..name.len()].copy_from_slice(name.as_bytes());
            bytes.extend_from_slice(&field);
        }
        bytes
    }

    #[test]
    fn parses_frames() {
        let bytes = sp2(b"IDS2", 2, 2, &[(16, 8, "sprites/a.pcx"), (4, 4, "b.pcx")]);
        let sprite = load_sp2(&bytes).unwrap();
        assert_eq!(sprite.frames.len(), 2);
        assert_eq!(
            sprite.frames[0],
            Sp2Frame {
                width: 16,
                height: 8,
                origin_x: 8,
                origin_y: 4,
                name: "sprites/a.pcx".to_string()
            }
        );
        assert_eq!(sprite.frames[1].name, "b.pcx");
    }

    #[test]
    fn rejects_bad_ident_and_version() {
        assert!(load_sp2(&sp2(b"IDP2", 2, 0, &[])).is_err());
        assert!(load_sp2(&sp2(b"IDS2", 3, 0, &[])).is_err());
    }

    #[test]
    fn rejects_truncated_header_and_frames() {
        assert!(load_sp2(&[]).is_err());
        assert!(load_sp2(&sp2(b"IDS2", 2, 0, &[])[..11]).is_err());
        let mut bytes = sp2(b"IDS2", 2, 1, &[(1, 1, "a")]);
        bytes.pop();
        assert!(load_sp2(&bytes).is_err());
    }

    #[test]
    fn rejects_negative_count_and_count_without_frame_data() {
        assert!(load_sp2(&sp2(b"IDS2", 2, -1, &[])).is_err());
        assert!(load_sp2(&sp2(b"IDS2", 2, 1, &[])).is_err());
    }

    #[test]
    fn ignores_frames_beyond_the_limit() {
        let frames = vec![(2, 2, "f.pcx"); 33];
        let sprite = load_sp2(&sp2(b"IDS2", 2, 33, &frames)).unwrap();
        assert_eq!(sprite.frames.len(), MAX_FRAMES);
    }
}
