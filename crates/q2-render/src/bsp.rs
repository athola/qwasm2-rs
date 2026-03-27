//! BSP (Binary Space Partitioning) file loader for Quake 2 `.bsp` maps.
//!
//! Parses the IBSP v38 format used by Quake 2 into typed Rust structs.
//! All on-disk values are little-endian.
//!
//! Reference: `common/header/files.h` in the Quake 2 source (struct definitions),
//! and `gl3/gl3_model.c` / `files/models.c` for the loading logic.

use q2_common::error::{Q2Error, Q2Result};
use q2_shared::types::Vec3f;

// ---------------------------------------------------------------------------
// BSP constants
// ---------------------------------------------------------------------------

/// Magic number: little-endian "IBSP".
pub const BSP_MAGIC: u32 = 0x5053_4249;

/// Quake 2 BSP version.
pub const BSP_VERSION: u32 = 38;

/// Number of lumps in the BSP header.
pub const NUM_LUMPS: usize = 19;

// Lump indices
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

// On-disk struct sizes (bytes). Used for lump validation.
const DISK_VERTEX_SIZE: usize = 12; // 3 * f32
const DISK_EDGE_SIZE: usize = 4; // 2 * u16
const DISK_FACE_SIZE: usize = 20; // u16 + i16 + i32 + i16 + i16 + 4*u8 + i32
const DISK_PLANE_SIZE: usize = 20; // 3*f32 + f32 + i32
const DISK_NODE_SIZE: usize = 28; // i32 + 2*i32 + 6*i16 + u16 + u16
const DISK_TEXINFO_SIZE: usize = 76; // 2*4*f32 + i32 + i32 + 32 + i32
const DISK_LEAF_SIZE: usize = 28; // i32 + i16 + i16 + 6*i16 + 4*u16
const DISK_MODEL_SIZE: usize = 48; // 9*f32 + i32 + i32 + i32

// ---------------------------------------------------------------------------
// Parsed BSP types
// ---------------------------------------------------------------------------

/// A lump directory entry (offset + length).
#[derive(Debug, Clone)]
pub struct BspLump {
    pub offset: u32,
    pub length: u32,
}

/// A vertex (3D position).
#[derive(Debug, Clone)]
pub struct BspVertex {
    pub position: Vec3f,
}

/// An edge — two vertex indices.
#[derive(Debug, Clone)]
pub struct BspEdge {
    pub v: [u16; 2],
}

/// Texture-mapping info for a face.
///
/// `vecs` encodes the texture projection: for each axis (s, t) there are four
/// floats `[x, y, z, offset]` such that `s = dot(position, xyz) + offset`.
#[derive(Debug, Clone)]
pub struct BspTexInfo {
    /// `[s/t][xyz offset]` — texture projection vectors.
    pub vecs: [[f32; 4]; 2],
    /// Surface flags (`SURF_*`).
    pub flags: i32,
    /// Light emission value.
    pub value: i32,
    /// Texture name (e.g. `"e1u1/floor1_1"`), up to 32 chars.
    pub texture: String,
    /// Next texinfo in animation chain, or -1 for none.
    pub next_texinfo: i32,
}

/// A face (polygon on a BSP plane).
#[derive(Debug, Clone)]
pub struct BspFace {
    pub plane_idx: u16,
    /// 0 = front side, 1 = back side of the plane.
    pub side: u16,
    /// Index into the surface-edges array.
    pub first_edge: i32,
    /// Number of edges (and thus vertices) for this face.
    pub num_edges: i16,
    /// Index into the texinfo array.
    pub texinfo_idx: i16,
    /// Lightmap styles (up to 4).
    pub styles: [u8; 4],
    /// Byte offset into lightmap data, or -1 for no lightmap.
    pub light_ofs: i32,
}

/// A BSP plane (splitting plane of the tree).
#[derive(Debug, Clone)]
pub struct BspPlane {
    pub normal: Vec3f,
    pub dist: f32,
    /// Plane type: 0-2 = axial (X/Y/Z), 3-5 = nearest axial.
    pub plane_type: i32,
}

/// A BSP internal node.
#[derive(Debug, Clone)]
pub struct BspNode {
    pub plane_idx: i32,
    /// Children: positive = node index, negative = -(leaf+1).
    pub children: [i32; 2],
    pub mins: [i16; 3],
    pub maxs: [i16; 3],
    pub first_face: u16,
    pub num_faces: u16,
}

