//! Quake 2 BSP file header (`dheader_t`) and lump directory.
//!
//! Collision (CM_LoadMap) and the renderer both read the same `.bsp`, as in
//! C; this module is the one place the header is validated, so a lump that
//! falls outside the file is rejected before either loader indexes it.

use crate::binary::try_read_u32;
use crate::error::{Q2Error, Q2Result};

/// Little-endian "IBSP".
pub const IDBSPHEADER: u32 =
    (b'I' as u32) | ((b'B' as u32) << 8) | ((b'S' as u32) << 16) | ((b'P' as u32) << 24);
pub const BSPVERSION: u32 = 38;
pub const HEADER_LUMPS: usize = 19;
/// ident + version + lump directory.
pub const HEADER_SIZE: usize = 8 + HEADER_LUMPS * 8;
/// Size of C `map_entitystring`, terminator included.
pub const MAX_MAP_ENTSTRING: usize = 0x40000;

pub const LUMP_ENTITIES: usize = 0;
pub const LUMP_PLANES: usize = 1;
pub const LUMP_VERTEXES: usize = 2;
pub const LUMP_VISIBILITY: usize = 3;
pub const LUMP_NODES: usize = 4;
pub const LUMP_TEXINFO: usize = 5;
pub const LUMP_FACES: usize = 6;
pub const LUMP_LIGHTING: usize = 7;
pub const LUMP_LEAFS: usize = 8;
pub const LUMP_LEAFFACES: usize = 9;
pub const LUMP_LEAFBRUSHES: usize = 10;
pub const LUMP_EDGES: usize = 11;
pub const LUMP_SURFEDGES: usize = 12;
pub const LUMP_MODELS: usize = 13;
pub const LUMP_BRUSHES: usize = 14;
pub const LUMP_BRUSHSIDES: usize = 15;
pub const LUMP_POP: usize = 16;
pub const LUMP_AREAS: usize = 17;
pub const LUMP_AREAPORTALS: usize = 18;

/// A lump directory entry (`lump_t`), already checked to lie inside the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lump {
    pub offset: usize,
    pub length: usize,
}

/// A validated `dheader_t`.
#[derive(Debug, Clone)]
pub struct BspHeader {
    pub lumps: [Lump; HEADER_LUMPS],
}

impl BspHeader {
    /// Check magic, version and that every lump lies inside `data`.
    pub fn parse(data: &[u8]) -> Q2Result<Self> {
        if data.len() < HEADER_SIZE {
            return Err(Q2Error::Drop("BSP file too small for header".into()));
        }
        let ident = try_read_u32(data, 0)?;
        if ident != IDBSPHEADER {
            return Err(Q2Error::Drop(format!("BSP has wrong magic 0x{ident:08X}")));
        }
        let version = try_read_u32(data, 4)?;
        if version != BSPVERSION {
            return Err(Q2Error::Drop(format!(
                "BSP has wrong version {version} (expected {BSPVERSION})"
            )));
        }

        let mut lumps = [Lump::default(); HEADER_LUMPS];
        for (i, lump) in lumps.iter_mut().enumerate() {
            let base = 8 + i * 8;
            let offset = try_read_u32(data, base)? as usize;
            let length = try_read_u32(data, base + 4)? as usize;
            // checked_add: on wasm32 usize is 32 bits and the sum can wrap.
            if !offset
                .checked_add(length)
                .is_some_and(|end| end <= data.len())
            {
                return Err(Q2Error::Drop(format!(
                    "BSP lump {i} extends past end of file"
                )));
            }
            *lump = Lump { offset, length };
        }
        Ok(Self { lumps })
    }

