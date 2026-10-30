//! BSP (Binary Space Partitioning) file loader for Quake 2 `.bsp` maps.
//!
//! Parses the IBSP v38 format used by Quake 2 into typed Rust structs.
//! All on-disk values are little-endian.
//!
//! Reference: `common/header/files.h` in the Quake 2 source (struct definitions),
//! and `gl3/gl3_model.c` / `files/models.c` for the loading logic.

use q2_common::bsp_file::{
    BspHeader, DEDGE_SIZE, DFACE_SIZE, DVERTEX_SIZE, LUMP_EDGES, LUMP_FACES, LUMP_LEAFFACES,
    LUMP_LIGHTING, LUMP_SURFEDGES, LUMP_VERTEXES, LUMP_VISIBILITY,
};
use q2_common::error::{Q2Error, Q2Result};
use q2_shared::types::Vec3f;

// ---------------------------------------------------------------------------
// Parsed BSP types
// ---------------------------------------------------------------------------

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

use q2_common::binary::{try_read_f32, try_read_i16, try_read_i32, try_read_u16};

/// Read a `Vec3f` (3 consecutive LE f32s), returning an error on out-of-bounds.
fn try_read_vec3(data: &[u8], off: usize) -> Q2Result<Vec3f> {
    Ok(Vec3f::new(
        try_read_f32(data, off)?,
        try_read_f32(data, off + 4)?,
        try_read_f32(data, off + 8)?,
    ))
}

// ---------------------------------------------------------------------------
// Individual lump loaders
// ---------------------------------------------------------------------------

