//! Quake 2 BSP file header (`dheader_t`) and lump directory.
//!
//! Collision (CM_LoadMap) and the renderer both read the same `.bsp`, as in
//! C; this module is the one place the header is validated, so a lump that
//! falls outside the file is rejected before either loader indexes it.

use crate::binary::{try_read_f32, try_read_i16, try_read_i32, try_read_u16, try_read_u32};
use crate::error::{Q2Error, Q2Result};
use q2_shared::Vec3f;

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

/// On-disk `dplane_t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DPlane {
    pub normal: Vec3f,
    pub dist: f32,
    pub plane_type: i32,
}

/// On-disk `dnode_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DNode {
    pub plane_num: i32,
    /// Positive = node index, negative = -(leaf + 1).
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u16,
    pub num_faces: u16,
}

/// On-disk `dleaf_t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DLeaf {
    pub contents: i32,
    pub cluster: i16,
    pub area: i16,
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_leaf_face: u16,
    pub num_leaf_faces: u16,
    pub first_leaf_brush: u16,
    pub num_leaf_brushes: u16,
}

/// On-disk `texinfo_t`.
#[derive(Debug, Clone, PartialEq)]
pub struct DTexinfo {
    /// `[s/t][x, y, z, offset]` texture projection.
    pub vecs: [[f32; 4]; 2],
    pub flags: i32,
    pub value: i32,
    /// Texture name up to the first NUL of the 32-byte field, one char per
    /// byte like other Q2 strings.
    pub texture: String,
    pub next_texinfo: i32,
}

/// On-disk `dmodel_t`, bounds as stored (loaders spread them by one unit).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DModel {
    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub origin: Vec3f,
    pub head_node: i32,
    pub first_face: i32,
    pub num_faces: i32,
}

pub const DPLANE_SIZE: usize = 20;
const DNODE_SIZE: usize = 28;
const DLEAF_SIZE: usize = 28;
const TEXINFO_SIZE: usize = 76;
const DMODEL_SIZE: usize = 48;

fn read_vec3(rec: &[u8], off: usize) -> Q2Result<Vec3f> {
    Ok(Vec3f::new(
        try_read_f32(rec, off)?,
        try_read_f32(rec, off + 4)?,
        try_read_f32(rec, off + 8)?,
    ))
}

