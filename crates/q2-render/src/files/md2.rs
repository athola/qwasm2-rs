//! MD2 alias-model loader (C `Mod_LoadAliasModel` in `refresh/files/models.c`).
//!
//! The GL command list (`ofs_glcmds`) is not parsed: the renderer draws from
//! the triangle list, so only skins, texture coordinates, triangles and frames
//! are kept.

pub use super::anorms::VERTEX_NORMALS;
use super::{fixed_cstring, malformed, non_negative, slice_at};
use q2_common::binary::{try_read_f32, try_read_i16, try_read_i32, try_read_u16};
use q2_common::error::Q2Result;
use q2_shared::types::Vec3f;

const WHAT: &str = "md2";
/// Little-endian "IDP2".
pub const MD2_IDENT: i32 = 0x3250_4449;
pub const MD2_VERSION: i32 = 8;

pub const MAX_TRIANGLES: usize = 4096;
pub const MAX_VERTS: usize = 2048;
pub const MAX_FRAMES: usize = 512;
pub const MAX_SKINS: usize = 32;
pub const MAX_SKINNAME: usize = 64;
/// C `MAX_LBM_HEIGHT`.
const MAX_SKIN_HEIGHT: i32 = 480;

const HEADER_SIZE: usize = 68; // 17 ints
const ST_SIZE: usize = 4;
const TRIANGLE_SIZE: usize = 12;
const FRAME_HEADER_SIZE: usize = 40; // scale + translate + name[16]
const FRAME_VERTEX_SIZE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Md2Triangle {
    /// Indices into each frame's `verts`.
    pub vertex: [u16; 3],
    /// Indices into `Md2Model::st`.
    pub st: [u16; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Md2Vertex {
    /// Quantised position, expanded by the frame's scale and translate.
    pub v: [u8; 3],
    /// Index into [`VERTEX_NORMALS`].
    pub normal_index: u8,
}

#[derive(Debug, Clone)]
pub struct Md2Frame {
    pub name: String,
    pub scale: [f32; 3],
    pub translate: [f32; 3],
    pub verts: Vec<Md2Vertex>,
}

impl Md2Frame {
    /// World-space position of vertex `index`: `v * scale + translate`.
    ///
    /// Panics if `index >= verts.len()`; `load_md2` guarantees triangle
    /// indices are in range.
    pub fn vertex_position(&self, index: usize) -> Vec3f {
        let [x, y, z] = self.verts[index].v;
        Vec3f::new(
            f32::from(x) * self.scale[0] + self.translate[0],
            f32::from(y) * self.scale[1] + self.translate[1],
            f32::from(z) * self.scale[2] + self.translate[2],
        )
    }
}

#[derive(Debug, Clone)]
pub struct Md2Model {
    pub skin_width: i32,
    pub skin_height: i32,
    pub skins: Vec<String>,
    /// Texture coordinates in skin pixels.
    pub st: Vec<[i16; 2]>,
    pub triangles: Vec<Md2Triangle>,
    pub frames: Vec<Md2Frame>,
}

pub fn load_md2(data: &[u8]) -> Q2Result<Md2Model> {
    let header = slice_at(WHAT, data, 0, HEADER_SIZE)?;
    let field = |index: usize| try_read_i32(header, index * 4);

    if field(0)? != MD2_IDENT {
        return Err(malformed(WHAT, "bad ident"));
    }
    let version = field(1)?;
    if version != MD2_VERSION {
        return Err(malformed(
            WHAT,
            format_args!("version {version}, expected {MD2_VERSION}"),
        ));
    }
    let skin_width = field(2)?;
    let skin_height = field(3)?;
    let frame_size = non_negative(WHAT, "framesize", field(4)?)?;
    let num_skins = non_negative(WHAT, "num_skins", field(5)?)?;
    let num_xyz = non_negative(WHAT, "num_xyz", field(6)?)?;
    let num_st = non_negative(WHAT, "num_st", field(7)?)?;
    let num_tris = non_negative(WHAT, "num_tris", field(8)?)?;
    // field(9) is num_glcmds, unused.
    let num_frames = non_negative(WHAT, "num_frames", field(10)?)?;
    let ofs_skins = non_negative(WHAT, "ofs_skins", field(11)?)?;
    let ofs_st = non_negative(WHAT, "ofs_st", field(12)?)?;
    let ofs_tris = non_negative(WHAT, "ofs_tris", field(13)?)?;
    let ofs_frames = non_negative(WHAT, "ofs_frames", field(14)?)?;
    // field(15) is ofs_glcmds, unused.
    let ofs_end = non_negative(WHAT, "ofs_end", field(16)?)?;

    if ofs_end > data.len() {
        return Err(malformed(
            WHAT,
            format_args!("file size {} smaller than ofs_end {ofs_end}", data.len()),
        ));
    }
    if skin_height > MAX_SKIN_HEIGHT {
        return Err(malformed(WHAT, "skin taller than 480"));
    }
    if num_xyz == 0 || num_xyz > MAX_VERTS {
        return Err(malformed(WHAT, format_args!("bad vertex count {num_xyz}")));
    }
    if num_st == 0 {
        return Err(malformed(WHAT, "no st vertices"));
    }
    if num_tris == 0 || num_tris > MAX_TRIANGLES {
        return Err(malformed(
            WHAT,
            format_args!("bad triangle count {num_tris}"),
        ));
    }
    if num_frames == 0 || num_frames > MAX_FRAMES {
        return Err(malformed(
            WHAT,
            format_args!("bad frame count {num_frames}"),
        ));
    }
    if frame_size < FRAME_HEADER_SIZE + num_xyz * FRAME_VERTEX_SIZE {
        return Err(malformed(WHAT, "framesize too small for num_xyz"));
    }
    // C clamps and ignores extra skins.
    let num_skins = num_skins.min(MAX_SKINS);

    let mut skins = Vec::with_capacity(num_skins);
    for index in 0..num_skins {
        let raw = slice_at(WHAT, data, ofs_skins + index * MAX_SKINNAME, MAX_SKINNAME)?;
        skins.push(fixed_cstring(raw));
    }

    // Checked so a huge `num_st` cannot wrap a 32-bit `usize` (wasm32) to a
    // short slice before `with_capacity` trusts it.
    let st_len = num_st
        .checked_mul(ST_SIZE)
        .ok_or_else(|| malformed(WHAT, "num_st overflow"))?;
    let st_bytes = slice_at(WHAT, data, ofs_st, st_len)?;
    let mut st = Vec::with_capacity(num_st);
    for index in 0..num_st {
        st.push([
            try_read_i16(st_bytes, index * ST_SIZE)?,
            try_read_i16(st_bytes, index * ST_SIZE + 2)?,
        ]);
    }

    let tri_bytes = slice_at(WHAT, data, ofs_tris, num_tris * TRIANGLE_SIZE)?;
    let mut triangles = Vec::with_capacity(num_tris);
    for index in 0..num_tris {
        let base = index * TRIANGLE_SIZE;
        let mut triangle = Md2Triangle {
            vertex: [0; 3],
            st: [0; 3],
        };
        for corner in 0..3 {
            let vertex = try_read_u16(tri_bytes, base + corner * 2)?;
            let tex = try_read_u16(tri_bytes, base + 6 + corner * 2)?;
            if usize::from(vertex) >= num_xyz {
                return Err(malformed(
                    WHAT,
                    format_args!("triangle {index} vertex index {vertex} >= {num_xyz}"),
                ));
            }
            if usize::from(tex) >= num_st {
                return Err(malformed(
                    WHAT,
                    format_args!("triangle {index} st index {tex} >= {num_st}"),
                ));
            }
            triangle.vertex[corner] = vertex;
            triangle.st[corner] = tex;
        }
        triangles.push(triangle);
    }

    let mut frames = Vec::with_capacity(num_frames);
    for index in 0..num_frames {
        let start = index
            .checked_mul(frame_size)
            .and_then(|offset| ofs_frames.checked_add(offset))
            .ok_or_else(|| malformed(WHAT, "frame offset overflow"))?;
        let raw = slice_at(WHAT, data, start, frame_size)?;
        let vec3 = |off: usize| -> Q2Result<[f32; 3]> {
            Ok([
                try_read_f32(raw, off)?,
                try_read_f32(raw, off + 4)?,
                try_read_f32(raw, off + 8)?,
            ])
        };
        let verts = raw[FRAME_HEADER_SIZE..FRAME_HEADER_SIZE + num_xyz * FRAME_VERTEX_SIZE]
            .chunks_exact(FRAME_VERTEX_SIZE)
            .map(|quad| Md2Vertex {
                v: [quad[0], quad[1], quad[2]],
                normal_index: quad[3],
            })
            .collect();
        frames.push(Md2Frame {
            name: fixed_cstring(&raw[24..40]),
            scale: vec3(0)?,
            translate: vec3(12)?,
            verts,
        });
    }

    Ok(Md2Model {
        skin_width,
        skin_height,
        skins,
        st,
        triangles,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Knobs for building a synthetic MD2: 3 vertices, 3 st, 1 triangle, 2 frames.
    struct Spec {
        ident: [u8; 4],
        version: i32,
        num_skins: i32,
        tri_vertex: [u16; 3],
        tri_st: [u16; 3],
    }

    impl Default for Spec {
        fn default() -> Self {
            Spec {
                ident: *b"IDP2",
                version: 8,
                num_skins: 1,
                tri_vertex: [0, 1, 2],
                tri_st: [0, 1, 2],
            }
        }
    }

    const NUM_XYZ: usize = 3;
    const FRAME_SIZE: usize = FRAME_HEADER_SIZE + NUM_XYZ * FRAME_VERTEX_SIZE;
    const OFS_SKINS: usize = HEADER_SIZE;

    fn build(spec: &Spec) -> Vec<u8> {
        let ofs_st = OFS_SKINS + usize::try_from(spec.num_skins.max(0)).unwrap() * MAX_SKINNAME;
        let ofs_tris = ofs_st + 3 * ST_SIZE;
        let ofs_frames = ofs_tris + TRIANGLE_SIZE;
        let ofs_end = ofs_frames + 2 * FRAME_SIZE;
        let ints: [i32; 15] = [
            spec.version,
            64,
            32,
            i32::try_from(FRAME_SIZE).unwrap(),
            spec.num_skins,
            3,
            3,
            1,
            0,
            2,
            i32::try_from(OFS_SKINS).unwrap(),
            i32::try_from(ofs_st).unwrap(),
            i32::try_from(ofs_tris).unwrap(),
            i32::try_from(ofs_frames).unwrap(),
            i32::try_from(ofs_frames).unwrap(), // ofs_glcmds (unused)
        ];
        let mut bytes = spec.ident.to_vec();
        for value in ints {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&i32::try_from(ofs_end).unwrap().to_le_bytes());
        for index in 0..spec.num_skins {
            let mut name = [0u8; MAX_SKINNAME];
            let text = format!("skin{index}.pcx");
            name[..text.len()].copy_from_slice(text.as_bytes());
            bytes.extend_from_slice(&name);
        }
        for (s, t) in [(1i16, 2i16), (3, 4), (-5, 6)] {
            bytes.extend_from_slice(&s.to_le_bytes());
            bytes.extend_from_slice(&t.to_le_bytes());
        }
        for index in spec.tri_vertex.iter().chain(spec.tri_st.iter()) {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        for frame in 0..2u8 {
            for value in [0.5f32, 2.0, 1.0, 10.0, 20.0, 30.0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            let mut name = [0u8; 16];
            name[..6].copy_from_slice(b"stand0");
            name[5] = b'0' + frame;
            bytes.extend_from_slice(&name);
            for vertex in 0..3u8 {
                bytes.extend_from_slice(&[vertex * 2, vertex * 4, vertex * 8, vertex + 100]);
            }
        }
        assert_eq!(bytes.len(), ofs_end);
        bytes
    }

    fn set_i32(bytes: &mut [u8], field: usize, value: i32) {
        bytes[field * 4..field * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn parses_valid_model() {
        let model = load_md2(&build(&Spec::default())).unwrap();
        assert_eq!((model.skin_width, model.skin_height), (64, 32));
        assert_eq!(model.skins, vec!["skin0.pcx".to_string()]);
        assert_eq!(model.st, vec![[1, 2], [3, 4], [-5, 6]]);
        assert_eq!(
            model.triangles,
            vec![Md2Triangle {
                vertex: [0, 1, 2],
                st: [0, 1, 2]
            }]
        );
        assert_eq!(model.frames.len(), 2);
        assert_eq!(model.frames[1].name, "stand1");
        assert_eq!(model.frames[0].scale, [0.5, 2.0, 1.0]);
        assert_eq!(
            model.frames[0].verts[2],
            Md2Vertex {
                v: [4, 8, 16],
                normal_index: 102
            }
        );
    }

    #[test]
    fn vertex_position_applies_scale_then_translate() {
        let model = load_md2(&build(&Spec::default())).unwrap();
        let position = model.frames[0].vertex_position(2);
        assert_eq!(
            position,
            Vec3f::new(4.0 * 0.5 + 10.0, 8.0 * 2.0 + 20.0, 16.0 + 30.0)
        );
    }

    #[test]
    fn rejects_bad_ident_and_version() {
        let bad_ident = Spec {
            ident: *b"IDS2",
            ..Spec::default()
        };
        assert!(load_md2(&build(&bad_ident)).is_err());
        let bad_version = Spec {
            version: 7,
            ..Spec::default()
        };
        assert!(load_md2(&build(&bad_version)).is_err());
    }

    #[test]
    fn rejects_truncated_data() {
        let bytes = build(&Spec::default());
        assert!(load_md2(&[]).is_err());
        assert!(load_md2(&bytes[..HEADER_SIZE - 1]).is_err());
        // ofs_end now exceeds the file length.
        assert!(load_md2(&bytes[..bytes.len() - 1]).is_err());
    }

    #[test]
    fn rejects_offsets_pointing_past_the_file() {
        // Keep ofs_end valid so the section bounds checks are what fire.
        for (field, bad) in [(11, 100_000), (12, 100_000), (13, 100_000), (14, 100_000)] {
            let mut bytes = build(&Spec::default());
            set_i32(&mut bytes, field, bad);
            assert!(load_md2(&bytes).is_err(), "field {field}");
        }
        let mut bytes = build(&Spec::default());
        set_i32(&mut bytes, 12, -4);
        assert!(load_md2(&bytes).is_err());
        let mut bytes = build(&Spec::default());
        set_i32(&mut bytes, 14, i32::MAX);
        assert!(load_md2(&bytes).is_err());
    }

    #[test]
    fn rejects_out_of_range_counts() {
        // field index -> bad value: skinheight, num_xyz x3, num_st, num_tris x2, num_frames x2
        let cases = [
            (3, 481),
            (6, 0),
            (6, i32::try_from(MAX_VERTS).unwrap() + 1),
            (6, -1),
            (7, 0),
            (8, 0),
            (8, i32::try_from(MAX_TRIANGLES).unwrap() + 1),
            (10, 0),
            (10, i32::try_from(MAX_FRAMES).unwrap() + 1),
        ];
        for (field, bad) in cases {
            let mut bytes = build(&Spec::default());
            set_i32(&mut bytes, field, bad);
            assert!(load_md2(&bytes).is_err(), "field {field} = {bad}");
        }
    }

    #[test]
    fn st_count_larger_than_the_file_is_rejected() {
        let mut bytes = build(&Spec::default());
        set_i32(&mut bytes, 7, 0x4000_0000);
        assert!(load_md2(&bytes).is_err());
    }

    #[test]
    fn rejects_frame_size_too_small_for_vertices() {
        let mut bytes = build(&Spec::default());
        set_i32(&mut bytes, 4, i32::try_from(FRAME_SIZE).unwrap() - 1);
        assert!(load_md2(&bytes).is_err());
    }

    #[test]
    fn rejects_triangle_indices_out_of_range() {
        let bad_vertex = Spec {
            tri_vertex: [0, 1, 3],
            ..Spec::default()
        };
        assert!(load_md2(&build(&bad_vertex)).is_err());
        let bad_st = Spec {
            tri_st: [0, 3, 2],
            ..Spec::default()
        };
        assert!(load_md2(&build(&bad_st)).is_err());
    }

    #[test]
    fn skins_beyond_limit_are_ignored() {
        let spec = Spec {
            num_skins: 40,
            ..Spec::default()
        };
        let model = load_md2(&build(&spec)).unwrap();
        assert_eq!(model.skins.len(), MAX_SKINS);
    }

    #[test]
    fn normal_table_matches_c_anorms() {
        assert_eq!(VERTEX_NORMALS.len(), 162);
        assert_eq!(VERTEX_NORMALS[0], [-0.525731, 0.0, 0.850651]);
        assert_eq!(VERTEX_NORMALS[161], [-0.688191, -0.587785, -0.425325]);
        for normal in VERTEX_NORMALS {
            let length =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            assert!((length - 1.0).abs() < 1e-3);
        }
    }
}
