//! Parsers for Quake 2 asset formats the renderer consumes: PCX, WAL, MD2, SP2, TGA.
//!
//! All parsers take a byte slice, never read out of bounds, and report
//! malformed data as `Q2Error::Drop` (C `Com_Error(ERR_DROP)` / failed load).

mod anorms;
pub mod md2;
pub mod palette;
pub mod pcx;
pub mod sp2;
pub mod tga;
pub mod wal;

use q2_common::error::{Q2Error, Q2Result};

/// Build the error every parser returns for malformed input.
pub(crate) fn malformed(what: &str, why: impl std::fmt::Display) -> Q2Error {
    Q2Error::Drop(format!("{what}: {why}"))
}

/// Decode a NUL-terminated string from a fixed-size on-disk field, one char
/// per byte (Latin-1) as q2-common decodes texinfo and pak names, so a
/// high-bit name is the same string in the renderer and in collision.
pub(crate) fn fixed_cstring(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| char::from(b))
        .collect()
}

/// Convert an on-disk signed count or offset to `usize`, rejecting negatives.
pub(crate) fn non_negative(what: &str, field: &str, value: i32) -> Q2Result<usize> {
    usize::try_from(value).map_err(|_| malformed(what, format_args!("negative {field} {value}")))
}

/// Borrow `len` bytes at `off`, failing instead of panicking when out of range.
pub(crate) fn slice_at<'a>(
    what: &str,
    data: &'a [u8],
    off: usize,
    len: usize,
) -> Q2Result<&'a [u8]> {
    off.checked_add(len)
        .and_then(|end| data.get(off..end))
        .ok_or_else(|| {
            malformed(
                what,
                format_args!("{len} bytes at offset {off} exceed file"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::fixed_cstring;

    #[test]
    fn fixed_cstring_maps_each_byte_to_one_char_and_stops_at_nul() {
        assert_eq!(fixed_cstring(b"e1/caf\xE9\0junk"), "e1/caf\u{e9}");
        assert_eq!(fixed_cstring(b"\xFF\x80"), "\u{ff}\u{80}");
    }
}

#[cfg(test)]
mod real_assets {
    use super::md2::{load_md2, VERTEX_NORMALS};
    use super::palette::Palette;
    use super::pcx::load_pcx;
    use super::sp2::load_sp2;
    use super::wal::load_wal;
    use q2_common::filesystem::{FileSystem, Pack};
    use std::path::PathBuf;

    /// Open `pak0.pak` from `Q2_GAMEDATA_DIR` or `<repo>/gamedata`; `None` when absent.
    fn open_baseq2() -> Option<FileSystem> {
        let root = std::env::var_os("Q2_GAMEDATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../gamedata"));
        let pak_path = root.join("baseq2/pak0.pak");
        if !pak_path.is_file() {
            // Visible with `--nocapture` or on failure; CI has no game data.
            eprintln!("SKIPPED: {} not found", pak_path.display());
            return None;
        }
        let mut filesystem = FileSystem::new("baseq2");
        filesystem.add_pack(Pack::load(&pak_path).expect("pak0.pak parses"));
        Some(filesystem)
    }

    #[test]
    fn parses_stock_assets_from_pak0() {
        let Some(filesystem) = open_baseq2() else {
            return;
        };

        let colormap = filesystem.load_file("pics/colormap.pcx").unwrap();
        let palette = Palette::from_pcx(&colormap).unwrap();
        assert_eq!(palette.rgba(255)[3], 0);
        let image = load_pcx(&colormap).unwrap();
        assert_eq!(image.pixels.len(), image.width * image.height);

        let wal_names = filesystem.list_files("wal");
        let wal_name = wal_names
            .iter()
            .find(|name| name.starts_with("textures/"))
            .expect("pak0 holds textures");
        let texture = load_wal(&filesystem.load_file(wal_name).unwrap()).unwrap();
        assert_eq!(texture.pixels.len(), texture.width * texture.height);

        let model = load_md2(
            &filesystem
                .load_file("models/monsters/soldier/tris.md2")
                .unwrap(),
        )
        .unwrap();
        assert!(!model.frames.is_empty() && !model.triangles.is_empty());
        let normal_limit = u8::try_from(VERTEX_NORMALS.len()).unwrap_or(u8::MAX);
        assert!(model.frames[0]
            .verts
            .iter()
            .all(|vertex| vertex.normal_index < normal_limit));

        let sprite_names = filesystem.list_files("sp2");
        let sprite_name = sprite_names.first().expect("pak0 holds sprites");
        assert!(!load_sp2(&filesystem.load_file(sprite_name).unwrap())
            .unwrap()
            .frames
            .is_empty());
    }
}