/// A BSP leaf (terminal node).
#[derive(Debug, Clone)]
pub struct BspLeaf {
    /// OR of all brush contents in this leaf.
    pub contents: i32,
    /// Visibility cluster, or -1.
    pub cluster: i16,
    /// Area index.
    pub area: i16,
    pub first_leaf_face: u16,
    pub num_leaf_faces: u16,
    pub first_leaf_brush: u16,
    pub num_leaf_brushes: u16,
}

/// A submodel (the world model is index 0, doors/platforms are 1+).
#[derive(Debug, Clone)]
pub struct BspModel {
    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub origin: Vec3f,
    pub head_node: i32,
    pub first_face: i32,
    pub num_faces: i32,
}

/// Complete parsed BSP data for rendering.
#[derive(Debug)]
pub struct BspData {
    pub entities: String,
    pub planes: Vec<BspPlane>,
    pub vertices: Vec<BspVertex>,
    pub visibility: Vec<u8>,
    pub nodes: Vec<BspNode>,
    pub texinfo: Vec<BspTexInfo>,
    pub faces: Vec<BspFace>,
    pub lightmap_data: Vec<u8>,
    pub leafs: Vec<BspLeaf>,
    pub leaf_faces: Vec<u16>,
    pub edges: Vec<BspEdge>,
    pub surface_edges: Vec<i32>,
    pub models: Vec<BspModel>,
}

use q2_common::binary::{try_read_f32, try_read_i16, try_read_i32, try_read_u16, try_read_u32};

/// Read a `Vec3f` (3 consecutive LE f32s), returning an error on out-of-bounds.
fn try_read_vec3(data: &[u8], off: usize) -> Q2Result<Vec3f> {
    Ok(Vec3f::new(
        try_read_f32(data, off)?,
        try_read_f32(data, off + 4)?,
        try_read_f32(data, off + 8)?,
    ))
}

/// Read a null-terminated string of up to `max_len` bytes.
fn read_cstring(data: &[u8], off: usize, max_len: usize) -> String {
    let slice = &data[off..off + max_len];
    let end = slice.iter().position(|&b| b == 0).unwrap_or(max_len);
    String::from_utf8_lossy(&slice[..end]).into_owned()
}

// ---------------------------------------------------------------------------
// Lump validation helper
// ---------------------------------------------------------------------------

/// Validate that a lump fits within the file and has a size that is a multiple
/// of the given struct size. Returns (offset, count).
fn validate_lump(
    data: &[u8],
    lump: &BspLump,
    struct_size: usize,
    name: &str,
) -> Q2Result<(usize, usize)> {
    let off = lump.offset as usize;
    let len = lump.length as usize;

    if off.checked_add(len).is_none() || off + len > data.len() {
        return Err(Q2Error::Drop(format!(
            "BSP {name} lump extends past end of file"
        )));
    }

    if struct_size > 0 && !len.is_multiple_of(struct_size) {
        return Err(Q2Error::Drop(format!(
            "BSP {name} lump has invalid size (not a multiple of {struct_size})"
        )));
    }

    let count = if struct_size > 0 { len / struct_size } else { 0 };
    Ok((off, count))
}

// ---------------------------------------------------------------------------
// Individual lump loaders
// ---------------------------------------------------------------------------

fn load_entities(data: &[u8], lump: &BspLump) -> Q2Result<String> {
    let off = lump.offset as usize;
    let len = lump.length as usize;
    if off + len > data.len() {
        return Err(Q2Error::Drop(
            "BSP entities lump extends past end of file".into(),
        ));
    }
    let raw = &data[off..off + len];
    // Strip trailing null bytes
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    Ok(String::from_utf8_lossy(&raw[..end]).into_owned())
}

fn load_planes(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspPlane>> {
    let (off, count) = validate_lump(data, lump, DISK_PLANE_SIZE, "planes")?;
    let mut planes = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_PLANE_SIZE;
        planes.push(BspPlane {
            normal: try_read_vec3(data, base)?,
            dist: try_read_f32(data, base + 12)?,
            plane_type: try_read_i32(data, base + 16)?,
        });
    }
    Ok(planes)
}

fn load_vertices(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspVertex>> {
    let (off, count) = validate_lump(data, lump, DISK_VERTEX_SIZE, "vertices")?;
    let mut verts = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_VERTEX_SIZE;
        verts.push(BspVertex {
            position: try_read_vec3(data, base)?,
        });
    }
    Ok(verts)
}