    /// The bytes of lump `idx`; bounds were checked in `parse`.
    pub fn lump<'a>(&self, data: &'a [u8], idx: usize) -> &'a [u8] {
        let Lump { offset, length } = self.lumps[idx];
        &data[offset..offset + length]
    }

    /// The entity lump as text, like C CMod_LoadEntityString: too large for
    /// `map_entitystring` is ERR_DROP, and the string ends at the first NUL.
    /// Bytes map 1:1 to chars, as for other Q2 strings.
    pub fn entity_string(&self, data: &[u8]) -> Q2Result<String> {
        let raw = self.lump(data, LUMP_ENTITIES);
        if raw.len() + 1 > MAX_MAP_ENTSTRING {
            return Err(Q2Error::Drop("Map has too large entity lump".into()));
        }
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        Ok(raw[..end].iter().map(|&b| char::from(b)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Header whose only non-empty lump is the entity lump, at offset 160.
    fn header_with(entities: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&IDBSPHEADER.to_le_bytes());
        buf.extend_from_slice(&BSPVERSION.to_le_bytes());
        let data_start = (HEADER_SIZE) as u32;
        for i in 0..HEADER_LUMPS {
            let len = if i == LUMP_ENTITIES {
                entities.len() as u32
            } else {
                0
            };
            buf.extend_from_slice(&data_start.to_le_bytes());
            buf.extend_from_slice(&len.to_le_bytes());
        }
        buf.extend_from_slice(entities);
        buf
    }

    fn set_lump(buf: &mut [u8], idx: usize, ofs: u32, len: u32) {
        let base = 8 + idx * 8;
        buf[base..base + 4].copy_from_slice(&ofs.to_le_bytes());
        buf[base + 4..base + 8].copy_from_slice(&len.to_le_bytes());
    }

    #[test]
    fn parse_accepts_valid_header_and_exposes_lumps() {
        let buf = header_with(b"{ }\0");
        let header = BspHeader::parse(&buf).unwrap();
        assert_eq!(header.lump(&buf, LUMP_ENTITIES), b"{ }\0");
        assert!(header.lump(&buf, LUMP_PLANES).is_empty());
    }

    #[test]
    fn parse_rejects_short_header() {
        let buf = header_with(b"");
        assert!(matches!(
            BspHeader::parse(&buf[..HEADER_SIZE - 1]),
            Err(Q2Error::Drop(_))
        ));
    }

    #[test]
    fn parse_rejects_bad_magic_and_version() {
        let mut buf = header_with(b"");
        buf[0] = b'X';
        assert!(matches!(BspHeader::parse(&buf), Err(Q2Error::Drop(_))));

        let mut buf = header_with(b"");
        buf[4..8].copy_from_slice(&37u32.to_le_bytes());
        assert!(matches!(BspHeader::parse(&buf), Err(Q2Error::Drop(_))));
    }

    #[test]
    fn parse_rejects_lump_past_end_or_overflowing() {
        let mut buf = header_with(b"");
        set_lump(&mut buf, LUMP_NODES, HEADER_SIZE as u32, 1);
        assert!(matches!(BspHeader::parse(&buf), Err(Q2Error::Drop(_))));

        // offset + length wraps a 32-bit usize (wasm32); must still fail.
        let mut buf = header_with(b"");
        set_lump(&mut buf, LUMP_NODES, u32::MAX, 2);
        assert!(matches!(BspHeader::parse(&buf), Err(Q2Error::Drop(_))));
    }

    #[test]
    fn entity_string_stops_at_nul_and_keeps_bytes_like_c() {
        // C copies the lump and uses it as a C string; bytes map 1:1.
        let buf = header_with(b"{ \"message\" \"caf\xe9\" }\0junk");
        let header = BspHeader::parse(&buf).unwrap();
        assert_eq!(
            header.entity_string(&buf).unwrap(),
            "{ \"message\" \"caf\u{e9}\" }"
        );
    }

    #[test]
    fn entity_string_rejects_lump_too_large_for_c_buffer() {
        // C: filelen + 1 > sizeof(map_entitystring) is ERR_DROP.
        let big = vec![b'a'; MAX_MAP_ENTSTRING];
        let buf = header_with(&big);
        let header = BspHeader::parse(&buf).unwrap();
        assert!(matches!(header.entity_string(&buf), Err(Q2Error::Drop(_))));

        let fits = vec![b'a'; MAX_MAP_ENTSTRING - 1];
        let buf = header_with(&fits);
        let header = BspHeader::parse(&buf).unwrap();
        assert_eq!(header.entity_string(&buf).unwrap().len(), fits.len());
    }
}