fn read_shorts3(rec: &[u8], off: usize) -> Q2Result<[i16; 3]> {
    Ok([
        try_read_i16(rec, off)?,
        try_read_i16(rec, off + 2)?,
        try_read_i16(rec, off + 4)?,
    ])
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

    /// Decode lump `idx` as fixed-size records. A length that is not a whole
    /// number of records is C's "funny lump size" ERR_DROP.
    fn records<T>(
        &self,
        data: &[u8],
        idx: usize,
        size: usize,
        name: &str,
        decode: impl Fn(&[u8]) -> Q2Result<T>,
    ) -> Q2Result<Vec<T>> {
        let raw = self.lump(data, idx);
        if !raw.len().is_multiple_of(size) {
            return Err(Q2Error::Drop(format!("BSP {name} lump has funny size")));
        }
        raw.chunks_exact(size).map(decode).collect()
    }

    pub fn planes(&self, data: &[u8]) -> Q2Result<Vec<DPlane>> {
        self.records(data, LUMP_PLANES, DPLANE_SIZE, "planes", |r| {
            Ok(DPlane {
                normal: read_vec3(r, 0)?,
                dist: try_read_f32(r, 12)?,
                plane_type: try_read_i32(r, 16)?,
            })
        })
    }

    pub fn nodes(&self, data: &[u8]) -> Q2Result<Vec<DNode>> {
        self.records(data, LUMP_NODES, DNODE_SIZE, "nodes", |r| {
            Ok(DNode {
                plane_num: try_read_i32(r, 0)?,
                children: [try_read_i32(r, 4)?, try_read_i32(r, 8)?],
                mins: read_shorts3(r, 12)?,
                maxs: read_shorts3(r, 18)?,
                first_face: try_read_u16(r, 24)?,
                num_faces: try_read_u16(r, 26)?,
            })
        })
    }

    pub fn leafs(&self, data: &[u8]) -> Q2Result<Vec<DLeaf>> {
        self.records(data, LUMP_LEAFS, DLEAF_SIZE, "leafs", |r| {
            Ok(DLeaf {
                contents: try_read_i32(r, 0)?,
                cluster: try_read_i16(r, 4)?,
                area: try_read_i16(r, 6)?,
                mins: read_shorts3(r, 8)?,
                maxs: read_shorts3(r, 14)?,
                first_leaf_face: try_read_u16(r, 20)?,
                num_leaf_faces: try_read_u16(r, 22)?,
                first_leaf_brush: try_read_u16(r, 24)?,
                num_leaf_brushes: try_read_u16(r, 26)?,
            })
        })
    }

    pub fn texinfo(&self, data: &[u8]) -> Q2Result<Vec<DTexinfo>> {
        self.records(data, LUMP_TEXINFO, TEXINFO_SIZE, "texinfo", |r| {
            let mut vecs = [[0.0f32; 4]; 2];
            for (row, axis) in vecs.iter_mut().enumerate() {
                for (col, v) in axis.iter_mut().enumerate() {
                    *v = try_read_f32(r, (row * 4 + col) * 4)?;
                }
            }
            let name = &r[40..72];
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            Ok(DTexinfo {
                vecs,
                flags: try_read_i32(r, 32)?,
                value: try_read_i32(r, 36)?,
                texture: name[..end].iter().map(|&b| char::from(b)).collect(),
                next_texinfo: try_read_i32(r, 72)?,
            })
        })
    }

    pub fn models(&self, data: &[u8]) -> Q2Result<Vec<DModel>> {
        self.records(data, LUMP_MODELS, DMODEL_SIZE, "models", |r| {
            Ok(DModel {
                mins: read_vec3(r, 0)?,
                maxs: read_vec3(r, 12)?,
                origin: read_vec3(r, 24)?,
                head_node: try_read_i32(r, 36)?,
                first_face: try_read_i32(r, 40)?,
                num_faces: try_read_i32(r, 44)?,
            })
        })
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
    use q2_shared::Vec3f;

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

    /// Valid header whose only non-empty lump is `idx`, holding `bytes`.
    fn with_lump(idx: usize, bytes: &[u8]) -> Vec<u8> {
        let mut buf = header_with(b"");
        set_lump(&mut buf, idx, HEADER_SIZE as u32, bytes.len() as u32);
        buf.extend_from_slice(bytes);
        buf
    }

    fn le(words: &[[u8; 4]]) -> Vec<u8> {
        words.concat()
    }

    #[test]
    fn planes_decode_dplane_t_records() {
        let rec = le(&[
            0.0f32.to_le_bytes(),
            1.0f32.to_le_bytes(),
            0.0f32.to_le_bytes(),
            64.0f32.to_le_bytes(),
            1i32.to_le_bytes(),
        ]);
        let buf = with_lump(LUMP_PLANES, &rec);
        let planes = BspHeader::parse(&buf).unwrap().planes(&buf).unwrap();
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].normal, Vec3f::new(0.0, 1.0, 0.0));
        assert_eq!(planes[0].dist, 64.0);
        assert_eq!(planes[0].plane_type, 1);
    }

    #[test]
    fn nodes_and_leafs_decode_all_fields() {
        let mut node = le(&[
            3i32.to_le_bytes(),
            1i32.to_le_bytes(),
            (-2i32).to_le_bytes(),
        ]);
        for v in [-1i16, -2, -3, 4, 5, 6] {
            node.extend_from_slice(&v.to_le_bytes());
        }
        node.extend_from_slice(&7u16.to_le_bytes());
        node.extend_from_slice(&8u16.to_le_bytes());
        let buf = with_lump(LUMP_NODES, &node);
        let nodes = BspHeader::parse(&buf).unwrap().nodes(&buf).unwrap();
        assert_eq!(nodes[0].plane_num, 3);
        assert_eq!(nodes[0].children, [1, -2]);
        assert_eq!((nodes[0].mins, nodes[0].maxs), ([-1, -2, -3], [4, 5, 6]));
        assert_eq!((nodes[0].first_face, nodes[0].num_faces), (7, 8));

        let mut leaf = 1i32.to_le_bytes().to_vec();
        for v in [9i16, 2, -1, -2, -3, 4, 5, 6] {
            leaf.extend_from_slice(&v.to_le_bytes());
        }
        for v in [10u16, 11, 12, 13] {
            leaf.extend_from_slice(&v.to_le_bytes());
        }
        let buf = with_lump(LUMP_LEAFS, &leaf);
        let leafs = BspHeader::parse(&buf).unwrap().leafs(&buf).unwrap();
        assert_eq!(
            (leafs[0].contents, leafs[0].cluster, leafs[0].area),
            (1, 9, 2)
        );
        assert_eq!((leafs[0].mins, leafs[0].maxs), ([-1, -2, -3], [4, 5, 6]));
        assert_eq!(
            (leafs[0].first_leaf_face, leafs[0].num_leaf_faces),
            (10, 11)
        );
        assert_eq!(
            (leafs[0].first_leaf_brush, leafs[0].num_leaf_brushes),
            (12, 13)
        );
    }

    #[test]
    fn texinfo_and_models_decode_all_fields() {
        let mut tex = Vec::new();
        for f in 1..=8 {
            tex.extend_from_slice(&(f as f32).to_le_bytes());
        }
        tex.extend_from_slice(&0x10i32.to_le_bytes());
        tex.extend_from_slice(&300i32.to_le_bytes());
        let mut name = [0u8; 32];
        name[..13].copy_from_slice(b"e1u1/floor1_1");
        tex.extend_from_slice(&name);
        tex.extend_from_slice(&(-1i32).to_le_bytes());
        let buf = with_lump(LUMP_TEXINFO, &tex);
        let infos = BspHeader::parse(&buf).unwrap().texinfo(&buf).unwrap();
        assert_eq!(infos[0].vecs, [[1.0, 2.0, 3.0, 4.0], [5.0, 6.0, 7.0, 8.0]]);
        assert_eq!((infos[0].flags, infos[0].value), (0x10, 300));
        assert_eq!(infos[0].texture, "e1u1/floor1_1");
        assert_eq!(infos[0].next_texinfo, -1);

        let mut model = Vec::new();
        for f in [0.0f32, 1.0, 2.0, 10.0, 11.0, 12.0, 5.0, 6.0, 7.0] {
            model.extend_from_slice(&f.to_le_bytes());
        }
        for i in [4i32, 20, 30] {
            model.extend_from_slice(&i.to_le_bytes());
        }
        let buf = with_lump(LUMP_MODELS, &model);
        let models = BspHeader::parse(&buf).unwrap().models(&buf).unwrap();
        // Raw on-disk bounds: the one-unit spread is each loader's business.
        assert_eq!(models[0].mins, Vec3f::new(0.0, 1.0, 2.0));
        assert_eq!(models[0].maxs, Vec3f::new(10.0, 11.0, 12.0));
        assert_eq!(models[0].origin, Vec3f::new(5.0, 6.0, 7.0));
        assert_eq!(
            (
                models[0].head_node,
                models[0].first_face,
                models[0].num_faces
            ),
            (4, 20, 30)
        );
    }

    #[test]
    fn record_lumps_reject_lengths_that_are_not_whole_records() {
        // C Mod_Load*: "funny lump size" is ERR_DROP.
        for idx in [
            LUMP_PLANES,
            LUMP_NODES,
            LUMP_LEAFS,
            LUMP_TEXINFO,
            LUMP_MODELS,
        ] {
            let buf = with_lump(idx, &[0u8; 7]);
            let header = BspHeader::parse(&buf).unwrap();
            let failed = match idx {
                LUMP_PLANES => header.planes(&buf).is_err(),
                LUMP_NODES => header.nodes(&buf).is_err(),
                LUMP_LEAFS => header.leafs(&buf).is_err(),
                LUMP_TEXINFO => header.texinfo(&buf).is_err(),
                _ => header.models(&buf).is_err(),
            };
            assert!(failed, "lump {idx} accepted 7 bytes");
        }
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
    fn texinfo_name_keeps_bytes_and_full_length() {
        // Bytes map 1:1 to chars like entity_string; a name filling all 32
        // bytes has no NUL and is kept whole for the renderer.
        let mut tex = vec![0u8; 40];
        tex.extend_from_slice(b"e1u1/caf\xe9_long_texture_name_xyz!");
        tex.extend_from_slice(&(-1i32).to_le_bytes());
        let buf = with_lump(LUMP_TEXINFO, &tex);
        let infos = BspHeader::parse(&buf).unwrap().texinfo(&buf).unwrap();
        assert_eq!(infos[0].texture, "e1u1/caf\u{e9}_long_texture_name_xyz!");
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