fn load_visibility(data: &[u8], lump: &BspLump) -> Q2Result<Vec<u8>> {
    let off = lump.offset as usize;
    let len = lump.length as usize;
    if off + len > data.len() {
        return Err(Q2Error::Drop(
            "BSP visibility lump extends past end of file".into(),
        ));
    }
    Ok(data[off..off + len].to_vec())
}

fn load_nodes(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspNode>> {
    let (off, count) = validate_lump(data, lump, DISK_NODE_SIZE, "nodes")?;
    let mut nodes = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_NODE_SIZE;
        nodes.push(BspNode {
            plane_idx: try_read_i32(data, base)?,
            children: [try_read_i32(data, base + 4)?, try_read_i32(data, base + 8)?],
            mins: [
                try_read_i16(data, base + 12)?,
                try_read_i16(data, base + 14)?,
                try_read_i16(data, base + 16)?,
            ],
            maxs: [
                try_read_i16(data, base + 18)?,
                try_read_i16(data, base + 20)?,
                try_read_i16(data, base + 22)?,
            ],
            first_face: try_read_u16(data, base + 24)?,
            num_faces: try_read_u16(data, base + 26)?,
        });
    }
    Ok(nodes)
}

fn load_texinfo(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspTexInfo>> {
    let (off, count) = validate_lump(data, lump, DISK_TEXINFO_SIZE, "texinfo")?;
    let mut infos = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_TEXINFO_SIZE;
        let mut vecs = [[0.0f32; 4]; 2];
        for (row, vecs_row) in vecs.iter_mut().enumerate() {
            for (col, val) in vecs_row.iter_mut().enumerate() {
                *val = try_read_f32(data, base + (row * 4 + col) * 4)?;
            }
        }
        infos.push(BspTexInfo {
            vecs,
            flags: try_read_i32(data, base + 32)?,
            value: try_read_i32(data, base + 36)?,
            texture: read_cstring(data, base + 40, 32),
            next_texinfo: try_read_i32(data, base + 72)?,
        });
    }
    Ok(infos)
}

fn load_faces(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspFace>> {
    let (off, count) = validate_lump(data, lump, DISK_FACE_SIZE, "faces")?;
    let mut faces = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_FACE_SIZE;
        faces.push(BspFace {
            plane_idx: try_read_u16(data, base)?,
            side: try_read_u16(data, base + 2)?,
            first_edge: try_read_i32(data, base + 4)?,
            num_edges: try_read_i16(data, base + 8)?,
            texinfo_idx: try_read_i16(data, base + 10)?,
            styles: [
                data[base + 12],
                data[base + 13],
                data[base + 14],
                data[base + 15],
            ],
            light_ofs: try_read_i32(data, base + 16)?,
        });
    }
    Ok(faces)
}

fn load_lightmap(data: &[u8], lump: &BspLump) -> Q2Result<Vec<u8>> {
    let off = lump.offset as usize;
    let len = lump.length as usize;
    if off + len > data.len() {
        return Err(Q2Error::Drop(
            "BSP lightmap lump extends past end of file".into(),
        ));
    }
    Ok(data[off..off + len].to_vec())
}

fn load_leafs(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspLeaf>> {
    let (off, count) = validate_lump(data, lump, DISK_LEAF_SIZE, "leafs")?;
    let mut leafs = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_LEAF_SIZE;
        leafs.push(BspLeaf {
            contents: try_read_i32(data, base)?,
            cluster: try_read_i16(data, base + 4)?,
            area: try_read_i16(data, base + 6)?,
            // mins/maxs at base+8..base+20 are for frustum culling; we skip them
            // (they are not stored in BspLeaf — used at render time by the node tree)
            first_leaf_face: try_read_u16(data, base + 20)?,
            num_leaf_faces: try_read_u16(data, base + 22)?,
            first_leaf_brush: try_read_u16(data, base + 24)?,
            num_leaf_brushes: try_read_u16(data, base + 26)?,
        });
    }
    Ok(leafs)
}

fn load_leaf_faces(data: &[u8], lump: &BspLump) -> Q2Result<Vec<u16>> {
    let (off, count) = validate_lump(data, lump, 2, "leaf_faces")?;
    let mut lf = Vec::with_capacity(count);
    for i in 0..count {
        lf.push(try_read_u16(data, off + i * 2)?);
    }
    Ok(lf)
}