fn load_planes(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspPlane>> {
    Ok(header
        .planes(data)?
        .into_iter()
        .map(|p| BspPlane {
            normal: p.normal,
            dist: p.dist,
            plane_type: p.plane_type,
        })
        .collect())
}

fn load_vertices(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspVertex>> {
    header.records(data, LUMP_VERTEXES, DVERTEX_SIZE, "vertices", |r| {
        Ok(BspVertex {
            position: try_read_vec3(r, 0)?,
        })
    })
}

fn load_nodes(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspNode>> {
    Ok(header
        .nodes(data)?
        .into_iter()
        .map(|n| BspNode {
            plane_idx: n.plane_num,
            children: n.children,
            mins: n.mins,
            maxs: n.maxs,
            first_face: n.first_face,
            num_faces: n.num_faces,
        })
        .collect())
}

fn load_texinfo(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspTexInfo>> {
    Ok(header
        .texinfo(data)?
        .into_iter()
        .map(|t| BspTexInfo {
            vecs: t.vecs,
            flags: t.flags,
            value: t.value,
            texture: t.texture,
            next_texinfo: t.next_texinfo,
        })
        .collect())
}

fn load_faces(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspFace>> {
    header.records(data, LUMP_FACES, DFACE_SIZE, "faces", |r| {
        Ok(BspFace {
            plane_idx: try_read_u16(r, 0)?,
            side: try_read_u16(r, 2)?,
            first_edge: try_read_i32(r, 4)?,
            num_edges: try_read_i16(r, 8)?,
            texinfo_idx: try_read_i16(r, 10)?,
            styles: [r[12], r[13], r[14], r[15]],
            light_ofs: try_read_i32(r, 16)?,
        })
    })
}

fn load_leafs(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspLeaf>> {
    Ok(header
        .leafs(data)?
        .into_iter()
        .map(|l| BspLeaf {
            contents: l.contents,
            cluster: l.cluster,
            area: l.area,
            first_leaf_face: l.first_leaf_face,
            num_leaf_faces: l.num_leaf_faces,
            first_leaf_brush: l.first_leaf_brush,
            num_leaf_brushes: l.num_leaf_brushes,
        })
        .collect())
}

fn load_leaf_faces(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<u16>> {
    header.records(data, LUMP_LEAFFACES, 2, "leaf_faces", |r| {
        try_read_u16(r, 0)
    })
}

fn load_edges(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspEdge>> {
    header.records(data, LUMP_EDGES, DEDGE_SIZE, "edges", |r| {
        Ok(BspEdge {
            v: [try_read_u16(r, 0)?, try_read_u16(r, 2)?],
        })
    })
}

fn load_surface_edges(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<i32>> {
    header.records(data, LUMP_SURFEDGES, 4, "surface_edges", |r| {
        try_read_i32(r, 0)
    })
}

fn load_models(header: &BspHeader, data: &[u8]) -> Q2Result<Vec<BspModel>> {
    let spread = Vec3f::new(1.0, 1.0, 1.0);
    Ok(header
        .models(data)?
        .into_iter()
        .map(|m| BspModel {
            // Spread the bounds by one unit like gl3_model.c Mod_LoadSubmodels.
            mins: m.mins - spread,
            maxs: m.maxs + spread,
            origin: m.origin,
            head_node: m.head_node,
            first_face: m.first_face,
            num_faces: m.num_faces,
        })
        .collect())
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
        let header = BspHeader::parse(data)?;

        // Load each lump
        let planes = load_planes(&header, data)?;
        let vertices = load_vertices(&header, data)?;
        let visibility = header.lump(data, LUMP_VISIBILITY).to_vec();
        let nodes = load_nodes(&header, data)?;
        let texinfo = load_texinfo(&header, data)?;
        let faces = load_faces(&header, data)?;
        let lightmap_data = header.lump(data, LUMP_LIGHTING).to_vec();
        let leafs = load_leafs(&header, data)?;
        let leaf_faces = load_leaf_faces(&header, data)?;
        let edges = load_edges(&header, data)?;
        let surface_edges = load_surface_edges(&header, data)?;
        let models = load_models(&header, data)?;

        let bsp = BspData {
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
        };
        bsp.validate_references()?;
        Ok(bsp)
    }

    /// The index checks C makes while loading, each an ERR_DROP with C's
    /// message: gl3_model.c `Mod_LoadFaces`, `Mod_LoadLeafs`,
    /// `Mod_LoadMarksurfaces` and `Mod_LoadSubmodels`, models.c
    /// `Mod_LoadNodes`. C checks no face's surfedge range nor a submodel's
    /// face range; the consumers skip those indices when they are out of
    /// range, where C would read past its arrays.
    fn validate_references(&self) -> Q2Result<()> {
        let drop = |msg: String| Err(Q2Error::Drop(msg));
        let in_range = |index: i64, len: usize| (0..len as i64).contains(&index);
        for face in &self.faces {
            // Unsigned here: C's LittleShort sign-extends, so it also
            // rejects planenum >= 32768 on maps with that many planes.
            if !in_range(i64::from(face.plane_idx), self.planes.len()) {
                return drop(format!(
                    "Mod_LoadFaces: Incorrect {} planenum.",
                    face.plane_idx
                ));
            }
            if !in_range(i64::from(face.texinfo_idx), self.texinfo.len()) {
                return drop("Mod_LoadFaces: bad texinfo number".into());
            }
        }
        for node in &self.nodes {
            if !in_range(i64::from(node.plane_idx), self.planes.len()) {
                return drop(format!(
                    "Mod_LoadNodes: Incorrect {} < {} planenum.",
                    node.plane_idx,
                    self.planes.len()
                ));
            }
            for &child in &node.children {
                if child >= 0 && !in_range(i64::from(child), self.nodes.len()) {
                    return drop(format!("Mod_LoadNodes: Incorrect {child} nodenum as leaf."));
                }
                let leaf = -1 - i64::from(child);
                if child < 0 && !in_range(leaf, self.leafs.len()) {
                    return drop(format!("Mod_LoadNodes: Incorrect {leaf} leafnum."));
                }
            }
        }
        for leaf in &self.leafs {
            let end = usize::from(leaf.first_leaf_face) + usize::from(leaf.num_leaf_faces);
            if end > self.leaf_faces.len() {
                return drop("Mod_LoadLeafs: wrong marksurfaces position".into());
            }
        }
        // C reads each entry as a signed short.
        for &face in &self.leaf_faces {
            if !in_range(i64::from(face as i16), self.faces.len()) {
                return drop("Mod_LoadMarksurfaces: bad surface number".into());
            }
        }
        for (index, model) in self.models.iter().enumerate() {
            if usize::try_from(model.head_node).is_ok_and(|node| node >= self.nodes.len()) {
                return drop(format!(
                    "Mod_LoadSubmodels: Inline model {index} has bad firstnode"
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A valid `.bsp` file with empty lumps except `models` zero-sized
/// submodels, for tests that load maps through a file loader.
#[cfg(test)]
pub(crate) fn test_bsp_bytes(models: usize) -> Vec<u8> {
    use q2_common::bsp_file::{BSPVERSION, HEADER_LUMPS, IDBSPHEADER, LUMP_MODELS};
    const MODEL_SIZE: usize = 48;
    let header_size = (4 + 4 + HEADER_LUMPS * 8) as u32;
    let mut buf = Vec::new();
    buf.extend_from_slice(&IDBSPHEADER.to_le_bytes());
    buf.extend_from_slice(&BSPVERSION.to_le_bytes());
    // Every lump starts right after the header; only models has a length.
    for lump in 0..HEADER_LUMPS {
        let length = if lump == LUMP_MODELS {
            models * MODEL_SIZE
        } else {
            0
        };
        buf.extend_from_slice(&header_size.to_le_bytes());
        buf.extend_from_slice(&(length as u32).to_le_bytes());
    }
    // Zeroed models but for head node -1: with no nodes, C rejects head
    // node 0 ("bad firstnode").
    for _ in 0..models {
        let mut model = [0u8; MODEL_SIZE];
        model[36..40].copy_from_slice(&(-1i32).to_le_bytes());
        buf.extend_from_slice(&model);
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use q2_common::bsp_file::{BSPVERSION, HEADER_LUMPS, IDBSPHEADER};

    /// Build a minimal valid BSP byte buffer with header + empty lumps.
    fn make_minimal_bsp() -> Vec<u8> {
        test_bsp_bytes(0)
    }

    #[test]
    fn test_bsp_bytes_parse_with_the_requested_model_count() {
        assert_eq!(BspData::load(&test_bsp_bytes(3)).unwrap().models.len(), 3);
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
        let header_size = (4 + 4 + HEADER_LUMPS * 8) as u32;
        let mut buf = Vec::new();

        // Magic + version
        buf.extend_from_slice(&IDBSPHEADER.to_le_bytes());
        buf.extend_from_slice(&BSPVERSION.to_le_bytes());

        // Vertex data will live right after header.
        // We place 2 vertices (24 bytes) for the vertex lump and leave everything else empty.
        let vert_offset = header_size;
        let vert_length: u32 = 2 * DVERTEX_SIZE as u32;

        for i in 0..HEADER_LUMPS {
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
    fn submodel_bounds_are_spread_by_one_unit() {
        // gl3_model.c Mod_LoadSubmodels: "spread the mins / maxs by a pixel".
        let mut record = Vec::new();
        for f in [0.0f32, 0.0, 0.0, 10.0, 10.0, 10.0, 5.0, 5.0, 5.0] {
            record.extend_from_slice(&f.to_le_bytes());
        }
        for i in [-1i32, 0, 1] {
            record.extend_from_slice(&i.to_le_bytes());
        }
        let mut data = make_minimal_bsp();
        let at = 8 + q2_common::bsp_file::LUMP_MODELS * 8;
        data[at + 4..at + 8].copy_from_slice(&(record.len() as u32).to_le_bytes());
        data.extend_from_slice(&record);
        let models = BspData::load(&data).unwrap().models;
        assert_eq!(models[0].mins, Vec3f::new(-1.0, -1.0, -1.0));
        assert_eq!(models[0].maxs, Vec3f::new(11.0, 11.0, 11.0));
        assert_eq!(models[0].origin, Vec3f::new(5.0, 5.0, 5.0));
    }

    #[test]
    fn raw_lumps_whose_end_overflows_are_rejected() {
        // offset + length wraps a 32-bit usize (wasm32) to a small value.
        for lump in [LUMP_VISIBILITY, LUMP_LIGHTING] {
            let mut data = make_minimal_bsp();
            let at = 8 + lump * 8;
            data[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            data[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());
            assert!(BspData::load(&data).is_err(), "lump {lump}");
        }
    }

    #[test]
    fn bsp_reject_misaligned_lump() {
        let header_size = (4 + 4 + HEADER_LUMPS * 8) as u32;
        let mut buf = Vec::new();

        buf.extend_from_slice(&IDBSPHEADER.to_le_bytes());
        buf.extend_from_slice(&BSPVERSION.to_le_bytes());

        // Vertex lump with length that is not a multiple of 12
        for i in 0..HEADER_LUMPS {
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

    // -- cross-references C rejects with ERR_DROP --------------------------

    /// One record of each cross-referenced lump, all pointing at index 0.
    struct Refs {
        face_plane: u16,
        face_texinfo: i16,
        node_plane: i32,
        node_children: [i32; 2],
        leaf_faces: (u16, u16),
        leaf_face: u16,
        head_node: i32,
    }

    impl Default for Refs {
        fn default() -> Self {
            Refs {
                face_plane: 0,
                face_texinfo: 0,
                node_plane: 0,
                node_children: [-1, -1],
                leaf_faces: (0, 1),
                leaf_face: 0,
                head_node: 0,
            }
        }
    }

    fn refs_bsp(refs: &Refs) -> Vec<u8> {
        use q2_common::bsp_file::{LUMP_LEAFS, LUMP_MODELS, LUMP_NODES, LUMP_PLANES, LUMP_TEXINFO};
        let mut face = Vec::new();
        face.extend_from_slice(&refs.face_plane.to_le_bytes());
        face.extend_from_slice(&0u16.to_le_bytes()); // side
        face.extend_from_slice(&0i32.to_le_bytes()); // first edge
        face.extend_from_slice(&0i16.to_le_bytes()); // edges
        face.extend_from_slice(&refs.face_texinfo.to_le_bytes());
        face.extend_from_slice(&[255; 4]);
        face.extend_from_slice(&(-1i32).to_le_bytes());
        let mut node = refs.node_plane.to_le_bytes().to_vec();
        for child in refs.node_children {
            node.extend_from_slice(&child.to_le_bytes());
        }
        node.extend_from_slice(&[0; 12]); // mins, maxs
        node.extend_from_slice(&[0; 4]); // first face, faces
        let mut leaf = vec![0; 20]; // contents .. maxs
        leaf.extend_from_slice(&refs.leaf_faces.0.to_le_bytes());
        leaf.extend_from_slice(&refs.leaf_faces.1.to_le_bytes());
        leaf.extend_from_slice(&[0; 4]); // leaf brushes
        let mut model = vec![0; 36];
        model.extend_from_slice(&refs.head_node.to_le_bytes());
        model.extend_from_slice(&[0; 8]);
        let lumps = [
            (LUMP_PLANES, vec![0; 20]),
            (LUMP_TEXINFO, vec![0; 76]),
            (LUMP_FACES, face),
            (LUMP_NODES, node),
            (LUMP_LEAFS, leaf),
            (LUMP_LEAFFACES, refs.leaf_face.to_le_bytes().to_vec()),
            (LUMP_MODELS, model),
        ];
        let mut buf = make_minimal_bsp();
        for (lump, bytes) in lumps {
            let at = 8 + lump * 8;
            let offset = buf.len() as u32;
            buf[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            buf[at + 4..at + 8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(&bytes);
        }
        buf
    }

    #[test]
    fn consistent_cross_references_load() {
        BspData::load(&refs_bsp(&Refs::default())).expect("valid references");
    }

    fn rejected(refs: Refs, message: &str) {
        match BspData::load(&refs_bsp(&refs)) {
            Err(Q2Error::Drop(text)) => assert!(text.contains(message), "{text}"),
            other => panic!("expected Drop({message}), got {other:?}"),
        }
    }

    // gl3_model.c Mod_LoadFaces.
    #[test]
    fn a_face_with_a_bad_plane_or_texinfo_is_err_drop_like_c() {
        let face_plane = 1;
        rejected(
            Refs {
                face_plane,
                ..Refs::default()
            },
            "planenum",
        );
        for face_texinfo in [1, -1] {
            rejected(
                Refs {
                    face_texinfo,
                    ..Refs::default()
                },
                "bad texinfo number",
            );
        }
    }

    // models.c Mod_LoadNodes.
    #[test]
    fn a_node_with_a_bad_plane_or_child_is_err_drop_like_c() {
        rejected(
            Refs {
                node_plane: 1,
                ..Refs::default()
            },
            "planenum",
        );
        rejected(
            Refs {
                node_plane: -1,
                ..Refs::default()
            },
            "planenum",
        );
        let node_children = [1, -1];
        rejected(
            Refs {
                node_children,
                ..Refs::default()
            },
            "nodenum",
        );
        let node_children = [-1, -2]; // leaf 1 of 1
        rejected(
            Refs {
                node_children,
                ..Refs::default()
            },
            "leafnum",
        );
    }

    // gl3_model.c Mod_LoadLeafs and Mod_LoadMarksurfaces.
    #[test]
    fn a_bad_leaf_face_range_or_entry_is_err_drop_like_c() {
        let leaf_faces = (1, 1);
        rejected(
            Refs {
                leaf_faces,
                ..Refs::default()
            },
            "marksurfaces",
        );
        rejected(
            Refs {
                leaf_face: 1,
                ..Refs::default()
            },
            "bad surface number",
        );
        // C reads the entry as a signed short.
        rejected(
            Refs {
                leaf_face: 0x8000,
                ..Refs::default()
            },
            "bad surface number",
        );
    }

    // gl3_model.c Mod_LoadSubmodels checks only the upper bound.
    #[test]
    fn a_submodel_head_node_past_the_nodes_is_err_drop_like_c() {
        rejected(
            Refs {
                head_node: 1,
                ..Refs::default()
            },
            "bad firstnode",
        );
        BspData::load(&refs_bsp(&Refs {
            head_node: -1,
            ..Refs::default()
        }))
        .expect("C accepts a negative head node");
    }
}