fn load_edges(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspEdge>> {
    let (off, count) = validate_lump(data, lump, DISK_EDGE_SIZE, "edges")?;
    let mut edges = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_EDGE_SIZE;
        edges.push(BspEdge {
            v: [try_read_u16(data, base)?, try_read_u16(data, base + 2)?],
        });
    }
    Ok(edges)
}

fn load_surface_edges(data: &[u8], lump: &BspLump) -> Q2Result<Vec<i32>> {
    let (off, count) = validate_lump(data, lump, 4, "surface_edges")?;
    let mut se = Vec::with_capacity(count);
    for i in 0..count {
        se.push(try_read_i32(data, off + i * 4)?);
    }
    Ok(se)
}

fn load_models(data: &[u8], lump: &BspLump) -> Q2Result<Vec<BspModel>> {
    let (off, count) = validate_lump(data, lump, DISK_MODEL_SIZE, "models")?;
    let mut models = Vec::with_capacity(count);
    for i in 0..count {
        let base = off + i * DISK_MODEL_SIZE;
        models.push(BspModel {
            mins: try_read_vec3(data, base)?,
            maxs: try_read_vec3(data, base + 12)?,
            origin: try_read_vec3(data, base + 24)?,
            head_node: try_read_i32(data, base + 36)?,
            first_face: try_read_i32(data, base + 40)?,
            num_faces: try_read_i32(data, base + 44)?,
        });
    }
    Ok(models)
}

// ---------------------------------------------------------------------------
// Main loader
// ---------------------------------------------------------------------------

impl BspData {
    /// Parse BSP data from raw bytes (the contents of a `.bsp` file).
    ///
    /// Validates the header magic and version, then reads each lump into
    /// typed Rust structures. All values are assumed little-endian (x86/wasm).
    pub fn load(data: &[u8]) -> Q2Result<Self> {
        // Minimum size: 4 (magic) + 4 (version) + 19 * 8 (lump directory) = 160 bytes
        const HEADER_SIZE: usize = 4 + 4 + NUM_LUMPS * 8;
        if data.len() < HEADER_SIZE {
            return Err(Q2Error::Drop("BSP file too small for header".into()));
        }

        // Validate magic
        let magic = try_read_u32(data, 0)?;
        if magic != BSP_MAGIC {
            return Err(Q2Error::Drop(format!(
                "BSP bad magic: expected 0x{BSP_MAGIC:08X}, got 0x{magic:08X}"
            )));
        }

        // Validate version
        let version = try_read_u32(data, 4)?;
        if version != BSP_VERSION {
            return Err(Q2Error::Drop(format!(
                "BSP bad version: expected {BSP_VERSION}, got {version}"
            )));
        }

        // Read lump directory
        let mut lumps = Vec::with_capacity(NUM_LUMPS);
        for i in 0..NUM_LUMPS {
            let base = 8 + i * 8;
            lumps.push(BspLump {
                offset: try_read_u32(data, base)?,
                length: try_read_u32(data, base + 4)?,
            });
        }

        // Load each lump
        let entities = load_entities(data, &lumps[LUMP_ENTITIES])?;
        let planes = load_planes(data, &lumps[LUMP_PLANES])?;
        let vertices = load_vertices(data, &lumps[LUMP_VERTEXES])?;
        let visibility = load_visibility(data, &lumps[LUMP_VISIBILITY])?;
        let nodes = load_nodes(data, &lumps[LUMP_NODES])?;
        let texinfo = load_texinfo(data, &lumps[LUMP_TEXINFO])?;
        let faces = load_faces(data, &lumps[LUMP_FACES])?;
        let lightmap_data = load_lightmap(data, &lumps[LUMP_LIGHTING])?;
        let leafs = load_leafs(data, &lumps[LUMP_LEAFS])?;
        let leaf_faces = load_leaf_faces(data, &lumps[LUMP_LEAFFACES])?;
        let edges = load_edges(data, &lumps[LUMP_EDGES])?;
        let surface_edges = load_surface_edges(data, &lumps[LUMP_SURFEDGES])?;
        let models = load_models(data, &lumps[LUMP_MODELS])?;

        Ok(BspData {
            entities,
            planes,
            vertices,
            visibility,
            nodes,
            texinfo,
            faces,
            lightmap_data,
            leafs,
            leaf_faces,
            edges,
            surface_edges,
            models,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid BSP byte buffer with header + empty lumps.
    fn make_minimal_bsp() -> Vec<u8> {
        let header_size: u32 = (4 + 4 + NUM_LUMPS * 8) as u32;
        let mut buf = Vec::new();

        // Magic
        buf.extend_from_slice(&BSP_MAGIC.to_le_bytes());
        // Version
        buf.extend_from_slice(&BSP_VERSION.to_le_bytes());

        // 19 lumps, all pointing to offset=header_size, length=0
        for _ in 0..NUM_LUMPS {
            buf.extend_from_slice(&header_size.to_le_bytes()); // offset
            buf.extend_from_slice(&0u32.to_le_bytes()); // length
        }

        buf
    }

    #[test]
    fn bsp_header_parse() {
        let data = make_minimal_bsp();
        let bsp = BspData::load(&data).expect("should parse minimal BSP");
        assert!(bsp.vertices.is_empty());
        assert!(bsp.faces.is_empty());
        assert!(bsp.planes.is_empty());
        assert!(bsp.nodes.is_empty());
        assert!(bsp.leafs.is_empty());
        assert!(bsp.edges.is_empty());
        assert!(bsp.models.is_empty());
        assert!(bsp.texinfo.is_empty());
        assert!(bsp.entities.is_empty());
    }

    #[test]
    fn bsp_reject_invalid() {
        // Too short
        assert!(BspData::load(&[0u8; 10]).is_err());

        // Wrong magic
        let mut bad_magic = make_minimal_bsp();
        bad_magic[0] = 0xFF;
        assert!(BspData::load(&bad_magic).is_err());

        // Wrong version
        let mut bad_version = make_minimal_bsp();
        bad_version[4] = 99;
        bad_version[5] = 0;
        bad_version[6] = 0;
        bad_version[7] = 0;
        assert!(BspData::load(&bad_version).is_err());
    }

    #[test]
    fn bsp_parse_vertices() {
        let header_size = (4 + 4 + NUM_LUMPS * 8) as u32;
        let mut buf = Vec::new();

        // Magic + version
        buf.extend_from_slice(&BSP_MAGIC.to_le_bytes());
        buf.extend_from_slice(&BSP_VERSION.to_le_bytes());

        // Vertex data will live right after header.
        // We place 2 vertices (24 bytes) for the vertex lump and leave everything else empty.
        let vert_offset = header_size;
        let vert_length: u32 = 2 * DISK_VERTEX_SIZE as u32;

        for i in 0..NUM_LUMPS {
            if i == LUMP_VERTEXES {
                buf.extend_from_slice(&vert_offset.to_le_bytes());
                buf.extend_from_slice(&vert_length.to_le_bytes());
            } else {
                // Point to after vert data, length 0
                buf.extend_from_slice(&(vert_offset + vert_length).to_le_bytes());
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }

        // Write 2 vertices
        // Vertex 0: (1.0, 2.0, 3.0)
        buf.extend_from_slice(&1.0f32.to_le_bytes());
        buf.extend_from_slice(&2.0f32.to_le_bytes());
        buf.extend_from_slice(&3.0f32.to_le_bytes());
        // Vertex 1: (4.0, 5.0, 6.0)
        buf.extend_from_slice(&4.0f32.to_le_bytes());
        buf.extend_from_slice(&5.0f32.to_le_bytes());
        buf.extend_from_slice(&6.0f32.to_le_bytes());

        let bsp = BspData::load(&buf).expect("should parse BSP with vertices");
        assert_eq!(bsp.vertices.len(), 2);
        assert_eq!(bsp.vertices[0].position, Vec3f::new(1.0, 2.0, 3.0));
        assert_eq!(bsp.vertices[1].position, Vec3f::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn bsp_reject_misaligned_lump() {
        let header_size = (4 + 4 + NUM_LUMPS * 8) as u32;
        let mut buf = Vec::new();

        buf.extend_from_slice(&BSP_MAGIC.to_le_bytes());
        buf.extend_from_slice(&BSP_VERSION.to_le_bytes());

        // Vertex lump with length that is not a multiple of 12
        for i in 0..NUM_LUMPS {
            if i == LUMP_VERTEXES {
                buf.extend_from_slice(&header_size.to_le_bytes());
                buf.extend_from_slice(&7u32.to_le_bytes()); // 7 is not a multiple of 12
            } else {
                buf.extend_from_slice(&header_size.to_le_bytes());
                buf.extend_from_slice(&0u32.to_le_bytes());
            }
        }

        // Pad some data so the offset is valid
        buf.extend_from_slice(&[0u8; 12]);

        assert!(BspData::load(&buf).is_err());
    }
}
