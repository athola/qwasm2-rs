//! BSP collision model — loads BSP data and performs trace queries.
//!
//! Faithfully ported from Quake 2's `common/collision.c`.
//! The collision model sweeps axis-aligned bounding boxes through the world
//! BSP tree and reports what they hit.

use crate::pmove::angle_vectors;
use crate::{Q2Error, Q2Result};
use q2_shared::{EntityState, Plane, Surface, Trace, Vec3f};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 1/32 epsilon to keep floating point happy — MUST be this exact value.
const DIST_EPSILON: f32 = 0.03125;

use crate::bsp_file::{
    BspHeader, Lump, LUMP_AREAPORTALS, LUMP_AREAS, LUMP_BRUSHES, LUMP_BRUSHSIDES, LUMP_LEAFBRUSHES,
    LUMP_LEAFS, LUMP_MODELS, LUMP_NODES, LUMP_PLANES, LUMP_TEXINFO,
};

// Map limits
const MAX_MAP_MODELS: usize = 1024;
const MAX_MAP_BRUSHES: usize = 8192;
const MAX_MAP_TEXINFO: usize = 8192;
const MAX_MAP_AREAS: usize = 256;
const MAX_MAP_AREAPORTALS: usize = 1024;
const MAX_MAP_PLANES: usize = 65536;
const MAX_MAP_NODES: usize = 65536;
const MAX_MAP_BRUSHSIDES: usize = 65536;
const MAX_MAP_LEAFS: usize = 65536;
const MAX_MAP_LEAFBRUSHES: usize = 65536;

// Content flags
use q2_shared::constants::{CONTENTS_MONSTER, CONTENTS_SOLID, MASK_PLAYERSOLID};

// On-disk struct sizes (bytes)
const DPLANE_SIZE: usize = 20; // 3 floats + 1 float + 1 int
const DNODE_SIZE: usize = 28; // 1 int + 2 int + 6 short + 2 ushort
const DLEAF_SIZE: usize = 28; // 1 int + 2 short + 6 short + 4 ushort
const DBRUSH_SIZE: usize = 12; // 3 int
const DBRUSHSIDE_SIZE: usize = 4; // 1 ushort + 1 short
const DMODEL_SIZE: usize = 48; // 9 float + 3 int
const TEXINFO_SIZE: usize = 76; // 8 float + 2 int + 32 char + 1 int
const DAREA_SIZE: usize = 8; // 2 int
const DAREAPORTAL_SIZE: usize = 8; // 2 int

// ---------------------------------------------------------------------------
// Internal data structures
// ---------------------------------------------------------------------------

/// Collision plane — internal representation.
#[derive(Debug, Clone)]
pub struct CPlane {
    pub normal: Vec3f,
    pub dist: f32,
    pub plane_type: u8,
    pub sign_bits: u8,
}

impl Default for CPlane {
    fn default() -> Self {
        Self {
            normal: Vec3f::ZERO,
            dist: 0.0,
            plane_type: 0,
            sign_bits: 0,
        }
    }
}

/// BSP internal node.
#[derive(Debug, Clone)]
pub struct CNode {
    pub plane_idx: usize,
    pub children: [i32; 2],
}

/// BSP leaf (contains brushes for collision).
#[derive(Debug, Clone, Default)]
pub struct CLeaf {
    pub contents: i32,
    pub cluster: i16,
    pub area: i16,
    pub first_leaf_brush: u16,
    pub num_leaf_brushes: u16,
}

/// Collision brush (convex solid).
#[derive(Debug, Clone, Default)]
pub struct CBrush {
    pub contents: i32,
    pub num_sides: i32,
    pub first_brush_side: i32,
    pub check_count: i32,
}

/// One face of a brush.
#[derive(Debug, Clone)]
pub struct CBrushSide {
    pub plane_idx: usize,
    pub surface_idx: i32, // index into surfaces array, -1 = null surface
}

/// Surface info loaded from texinfo lump.
#[derive(Debug, Clone, Default)]
pub struct MapSurface {
    pub name: String,
    pub flags: i32,
    pub value: i32,
}

/// The entities a client-side movement trace clips against (C
/// `CL_ClipMoveToEntities` reads these from `cl.frame` and `cl.model_clip`).
#[derive(Debug, Clone, Copy)]
pub struct ClipEntities<'a> {
    /// Entities in the current frame.
    pub entities: &'a [EntityState],
    /// Entity number of the local player (`cl.playernum + 1`), never clipped.
    pub player_number: i32,
    /// Head node of each inline model by model index (`cl.model_clip`);
    /// `None` or a missing index skips a `solid == 31` entity, as in C.
    pub model_clip: &'a [Option<i32>],
}

/// Collision model (world or entity submodel).
#[derive(Debug, Clone, Default)]
pub struct CModel {
    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub origin: Vec3f,
    pub head_node: i32,
}

/// Area for area-portal connectivity.
#[derive(Debug, Clone, Default)]
pub struct CArea {
    pub num_area_portals: i32,
    pub first_area_portal: i32,
    pub flood_num: i32,
    pub flood_valid: i32,
}

/// Area portal (on disk format reused).
#[derive(Debug, Clone, Default)]
pub struct CAreaPortal {
    pub portal_num: i32,
    pub other_area: i32,
}

// ---------------------------------------------------------------------------
// Trace state (replaces the C globals)
// ---------------------------------------------------------------------------

/// Mutable state used during a single trace operation.
struct TraceState {
    contents: i32,
    is_point: bool,
    start: Vec3f,
    end: Vec3f,
    mins: Vec3f,
    maxs: Vec3f,
    extents: Vec3f,
    trace: Trace,
}

// ---------------------------------------------------------------------------
// CollisionMap
// ---------------------------------------------------------------------------

/// The collision map — holds all BSP collision data.
pub struct CollisionMap {
    planes: Vec<CPlane>,
    nodes: Vec<CNode>,
    leafs: Vec<CLeaf>,
    leaf_brushes: Vec<u16>,
    brushes: Vec<CBrush>,
    brush_sides: Vec<CBrushSide>,
    surfaces: Vec<MapSurface>,
    models: Vec<CModel>,
    check_count: i32,

    // Box hull (temporary entity collision)
    box_head_node: i32,
    box_brush_idx: usize,
    box_leaf_idx: usize,
    // Indices into planes for the 12 box planes
    box_planes_start: usize,

    empty_leaf: i32,
    #[allow(dead_code)]
    solid_leaf: i32,

    // Area connectivity
    areas: Vec<CArea>,
    area_portals: Vec<CAreaPortal>,
    /// Entity lump text (C `map_entitystring`), for the game's spawn code.
    entity_string: String,
    portal_open: Vec<bool>,
    flood_valid: i32,
}

impl CollisionMap {
    /// Create a new, empty collision map.
    pub fn new() -> Self {
        Self {
            planes: Vec::new(),
            nodes: Vec::new(),
            leafs: vec![CLeaf::default()], // leaf 0 exists even without a map
            leaf_brushes: Vec::new(),
            brushes: Vec::new(),
            brush_sides: Vec::new(),
            surfaces: Vec::new(),
            models: Vec::new(),
            check_count: 0,
            box_head_node: 0,
            box_brush_idx: 0,
            box_leaf_idx: 0,
            box_planes_start: 0,
            empty_leaf: -1,
            solid_leaf: 0,
            areas: vec![CArea::default()], // area 0 is unused
            area_portals: Vec::new(),
            entity_string: String::new(),
            portal_open: vec![false; MAX_MAP_AREAPORTALS],
            flood_valid: 0,
        }
    }

    // -----------------------------------------------------------------------
    // BSP Loading
    // -----------------------------------------------------------------------

    /// Load BSP collision data from raw BSP file bytes.
    /// Returns a checksum of the file data.
    ///
    /// Loads into a fresh map and swaps it in, so a malformed file leaves an
    /// empty map rather than a half-loaded one that later traces trust.
    pub fn load_map(&mut self, data: &[u8]) -> Q2Result<u32> {
        let mut fresh = CollisionMap::new();
        let result = fresh.load_map_data(data);
        *self = if result.is_ok() {
            fresh
        } else {
            CollisionMap::new()
        };
        result
    }

    fn load_map_data(&mut self, data: &[u8]) -> Q2Result<u32> {
        self.planes.clear();
        self.nodes.clear();
        self.leafs.clear();
        self.leaf_brushes.clear();
        self.brushes.clear();
        self.brush_sides.clear();
        self.surfaces.clear();
        self.models.clear();
        self.areas.clear();
        self.area_portals.clear();
        self.check_count = 0;

        if data.is_empty() {
            // Empty map — cinematic servers. C CM_LoadMap returns before
            // CM_InitBoxHull, so there is no box tree to trace against.
            self.leafs.push(CLeaf::default());
            self.areas.push(CArea::default());
            return Ok(0);
        }

        let header = BspHeader::parse(data)?;
        let lumps = header.lumps;

        // Com_BlockChecksum: MD4 hash, then XOR the four 32-bit digest words.
        let checksum = com_block_checksum(data);

        // Load lumps in the same order as the C code
        self.load_surfaces(data, lumps[LUMP_TEXINFO])?;
        self.load_leafs(data, lumps[LUMP_LEAFS])?;
        self.load_leaf_brushes(data, lumps[LUMP_LEAFBRUSHES])?;
        self.load_planes(data, lumps[LUMP_PLANES])?;
        self.load_brushes(data, lumps[LUMP_BRUSHES])?;
        self.load_brush_sides(data, lumps[LUMP_BRUSHSIDES])?;
        self.load_submodels(data, lumps[LUMP_MODELS])?;
        self.load_nodes(data, lumps[LUMP_NODES])?;
        self.load_areas(data, lumps[LUMP_AREAS])?;
        self.load_area_portals(data, lumps[LUMP_AREAPORTALS])?;
        self.entity_string = header.entity_string(data)?;

        self.validate_references()?;
        self.init_box_hull()?;

        self.portal_open = vec![false; MAX_MAP_AREAPORTALS];
        self.flood_area_connections();

        Ok(checksum)
    }

    fn load_planes(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DPLANE_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadPlanes: funny lump size".into()));
        }
        let count = len / DPLANE_SIZE;
        if count < 1 {
            return Err(Q2Error::Drop("Map with no planes".into()));
        }
        if count > MAX_MAP_PLANES {
            return Err(Q2Error::Drop("Map has too many planes".into()));
        }

        self.planes.reserve(count);
        for i in 0..count {
            let base = ofs + i * DPLANE_SIZE;
            let nx = try_read_f32(data, base)?;
            let ny = try_read_f32(data, base + 4)?;
            let nz = try_read_f32(data, base + 8)?;
            let dist = try_read_f32(data, base + 12)?;
            let ptype = try_read_i32(data, base + 16)? as u8;

            let mut bits: u8 = 0;
            if nx < 0.0 {
                bits |= 1;
            }
            if ny < 0.0 {
                bits |= 2;
            }
            if nz < 0.0 {
                bits |= 4;
            }

            self.planes.push(CPlane {
                normal: Vec3f::new(nx, ny, nz),
                dist,
                plane_type: ptype,
                sign_bits: bits,
            });
        }
        Ok(())
    }

    fn load_nodes(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DNODE_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadNodes: funny lump size".into()));
        }
        let count = len / DNODE_SIZE;
        if count < 1 {
            return Err(Q2Error::Drop("Map has no nodes".into()));
        }
        if count > MAX_MAP_NODES {
            return Err(Q2Error::Drop("Map has too many nodes".into()));
        }

        self.nodes.reserve(count);
        for i in 0..count {
            let base = ofs + i * DNODE_SIZE;
            let planenum = try_read_i32(data, base)? as usize;
            let child0 = try_read_i32(data, base + 4)?;
            let child1 = try_read_i32(data, base + 8)?;
            self.nodes.push(CNode {
                plane_idx: planenum,
                children: [child0, child1],
            });
        }
        Ok(())
    }

    fn load_leafs(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DLEAF_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadLeafs: funny lump size".into()));
        }
        let count = len / DLEAF_SIZE;
        if count < 1 {
            return Err(Q2Error::Drop("Map with no leafs".into()));
        }
        if count > MAX_MAP_LEAFS {
            return Err(Q2Error::Drop("Map has too many leafs".into()));
        }

        self.leafs.clear();
        self.leafs.reserve(count);
        for i in 0..count {
            let base = ofs + i * DLEAF_SIZE;
            let contents = try_read_i32(data, base)?;
            let cluster = try_read_i16(data, base + 4)?;
            let area = try_read_i16(data, base + 6)?;
            // skip mins[3] and maxs[3] (6 shorts = 12 bytes at offset 8..20)
            // dleaf_t layout:
            //   int contents (4), short cluster (2), short area (2),
            //   short mins[3] (6), short maxs[3] (6),
            //   ushort firstleafface (2), ushort numleaffaces (2),
            //   ushort firstleafbrush (2), ushort numleafbrushes (2)
            // Total = 28
            let first_leaf_brush = try_read_u16(data, base + 24)?;
            let num_leaf_brushes = try_read_u16(data, base + 26)?;

            self.leafs.push(CLeaf {
                contents,
                cluster,
                area,
                first_leaf_brush,
                num_leaf_brushes,
            });
        }

        // Validate leaf 0 is solid
        if self.leafs[0].contents != CONTENTS_SOLID {
            return Err(Q2Error::Drop("Map leaf 0 is not CONTENTS_SOLID".into()));
        }

        self.solid_leaf = 0;
        self.empty_leaf = -1;
        for i in 1..self.leafs.len() {
            if self.leafs[i].contents == 0 {
                self.empty_leaf = i as i32;
                break;
            }
        }
        if self.empty_leaf == -1 {
            return Err(Q2Error::Drop("Map does not have an empty leaf".into()));
        }

        Ok(())
    }

    fn load_leaf_brushes(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(2) {
            return Err(Q2Error::Drop("Mod_LoadLeafBrushes: funny lump size".into()));
        }
        let count = len / 2;
        if count < 1 {
            return Err(Q2Error::Drop("Map with no leaf brushes".into()));
        }
        if count > MAX_MAP_LEAFBRUSHES {
            return Err(Q2Error::Drop("Map has too many leafbrushes".into()));
        }

        self.leaf_brushes.reserve(count);
        for i in 0..count {
            self.leaf_brushes.push(try_read_u16(data, ofs + i * 2)?);
        }
        Ok(())
    }

    fn load_brushes(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DBRUSH_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadBrushes: funny lump size".into()));
        }
        let count = len / DBRUSH_SIZE;
        if count > MAX_MAP_BRUSHES {
            return Err(Q2Error::Drop("Map has too many brushes".into()));
        }

        self.brushes.reserve(count);
        for i in 0..count {
            let base = ofs + i * DBRUSH_SIZE;
            self.brushes.push(CBrush {
                first_brush_side: try_read_i32(data, base)?,
                num_sides: try_read_i32(data, base + 4)?,
                contents: try_read_i32(data, base + 8)?,
                check_count: 0,
            });
        }
        Ok(())
    }

    fn load_brush_sides(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DBRUSHSIDE_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadBrushSides: funny lump size".into()));
        }
        let count = len / DBRUSHSIDE_SIZE;
        if count > MAX_MAP_BRUSHSIDES {
            return Err(Q2Error::Drop("Map has too many brush sides".into()));
        }

        let num_texinfo = self.surfaces.len() as i32;
        self.brush_sides.reserve(count);
        for i in 0..count {
            let base = ofs + i * DBRUSHSIDE_SIZE;
            let planenum = try_read_u16(data, base)? as usize;
            let texinfo = try_read_i16(data, base + 2)? as i32;

            if texinfo >= num_texinfo {
                return Err(Q2Error::Drop("Bad brushside texinfo".into()));
            }

            self.brush_sides.push(CBrushSide {
                plane_idx: planenum,
                surface_idx: texinfo,
            });
        }
        Ok(())
    }

    fn load_surfaces(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(TEXINFO_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadSurfaces: funny lump size".into()));
        }
        let count = len / TEXINFO_SIZE;
        if count < 1 {
            return Err(Q2Error::Drop("Map with no surfaces".into()));
        }
        if count > MAX_MAP_TEXINFO {
            return Err(Q2Error::Drop("Map has too many surfaces".into()));
        }

        self.surfaces.reserve(count);
        for i in 0..count {
            let base = ofs + i * TEXINFO_SIZE;
            // texinfo_t: float vecs[2][4] (32 bytes), int flags (4), int value (4),
            //            char texture[32] (32), int nexttexinfo (4)
            let flags = try_read_i32(data, base + 32)?;
            let value = try_read_i32(data, base + 36)?;

            // Read texture name (32 bytes, null terminated)
            let name_start = base + 40;
            let name_end = name_start + 32;
            let name_bytes = data
                .get(name_start..name_end)
                .ok_or_else(|| Q2Error::Drop("Mod_LoadSurfaces: truncated texinfo".into()))?;
            let nul_pos = name_bytes.iter().position(|&b| b == 0).unwrap_or(32);
            let name = String::from_utf8_lossy(&name_bytes[..nul_pos]).into_owned();

            self.surfaces.push(MapSurface { name, flags, value });
        }
        Ok(())
    }

    fn load_submodels(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DMODEL_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadSubmodels: funny lump size".into()));
        }
        let count = len / DMODEL_SIZE;
        if count < 1 {
            return Err(Q2Error::Drop("Map with no models".into()));
        }
        if count > MAX_MAP_MODELS {
            return Err(Q2Error::Drop("Map has too many models".into()));
        }

        self.models.reserve(count);
        for i in 0..count {
            let base = ofs + i * DMODEL_SIZE;
            // dmodel_t: float mins[3], maxs[3], origin[3], int headnode, int firstface, int numfaces
            let mins = Vec3f::new(
                try_read_f32(data, base)? - 1.0,
                try_read_f32(data, base + 4)? - 1.0,
                try_read_f32(data, base + 8)? - 1.0,
            );
            let maxs = Vec3f::new(
                try_read_f32(data, base + 12)? + 1.0,
                try_read_f32(data, base + 16)? + 1.0,
                try_read_f32(data, base + 20)? + 1.0,
            );
            let origin = Vec3f::new(
                try_read_f32(data, base + 24)?,
                try_read_f32(data, base + 28)?,
                try_read_f32(data, base + 32)?,
            );
            let head_node = try_read_i32(data, base + 36)?;

            self.models.push(CModel {
                mins,
                maxs,
                origin,
                head_node,
            });
        }
        Ok(())
    }

    fn load_areas(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DAREA_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadAreas: funny lump size".into()));
        }
        let count = len / DAREA_SIZE;
        if count > MAX_MAP_AREAS {
            return Err(Q2Error::Drop("Map has too many areas".into()));
        }

        self.areas.clear();
        self.areas.reserve(count);
        for i in 0..count {
            let base = ofs + i * DAREA_SIZE;
            self.areas.push(CArea {
                num_area_portals: try_read_i32(data, base)?,
                first_area_portal: try_read_i32(data, base + 4)?,
                flood_num: 0,
                flood_valid: 0,
            });
        }
        Ok(())
    }

    fn load_area_portals(&mut self, data: &[u8], lump: Lump) -> Q2Result<()> {
        let Lump {
            offset: ofs,
            length: len,
        } = lump;
        if !len.is_multiple_of(DAREAPORTAL_SIZE) {
            return Err(Q2Error::Drop("Mod_LoadAreaPortals: funny lump size".into()));
        }
        let count = len / DAREAPORTAL_SIZE;
        if count > MAX_MAP_AREAPORTALS {
            return Err(Q2Error::Drop("Map has too many area portals".into()));
        }

        self.area_portals.clear();
        self.area_portals.reserve(count);
        for i in 0..count {
            let base = ofs + i * DAREAPORTAL_SIZE;
            self.area_portals.push(CAreaPortal {
                portal_num: try_read_i32(data, base)?,
                other_area: try_read_i32(data, base + 4)?,
            });
        }
        Ok(())
    }

    /// Check every cross-reference between lumps so the trace code can index
    /// without bounds failures. C trusts the file (out-of-range indices are
    /// UB there); here they are ERR_DROP. Child nodes must have a larger index
    /// than their parent (qbsp emits nodes in preorder), which rules out
    /// cycles that would make the tree walks loop or recurse forever.
    fn validate_references(&self) -> Q2Result<()> {
        let bad = |what: String| Err(Q2Error::Drop(format!("CM_LoadMap: {what}")));
        let leaf_ok = |child: i32| ((-1 - child) as usize) < self.leafs.len();

        for (i, node) in self.nodes.iter().enumerate() {
            if node.plane_idx >= self.planes.len() {
                return bad(format!("node {i} has bad plane {}", node.plane_idx));
            }
            for child in node.children {
                let ok = if child >= 0 {
                    (child as usize) < self.nodes.len() && (child as usize) > i
                } else {
                    leaf_ok(child)
                };
                if !ok {
                    return bad(format!("node {i} has bad child {child}"));
                }
            }
        }

        for (i, brush) in self.brushes.iter().enumerate() {
            let in_range = brush.first_brush_side >= 0
                && brush.num_sides >= 0
                && (brush.first_brush_side as usize)
                    .checked_add(brush.num_sides as usize)
                    .is_some_and(|end| end <= self.brush_sides.len());
            if !in_range {
                return bad(format!("brush {i} has bad side range"));
            }
        }

        if let Some(i) = self
            .brush_sides
            .iter()
            .position(|side| side.plane_idx >= self.planes.len())
        {
            return bad(format!("brush side {i} has bad plane"));
        }

        for (i, leaf) in self.leafs.iter().enumerate() {
            let end = leaf.first_leaf_brush as usize + leaf.num_leaf_brushes as usize;
            if end > self.leaf_brushes.len() {
                return bad(format!("leaf {i} has bad leaf brush range"));
            }
        }

        if let Some(i) = self
            .leaf_brushes
            .iter()
            .position(|&b| b as usize >= self.brushes.len())
        {
            return bad(format!("leaf brush {i} names a missing brush"));
        }

        for (i, model) in self.models.iter().enumerate() {
            let head = model.head_node;
            let ok = if head >= 0 {
                (head as usize) < self.nodes.len()
            } else {
                leaf_ok(head)
            };
            if !ok {
                return bad(format!("model {i} has bad head node {head}"));
            }
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Box hull — for entity-vs-entity collision
    // -----------------------------------------------------------------------

    /// Set up the planes and nodes so that a bounding box can be used
    /// as a clipping hull structure.
    fn init_box_hull(&mut self) -> Q2Result<()> {
        let num_nodes = self.nodes.len();
        let num_planes = self.planes.len();
        let num_brushes = self.brushes.len();
        let num_brush_sides = self.brush_sides.len();
        let num_leafs = self.leafs.len();
        let num_leaf_brushes = self.leaf_brushes.len();

        // C CM_InitBoxHull room check. It also keeps the box leaf's u16
        // first_leaf_brush from truncating.
        if num_nodes + 6 > MAX_MAP_NODES
            || num_brushes + 1 > MAX_MAP_BRUSHES
            || num_leaf_brushes + 1 > MAX_MAP_LEAFBRUSHES
            || num_brush_sides + 6 > MAX_MAP_BRUSHSIDES
            || num_planes + 12 > MAX_MAP_PLANES
        {
            return Err(Q2Error::Drop("Not enough room for box tree".into()));
        }

        self.box_head_node = num_nodes as i32;
        self.box_planes_start = num_planes;

        // Add the box brush
        self.box_brush_idx = num_brushes;
        self.brushes.push(CBrush {
            num_sides: 6,
            first_brush_side: num_brush_sides as i32,
            contents: CONTENTS_MONSTER,
            check_count: 0,
        });

        // Add the box leaf
        self.box_leaf_idx = num_leafs;
        self.leafs.push(CLeaf {
            contents: CONTENTS_MONSTER,
            cluster: -1,
            area: -1,
            first_leaf_brush: num_leaf_brushes as u16,
            num_leaf_brushes: 1,
        });

        self.leaf_brushes.push(num_brushes as u16);

        for i in 0..6 {
            let side = i & 1;

            // Brush sides
            let plane_side_idx = num_planes + i * 2 + side;
            self.brush_sides.push(CBrushSide {
                plane_idx: plane_side_idx,
                surface_idx: -1,
            });

            // Nodes
            let child_side = -1 - self.empty_leaf;
            // Safe: indices are ≤ 12 (6 axial planes × 2 sides), well within i32 range.
            let child_other = if i != 5 {
                (num_nodes + i + 1) as i32
            } else {
                -1 - num_leafs as i32
            };

            let mut children = [0i32; 2];
            children[side] = child_side;
            children[side ^ 1] = child_other;

            self.nodes.push(CNode {
                plane_idx: num_planes + i * 2,
                children,
            });

            // Planes — two per axis (positive and negative)
            let axis = i >> 1;

            // Positive plane
            let mut normal = Vec3f::ZERO;
            normal[axis] = 1.0;
            self.planes.push(CPlane {
                normal,
                dist: 0.0,
                plane_type: axis as u8,
                sign_bits: 0,
            });

            // Negative plane
            let mut normal_neg = Vec3f::ZERO;
            normal_neg[axis] = -1.0;
            self.planes.push(CPlane {
                normal: normal_neg,
                dist: 0.0,
                plane_type: (3 + axis) as u8,
                sign_bits: 0,
            });
        }
        Ok(())
    }

    /// Set up box planes for a specific bounding box, returns the head node
    /// for tracing against this box.
    ///
    /// Without a loaded map there is no box tree; the returned head then
    /// refers to an empty node list, which traces and point queries treat as
    /// "nothing to hit".
    pub fn headnode_for_box(&mut self, mins: Vec3f, maxs: Vec3f) -> i32 {
        let bp = self.box_planes_start;
        if let Some(p) = self.planes.get_mut(bp..bp + 12) {
            p[0].dist = maxs.x;
            p[1].dist = -maxs.x;
            p[2].dist = mins.x;
            p[3].dist = -mins.x;
            p[4].dist = maxs.y;
            p[5].dist = -maxs.y;
            p[6].dist = mins.y;
            p[7].dist = -mins.y;
            p[8].dist = maxs.z;
            p[9].dist = -maxs.z;
            p[10].dist = mins.z;
            p[11].dist = -mins.z;
        }
        self.box_head_node
    }

    // -----------------------------------------------------------------------
    // Area connectivity
    // -----------------------------------------------------------------------

    fn flood_area_r(&mut self, area_idx: usize, flood_num: i32) {
        let area = &self.areas[area_idx];
        if area.flood_valid == self.flood_valid {
            return; // Already flooded
        }

        let num_portals = area.num_area_portals;
        let first_portal = area.first_area_portal as usize;

        self.areas[area_idx].flood_num = flood_num;
        self.areas[area_idx].flood_valid = self.flood_valid;

        for i in 0..num_portals as usize {
            let portal_idx = first_portal + i;
            if portal_idx >= self.area_portals.len() {
                break;
            }
            let portal = &self.area_portals[portal_idx];
            let portal_num = portal.portal_num as usize;
            let other_area = portal.other_area as usize;

            if portal_num < self.portal_open.len()
                && self.portal_open[portal_num]
                && other_area < self.areas.len()
            {
                self.flood_area_r(other_area, flood_num);
            }
        }
    }

    fn flood_area_connections(&mut self) {
        self.flood_valid += 1;
        let mut flood_num = 0i32;
        let num_areas = self.areas.len();

        // area 0 is not used
        for i in 1..num_areas {
            if self.areas[i].flood_valid == self.flood_valid {
                continue;
            }
            flood_num += 1;
            self.flood_area_r(i, flood_num);
        }
    }

    /// The map's entity lump (C CM_EntityString).
    pub fn entity_string(&self) -> &str {
        &self.entity_string
    }

    /// Set an area portal open or closed and recalculate connectivity.
    ///
    /// Like C CM_SetAreaPortalState, a portal number past the map's portal
    /// count is ERR_DROP rather than a silent no-op.
    pub fn set_area_portal_state(&mut self, portal_num: usize, open: bool) -> Q2Result<()> {
        if portal_num > self.area_portals.len() || portal_num >= self.portal_open.len() {
            return Err(Q2Error::Drop("areaportal > numareaportals".into()));
        }
        self.portal_open[portal_num] = open;
        self.flood_area_connections();
        Ok(())
    }

    /// Check if two areas are connected (can see / hear each other).
    ///
    /// C CM_AreasConnected raises ERR_DROP for an area past the map's area
    /// count; negative and == count are rejected too, since C reads past
    /// the array for those.
    pub fn areas_connected(&self, area1: i32, area2: i32) -> Q2Result<bool> {
        let (Ok(a1), Ok(a2)) = (usize::try_from(area1), usize::try_from(area2)) else {
            return Err(Q2Error::Drop("area > numareas".into()));
        };
        match (self.areas.get(a1), self.areas.get(a2)) {
            (Some(x), Some(y)) => Ok(x.flood_num == y.flood_num),
            _ => Err(Q2Error::Drop("area > numareas".into())),
        }
    }

    // -----------------------------------------------------------------------
    // Point queries
    // -----------------------------------------------------------------------

    /// Walk the BSP tree to find which leaf a point falls in.
    fn point_leafnum_r(&self, point: Vec3f, mut num: i32) -> usize {
        while num >= 0 {
            let node = &self.nodes[num as usize];
            let plane = &self.planes[node.plane_idx];

            let d = if plane.plane_type < 3 {
                point[plane.plane_type as usize] - plane.dist
            } else {
                plane.normal.dot(point) - plane.dist
            };

            if d < 0.0 {
                num = node.children[1];
            } else {
                num = node.children[0];
            }
        }
        (-1 - num) as usize
    }

    /// Get contents at a point.
    pub fn point_contents(&self, point: Vec3f, head_node: i32) -> i32 {
        if self.nodes.is_empty() {
            return 0;
        }
        let leaf_idx = self.point_leafnum_r(point, head_node);
        if leaf_idx < self.leafs.len() {
            self.leafs[leaf_idx].contents
        } else {
            0
        }
    }

    /// Get point leaf number (from world root).
    pub fn point_leafnum(&self, point: Vec3f) -> usize {
        if self.planes.is_empty() {
            return 0;
        }
        self.point_leafnum_r(point, 0)
    }

    // -----------------------------------------------------------------------
    // Box leaf enumeration
    // -----------------------------------------------------------------------

    /// Determine which side of a plane a box is on.
    /// Returns 1 = front, 2 = back, 3 = crossing.
    fn box_on_plane_side(mins: &Vec3f, maxs: &Vec3f, plane: &CPlane) -> i32 {
        // Fast axial case, exactly as C BOX_ON_PLANE_SIDE: a box touching
        // the plane from one side is on that side only.
        if plane.plane_type < 3 {
            let axis = plane.plane_type as usize;
            return if plane.dist <= mins[axis] {
                1
            } else if plane.dist >= maxs[axis] {
                2
            } else {
                3
            };
        }

        // General case based on sign bits
        let (p1, p2) = match plane.sign_bits {
            0 => (
                Vec3f::new(maxs.x, maxs.y, maxs.z),
                Vec3f::new(mins.x, mins.y, mins.z),
            ),
            1 => (
                Vec3f::new(mins.x, maxs.y, maxs.z),
                Vec3f::new(maxs.x, mins.y, mins.z),
            ),
            2 => (
                Vec3f::new(maxs.x, mins.y, maxs.z),
                Vec3f::new(mins.x, maxs.y, mins.z),
            ),
            3 => (
                Vec3f::new(mins.x, mins.y, maxs.z),
                Vec3f::new(maxs.x, maxs.y, mins.z),
            ),
            4 => (
                Vec3f::new(maxs.x, maxs.y, mins.z),
                Vec3f::new(mins.x, mins.y, maxs.z),
            ),
            5 => (
                Vec3f::new(mins.x, maxs.y, mins.z),
                Vec3f::new(maxs.x, mins.y, maxs.z),
            ),
            6 => (
                Vec3f::new(maxs.x, mins.y, mins.z),
                Vec3f::new(mins.x, maxs.y, maxs.z),
            ),
            7 => (
                Vec3f::new(mins.x, mins.y, mins.z),
                Vec3f::new(maxs.x, maxs.y, maxs.z),
            ),
            _ => (
                Vec3f::new(maxs.x, maxs.y, maxs.z),
                Vec3f::new(mins.x, mins.y, mins.z),
            ),
        };
        let d1 = plane.normal.dot(p1) - plane.dist;
        let d2 = plane.normal.dot(p2) - plane.dist;

        let mut sides = 0;
        if d1 >= 0.0 {
            sides = 1;
        }
        if d2 < 0.0 {
            sides |= 2;
        }
        sides
    }

    /// Find all leaf indices that a box touches under a given head node.
    fn box_leafnums_headnode(
        &self,
        mins: Vec3f,
        maxs: Vec3f,
        max_count: usize,
        head_node: i32,
    ) -> (Vec<usize>, i32) {
        let mut result = Vec::new();
        let mut top_node = -1i32;
        self.box_leafnums_r(
            head_node,
            &mins,
            &maxs,
            max_count,
            &mut result,
            &mut top_node,
        );
        (result, top_node)
    }

    fn box_leafnums_r(
        &self,
        mut nodenum: i32,
        mins: &Vec3f,
        maxs: &Vec3f,
        max_count: usize,
        result: &mut Vec<usize>,
        top_node: &mut i32,
    ) {
        loop {
            if nodenum < 0 {
                if result.len() >= max_count {
                    return;
                }
                result.push((-1 - nodenum) as usize);
                return;
            }

            let node = &self.nodes[nodenum as usize];
            let plane = &self.planes[node.plane_idx];
            let s = Self::box_on_plane_side(mins, maxs, plane);

            if s == 1 {
                nodenum = node.children[0];
            } else if s == 2 {
                nodenum = node.children[1];
            } else {
                // Crosses both sides
                if *top_node == -1 {
                    *top_node = nodenum;
                }
                self.box_leafnums_r(node.children[0], mins, maxs, max_count, result, top_node);
                nodenum = node.children[1];
            }
        }
    }

    // -----------------------------------------------------------------------
    // Trace — sweep a box through the world
    // -----------------------------------------------------------------------

    /// Trace against the world model and report the world entity on a hit.
    ///
    /// The world-clip half of C `CL_PMTrace` / `SV_Trace`: `box_trace` knows
    /// nothing of entities, and pmove only counts a trace as ground when an
    /// entity is set. Argument order follows the C wrappers. Clipping against
    /// other entities stays with the callers that own them (#11, #19).
    pub fn world_trace(
        &mut self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        brush_mask: i32,
    ) -> Trace {
        let mut trace = self.box_trace(start, end, mins, maxs, 0, brush_mask);
        if trace.fraction < 1.0 {
            trace.ent_index = Some(0);
        }
        trace
    }

    /// Clip a movement trace against frame entities, C CL_ClipMoveToEntities.
    ///
    /// `tr` is the world trace (see `world_trace`); a closer entity hit
    /// replaces it and sets `ent_index` to the entity number. Unwired until
    /// client prediction lands (#19); server-side clipping needs linked
    /// entities (#11, #31).
    pub fn clip_move_to_entities(
        &mut self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        clip: &ClipEntities,
        tr: &mut Trace,
    ) {
        for ent in clip.entities {
            if ent.solid == 0 || ent.number == clip.player_number {
                continue;
            }

            let (head_node, angles) = if ent.solid == 31 {
                // Special value for brush models.
                let model = usize::try_from(ent.modelindex)
                    .ok()
                    .and_then(|i| clip.model_clip.get(i).copied().flatten());
                let Some(head_node) = model else {
                    continue;
                };
                (head_node, ent.angles)
            } else {
                // Encoded bbox; boxes don't rotate.
                let x = (8 * (ent.solid & 31)) as f32;
                let zd = (8 * ((ent.solid >> 5) & 31)) as f32;
                let zu = (8 * ((ent.solid >> 10) & 63) - 32) as f32;
                let bmins = Vec3f::new(-x, -x, -zd);
                let bmaxs = Vec3f::new(x, x, zu);
                (self.headnode_for_box(bmins, bmaxs), Vec3f::ZERO)
            };

            if tr.allsolid {
                return;
            }

            let mut trace = self.transformed_box_trace(
                start,
                end,
                mins,
                maxs,
                head_node,
                MASK_PLAYERSOLID,
                ent.origin,
                angles,
            );

            if trace.allsolid || trace.startsolid || trace.fraction < tr.fraction {
                trace.ent_index = usize::try_from(ent.number).ok();
                let was_startsolid = tr.startsolid;
                *tr = trace;
                if was_startsolid {
                    tr.startsolid = true;
                }
            }
        }
    }

    /// Trace a box through the world BSP.
    ///
    /// Takes `&mut self` because `check_count` is used to avoid testing the
    /// same brush twice in a single trace (matching the C global).
    /// Single-threaded assumption: concurrent traces are not supported.
    pub fn box_trace(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        head_node: i32,
        brush_mask: i32,
    ) -> Trace {
        // C `checkcount++` on an int; wrap instead of overflowing.
        self.check_count = self.check_count.wrapping_add(1);

        let mut state = TraceState {
            contents: brush_mask,
            is_point: false,
            start,
            end,
            mins,
            maxs,
            extents: Vec3f::ZERO,
            trace: Trace::default(),
        };

        if self.nodes.is_empty() {
            // No map loaded — no collision, full sweep
            state.trace.endpos = end;
            return state.trace;
        }

        // Position test special case (start == end)
        if start == end {
            let c1 = start + mins - Vec3f::ONE;
            let c2 = start + maxs + Vec3f::ONE;

            let (leaf_list, _top_node) = self.box_leafnums_headnode(c1, c2, 1024, head_node);

            for &leaf_idx in &leaf_list {
                self.test_in_leaf(leaf_idx, &mut state);
                if state.trace.allsolid {
                    break;
                }
            }

            state.trace.endpos = start;
            return state.trace;
        }

        // Check for point special case
        if mins == Vec3f::ZERO && maxs == Vec3f::ZERO {
            state.is_point = true;
            state.extents = Vec3f::ZERO;
        } else {
            state.is_point = false;
            state.extents = Vec3f::new(
                (-mins.x).max(maxs.x),
                (-mins.y).max(maxs.y),
                (-mins.z).max(maxs.z),
            );
        }

        // General sweeping through world
        self.recursive_hull_check(head_node, 0.0, 1.0, start, end, &mut state);

        if state.trace.fraction == 1.0 {
            state.trace.endpos = end;
        } else {
            state.trace.endpos = start + state.trace.fraction * (end - start);
        }

        state.trace
    }

    /// Trace with entity transform (origin + angles).
    #[allow(clippy::too_many_arguments)]
    pub fn transformed_box_trace(
        &mut self,
        start: Vec3f,
        end: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        head_node: i32,
        brush_mask: i32,
        origin: Vec3f,
        angles: Vec3f,
    ) -> Trace {
        // Subtract origin offset
        let mut start_l = start - origin;
        let mut end_l = end - origin;

        // Rotate start and end into the model's frame of reference
        let rotated = head_node != self.box_head_node
            && (angles.x != 0.0 || angles.y != 0.0 || angles.z != 0.0);

        if rotated {
            let (forward, right, up) = angle_vectors(angles);

            let temp = start_l;
            start_l.x = temp.dot(forward);
            start_l.y = -temp.dot(right);
            start_l.z = temp.dot(up);

            let temp = end_l;
            end_l.x = temp.dot(forward);
            end_l.y = -temp.dot(right);
            end_l.z = temp.dot(up);
        }

        // Sweep the box through the model
        let mut trace = self.box_trace(start_l, end_l, mins, maxs, head_node, brush_mask);

        if rotated && trace.fraction != 1.0 {
            let neg_angles = -angles;
            let (forward, right, up) = angle_vectors(neg_angles);

            let temp = trace.plane.normal;
            trace.plane.normal.x = temp.dot(forward);
            trace.plane.normal.y = -temp.dot(right);
            trace.plane.normal.z = temp.dot(up);
        }

        trace.endpos = start + trace.fraction * (end - start);
        trace
    }

    // -----------------------------------------------------------------------
    // Brush clipping (inner loop)
    // -----------------------------------------------------------------------

    /// Clip a box to a single brush, updating the trace if this brush is hit earlier.
    fn clip_box_to_brush(
        planes: &[CPlane],
        brush_sides: &[CBrushSide],
        surfaces: &[MapSurface],
        brush: &CBrush,
        state: &mut TraceState,
    ) {
        if brush.num_sides == 0 {
            return;
        }

        let mut enter_frac: f32 = -1.0;
        let mut leave_frac: f32 = 1.0;
        let mut clip_plane_idx: Option<usize> = None;
        let mut lead_side_idx: Option<usize> = None;

        let mut get_out = false;
        let mut start_out = false;

        for i in 0..brush.num_sides as usize {
            let side_idx = brush.first_brush_side as usize + i;
            let side = &brush_sides[side_idx];
            let plane = &planes[side.plane_idx];

            let dist = if !state.is_point {
                // General box case — push the plane out for mins/maxs
                let ofs = Vec3f::new(
                    if plane.normal.x < 0.0 {
                        state.maxs.x
                    } else {
                        state.mins.x
                    },
                    if plane.normal.y < 0.0 {
                        state.maxs.y
                    } else {
                        state.mins.y
                    },
                    if plane.normal.z < 0.0 {
                        state.maxs.z
                    } else {
                        state.mins.z
                    },
                );
                plane.dist - ofs.dot(plane.normal)
            } else {
                plane.dist
            };

            let d1 = plane.normal.dot(state.start) - dist;
            let d2 = plane.normal.dot(state.end) - dist;

            if d2 > 0.0 {
                get_out = true;
            }
            if d1 > 0.0 {
                start_out = true;
            }

            // If completely in front of face, no intersection
            if d1 > 0.0 && d2 >= d1 {
                return;
            }

            if d1 <= 0.0 && d2 <= 0.0 {
                continue;
            }

            // Crosses face
            if d1 > d2 {
                // Enter
                let f = (d1 - DIST_EPSILON) / (d1 - d2);
                if f > enter_frac {
                    enter_frac = f;
                    clip_plane_idx = Some(side.plane_idx);
                    lead_side_idx = Some(side_idx);
                }
            } else {
                // Leave
                let f = (d1 + DIST_EPSILON) / (d1 - d2);
                if f < leave_frac {
                    leave_frac = f;
                }
            }
        }

        if !start_out {
            // Original point was inside brush
            state.trace.startsolid = true;
            if !get_out {
                state.trace.allsolid = true;
            }
            return;
        }

        if enter_frac < leave_frac && enter_frac > -1.0 && enter_frac < state.trace.fraction {
            let enter_frac = enter_frac.max(0.0);

            state.trace.fraction = enter_frac;

            if let Some(pidx) = clip_plane_idx {
                let cp = &planes[pidx];
                state.trace.plane = Plane {
                    normal: cp.normal,
                    dist: cp.dist,
                    plane_type: cp.plane_type,
                    sign_bits: cp.sign_bits,
                };
            }

            if let Some(sidx) = lead_side_idx {
                let side = &brush_sides[sidx];
                if side.surface_idx >= 0 && (side.surface_idx as usize) < surfaces.len() {
                    let surf = &surfaces[side.surface_idx as usize];
                    state.trace.surface = Some(Surface {
                        name: surf.name.clone(),
                        flags: surf.flags,
                        value: surf.value,
                    });
                } else {
                    state.trace.surface = None;
                }
            }

            state.trace.contents = brush.contents;
        }
    }

    /// Test if a box at a single position is inside a brush (no movement).
    fn test_box_in_brush(
        planes: &[CPlane],
        brush_sides: &[CBrushSide],
        brush: &CBrush,
        state: &mut TraceState,
    ) {
        if brush.num_sides == 0 {
            return;
        }

        for i in 0..brush.num_sides as usize {
            let side_idx = brush.first_brush_side as usize + i;
            let side = &brush_sides[side_idx];
            let plane = &planes[side.plane_idx];

            // Push the plane out for mins/maxs
            let ofs = Vec3f::new(
                if plane.normal.x < 0.0 {
                    state.maxs.x
                } else {
                    state.mins.x
                },
                if plane.normal.y < 0.0 {
                    state.maxs.y
                } else {
                    state.mins.y
                },
                if plane.normal.z < 0.0 {
                    state.maxs.z
                } else {
                    state.mins.z
                },
            );

            let dist = plane.dist - ofs.dot(plane.normal);
            let d1 = plane.normal.dot(state.start) - dist;

            if d1 > 0.0 {
                return; // Completely in front of face
            }
        }

        // Inside this brush
        state.trace.startsolid = true;
        state.trace.allsolid = true;
        state.trace.fraction = 0.0;
        state.trace.contents = brush.contents;
    }

    /// Test all brushes in a leaf for tracing.
    fn trace_to_leaf(&mut self, leaf_idx: usize, state: &mut TraceState) {
        let leaf = &self.leafs[leaf_idx];
        if leaf.contents & state.contents == 0 {
            return;
        }

        let first = leaf.first_leaf_brush as usize;
        let count = leaf.num_leaf_brushes as usize;

        for k in 0..count {
            let brush_idx = self.leaf_brushes[first + k] as usize;
            if self.brushes[brush_idx].check_count == self.check_count {
                continue;
            }
            self.brushes[brush_idx].check_count = self.check_count;

            if self.brushes[brush_idx].contents & state.contents == 0 {
                continue;
            }

            // We need to borrow brush data immutably while state is borrowed mutably.
            // Since CBrush fields we need are Copy types, clone the brush.
            let brush = self.brushes[brush_idx].clone();
            Self::clip_box_to_brush(
                &self.planes,
                &self.brush_sides,
                &self.surfaces,
                &brush,
                state,
            );

            if state.trace.fraction == 0.0 {
                return;
            }
        }
    }

    /// Test all brushes in a leaf for position test (start == end).
    fn test_in_leaf(&mut self, leaf_idx: usize, state: &mut TraceState) {
        let leaf = &self.leafs[leaf_idx];
        if leaf.contents & state.contents == 0 {
            return;
        }

        let first = leaf.first_leaf_brush as usize;
        let count = leaf.num_leaf_brushes as usize;

        for k in 0..count {
            let brush_idx = self.leaf_brushes[first + k] as usize;
            if self.brushes[brush_idx].check_count == self.check_count {
                continue;
            }
            self.brushes[brush_idx].check_count = self.check_count;

            if self.brushes[brush_idx].contents & state.contents == 0 {
                continue;
            }

            let brush = self.brushes[brush_idx].clone();
            Self::test_box_in_brush(&self.planes, &self.brush_sides, &brush, state);

            if state.trace.fraction == 0.0 {
                return;
            }
        }
    }

    /// The core recursive hull check — walks the BSP tree and clips
    /// against brushes in each leaf.
    fn recursive_hull_check(
        &mut self,
        num: i32,
        p1f: f32,
        p2f: f32,
        p1: Vec3f,
        p2: Vec3f,
        state: &mut TraceState,
    ) {
        if state.trace.fraction <= p1f {
            return; // Already hit something nearer
        }

        // If < 0, we are in a leaf node
        if num < 0 {
            self.trace_to_leaf((-1 - num) as usize, state);
            return;
        }

        // Find the point distances to the separating plane
        let node = &self.nodes[num as usize];
        let plane_idx = node.plane_idx;
        let children = node.children;
        let plane = &self.planes[plane_idx];

        let (t1, t2, offset) = if plane.plane_type < 3 {
            let axis = plane.plane_type as usize;
            (
                p1[axis] - plane.dist,
                p2[axis] - plane.dist,
                state.extents[axis],
            )
        } else {
            let t1 = plane.normal.dot(p1) - plane.dist;
            let t2 = plane.normal.dot(p2) - plane.dist;
            let offset = if state.is_point {
                0.0
            } else {
                (state.extents.x * plane.normal.x).abs()
                    + (state.extents.y * plane.normal.y).abs()
                    + (state.extents.z * plane.normal.z).abs()
            };
            (t1, t2, offset)
        };

        // See which sides we need to consider
        if t1 >= offset && t2 >= offset {
            self.recursive_hull_check(children[0], p1f, p2f, p1, p2, state);
            return;
        }
        if t1 < -offset && t2 < -offset {
            self.recursive_hull_check(children[1], p1f, p2f, p1, p2, state);
            return;
        }

        // Put the crosspoint DIST_EPSILON pixels on the near side
        let (side, frac, frac2) = if t1 < t2 {
            let idist = 1.0 / (t1 - t2);
            (
                1usize,
                ((t1 - offset + DIST_EPSILON) * idist).clamp(0.0, 1.0),
                ((t1 + offset + DIST_EPSILON) * idist).clamp(0.0, 1.0),
            )
        } else if t1 > t2 {
            let idist = 1.0 / (t1 - t2);
            (
                0usize,
                ((t1 + offset + DIST_EPSILON) * idist).clamp(0.0, 1.0),
                ((t1 - offset - DIST_EPSILON) * idist).clamp(0.0, 1.0),
            )
        } else {
            (0usize, 1.0f32, 0.0f32)
        };

        // Move up to the node
        let midf = p1f + (p2f - p1f) * frac;
        let mid = p1 + frac * (p2 - p1);

        self.recursive_hull_check(children[side], p1f, midf, p1, mid, state);

        // Go past the node
        let midf2 = p1f + (p2f - p1f) * frac2;
        let mid2 = p1 + frac2 * (p2 - p1);

        self.recursive_hull_check(children[side ^ 1], midf2, p2f, mid2, p2, state);
    }

    // -----------------------------------------------------------------------
    // Public accessors
    // -----------------------------------------------------------------------

    /// Get number of models.
    pub fn num_models(&self) -> usize {
        self.models.len()
    }

    pub fn num_brushes(&self) -> usize {
        self.brushes.len()
    }

    pub fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    pub fn num_leafs(&self) -> usize {
        self.leafs.len()
    }

    pub fn num_planes(&self) -> usize {
        self.planes.len()
    }

    /// Get a specific model.
    pub fn model(&self, index: usize) -> Option<&CModel> {
        self.models.get(index)
    }

    /// Get leaf contents by leaf index.
    pub fn leaf_contents(&self, leaf_idx: usize) -> i32 {
        self.leafs.get(leaf_idx).map_or(0, |l| l.contents)
    }

    /// Get leaf cluster by leaf index.
    pub fn leaf_cluster(&self, leaf_idx: usize) -> i16 {
        self.leafs.get(leaf_idx).map_or(-1, |l| l.cluster)
    }

    /// Get leaf area by leaf index.
    pub fn leaf_area(&self, leaf_idx: usize) -> i16 {
        self.leafs.get(leaf_idx).map_or(0, |l| l.area)
    }

    /// Get number of inline models.
    pub fn num_inline_models(&self) -> usize {
        self.models.len()
    }

    /// Check if a headnode's subtree has any leaf with a potentially visible cluster.
    pub fn headnode_visible(&self, nodenum: i32, visbits: &[u8]) -> bool {
        if nodenum < 0 {
            let leaf_idx = (-1 - nodenum) as usize;
            if leaf_idx >= self.leafs.len() {
                return false;
            }
            let cluster = self.leafs[leaf_idx].cluster as i32;
            if cluster == -1 {
                return false;
            }
            let byte_idx = (cluster >> 3) as usize;
            let bit = 1u8 << (cluster & 7);
            if byte_idx < visbits.len() {
                return visbits[byte_idx] & bit != 0;
            }
            return false;
        }

        if (nodenum as usize) >= self.nodes.len() {
            return false;
        }
        let node = &self.nodes[nodenum as usize];
        if self.headnode_visible(node.children[0], visbits) {
            return true;
        }
        self.headnode_visible(node.children[1], visbits)
    }
}

impl Default for CollisionMap {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MD4 hash — Com_BlockChecksum
// ---------------------------------------------------------------------------

/// Compute the MD4 digest of `data`, returning 4 x u32 state words.
fn md4_digest(data: &[u8]) -> [u32; 4] {
    #[inline(always)]
    fn ff(x: u32, y: u32, z: u32) -> u32 {
        (x & y) | (!x & z)
    }
    #[inline(always)]
    fn gg(x: u32, y: u32, z: u32) -> u32 {
        (x & y) | (x & z) | (y & z)
    }
    #[inline(always)]
    fn hh(x: u32, y: u32, z: u32) -> u32 {
        x ^ y ^ z
    }

    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity(data.len() + 72);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());

    // MD4 initial state words (RFC 1320 §3.3)
    const MD4_INIT_A: u32 = 0x6745_2301;
    const MD4_INIT_B: u32 = 0xefcd_ab89;
    const MD4_INIT_C: u32 = 0x98ba_dcfe;
    const MD4_INIT_D: u32 = 0x1032_5476;
    let mut state: [u32; 4] = [MD4_INIT_A, MD4_INIT_B, MD4_INIT_C, MD4_INIT_D];

    for block in msg.chunks_exact(64) {
        let mut x = [0u32; 16];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            x[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        let (mut a, mut b, mut c, mut d) = (state[0], state[1], state[2], state[3]);

        // Round 1 (F)
        const R1: [usize; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        const S1: [u32; 16] = [3, 7, 11, 19, 3, 7, 11, 19, 3, 7, 11, 19, 3, 7, 11, 19];
        for i in 0..16 {
            let v = a.wrapping_add(ff(b, c, d)).wrapping_add(x[R1[i]]);
            a = v.rotate_left(S1[i]);
            let t = d;
            d = c;
            c = b;
            b = a;
            a = t;
        }

        // Round 2 (G)
        const R2: [usize; 16] = [0, 4, 8, 12, 1, 5, 9, 13, 2, 6, 10, 14, 3, 7, 11, 15];
        const S2: [u32; 16] = [3, 5, 9, 13, 3, 5, 9, 13, 3, 5, 9, 13, 3, 5, 9, 13];
        for i in 0..16 {
            let v = a
                .wrapping_add(gg(b, c, d))
                .wrapping_add(x[R2[i]])
                .wrapping_add(0x5a82_7999);
            a = v.rotate_left(S2[i]);
            let t = d;
            d = c;
            c = b;
            b = a;
            a = t;
        }

        // Round 3 (H)
        const R3: [usize; 16] = [0, 8, 4, 12, 2, 10, 6, 14, 1, 9, 5, 13, 3, 11, 7, 15];
        const S3: [u32; 16] = [3, 9, 11, 15, 3, 9, 11, 15, 3, 9, 11, 15, 3, 9, 11, 15];
        for i in 0..16 {
            let v = a
                .wrapping_add(hh(b, c, d))
                .wrapping_add(x[R3[i]])
                .wrapping_add(0x6ed9_eba1);
            a = v.rotate_left(S3[i]);
            let t = d;
            d = c;
            c = b;
            b = a;
            a = t;
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }

    state
}

/// `Com_BlockChecksum`: MD4 hash of `data`, then XOR the four 32-bit digest
/// words into a single u32.
fn com_block_checksum(data: &[u8]) -> u32 {
    let digest = md4_digest(data);
    digest[0] ^ digest[1] ^ digest[2] ^ digest[3]
}

// ---------------------------------------------------------------------------
// Helper: compute sign_bits for a plane normal
// ---------------------------------------------------------------------------

/// Compute sign_bits for a plane normal:
/// `(n.x < 0) as u8 | ((n.y < 0) as u8) << 1 | ((n.z < 0) as u8) << 2`
pub fn sign_bits_for_plane(normal: Vec3f) -> u8 {
    let mut bits: u8 = 0;
    if normal.x < 0.0 {
        bits |= 1;
    }
    if normal.y < 0.0 {
        bits |= 2;
    }
    if normal.z < 0.0 {
        bits |= 4;
    }
    bits
}

use crate::binary::{try_read_f32, try_read_i16, try_read_i32, try_read_u16};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::binary::try_read_u32;
    use crate::bsp_file::{BSPVERSION, IDBSPHEADER};

    #[test]
    fn collision_map_new() {
        let cm = CollisionMap::new();
        assert!(cm.planes.is_empty());
        assert!(cm.nodes.is_empty());
        assert_eq!(cm.leafs.len(), 1); // default leaf 0
        assert!(cm.brushes.is_empty());
        assert!(cm.models.is_empty());
        assert_eq!(cm.check_count, 0);
        assert_eq!(cm.areas.len(), 1); // area 0 unused sentinel
        assert_eq!(cm.portal_open.len(), MAX_MAP_AREAPORTALS);
    }

    #[test]
    fn empty_map_trace() {
        let mut cm = CollisionMap::new();
        let start = Vec3f::new(0.0, 0.0, 0.0);
        let end = Vec3f::new(100.0, 0.0, 0.0);
        let mins = Vec3f::ZERO;
        let maxs = Vec3f::ZERO;

        let trace = cm.box_trace(start, end, mins, maxs, 0, 0);
        assert_eq!(trace.fraction, 1.0);
        assert!(!trace.allsolid);
        assert!(!trace.startsolid);
        assert_eq!(trace.endpos, end);
    }

    #[test]
    fn point_contents_empty() {
        let cm = CollisionMap::new();
        let contents = cm.point_contents(Vec3f::new(10.0, 20.0, 30.0), 0);
        assert_eq!(contents, 0);
    }

    #[test]
    fn plane_sign_bits() {
        // All positive
        assert_eq!(sign_bits_for_plane(Vec3f::new(1.0, 0.0, 0.0)), 0);
        // X negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(-1.0, 0.0, 0.0)), 1);
        // Y negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(0.0, -1.0, 0.0)), 2);
        // X and Y negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(-1.0, -1.0, 0.0)), 3);
        // Z negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(0.0, 0.0, -1.0)), 4);
        // X and Z negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(-1.0, 0.0, -1.0)), 5);
        // Y and Z negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(0.0, -1.0, -1.0)), 6);
        // All negative
        assert_eq!(sign_bits_for_plane(Vec3f::new(-1.0, -1.0, -1.0)), 7);
    }

    #[test]
    fn trace_default() {
        let t = Trace::default();
        assert_eq!(t.fraction, 1.0);
        assert!(!t.allsolid);
        assert!(!t.startsolid);
        assert_eq!(t.endpos, Vec3f::ZERO);
        assert_eq!(t.contents, 0);
    }

    #[test]
    fn dist_epsilon_value() {
        // CRITICAL: must be exactly 1/32
        assert_eq!(DIST_EPSILON, 1.0 / 32.0);
        assert_eq!(DIST_EPSILON, 0.03125);
    }

    #[test]
    fn bsp_magic_and_version() {
        assert_eq!(IDBSPHEADER, 0x50534249);
        assert_eq!(BSPVERSION, 38);
    }

    #[test]
    fn cplane_default() {
        let p = CPlane::default();
        assert_eq!(p.normal, Vec3f::ZERO);
        assert_eq!(p.dist, 0.0);
        assert_eq!(p.plane_type, 0);
        assert_eq!(p.sign_bits, 0);
    }

    #[test]
    fn collision_map_default_trait() {
        let cm = CollisionMap::default();
        assert!(cm.nodes.is_empty());
        assert_eq!(cm.leafs.len(), 1);
    }

    #[test]
    fn box_on_plane_side_axial() {
        // X-axis plane at dist=5
        let plane = CPlane {
            normal: Vec3f::new(1.0, 0.0, 0.0),
            dist: 5.0,
            plane_type: 0,
            sign_bits: 0,
        };
        let mins = Vec3f::new(6.0, 0.0, 0.0);
        let maxs = Vec3f::new(10.0, 1.0, 1.0);
        // Box is entirely in front (maxs.x=10 > 5, mins.x=6 > 5)
        assert_eq!(CollisionMap::box_on_plane_side(&mins, &maxs, &plane), 1);

        let mins2 = Vec3f::new(0.0, 0.0, 0.0);
        let maxs2 = Vec3f::new(3.0, 1.0, 1.0);
        // Box is entirely behind (maxs.x=3 < 5)
        assert_eq!(CollisionMap::box_on_plane_side(&mins2, &maxs2, &plane), 2);

        let mins3 = Vec3f::new(3.0, 0.0, 0.0);
        let maxs3 = Vec3f::new(7.0, 1.0, 1.0);
        // Box crosses plane (mins.x=3 < 5 < maxs.x=7)
        assert_eq!(CollisionMap::box_on_plane_side(&mins3, &maxs3, &plane), 3);
    }

    #[test]
    fn empty_map_position_test() {
        // Trace with start == end (position test)
        let mut cm = CollisionMap::new();
        let pos = Vec3f::new(50.0, 50.0, 50.0);
        let trace = cm.box_trace(pos, pos, Vec3f::ZERO, Vec3f::ZERO, 0, 0);
        assert_eq!(trace.fraction, 1.0);
        assert!(!trace.allsolid);
        assert_eq!(trace.endpos, pos);
    }

    #[test]
    fn angle_vectors_identity() {
        // Zero angles should give forward=(1,0,0), right=(0,-1,0), up=(0,0,1)
        let (fwd, right, up) = angle_vectors(Vec3f::ZERO);
        assert!((fwd.x - 1.0).abs() < 1e-6);
        assert!(fwd.y.abs() < 1e-6);
        assert!(fwd.z.abs() < 1e-6);
        assert!(right.x.abs() < 1e-6);
        assert!((right.y - (-1.0)).abs() < 1e-6);
        assert!(right.z.abs() < 1e-6);
        assert!(up.x.abs() < 1e-6);
        assert!(up.y.abs() < 1e-6);
        assert!((up.z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn read_helpers() {
        let data: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        assert_eq!(try_read_u16(&data, 0).unwrap(), 0x0201);
        assert_eq!(try_read_u32(&data, 0).unwrap(), 0x04030201);
        assert_eq!(try_read_i32(&data, 0).unwrap(), 0x04030201);
    }

    // -----------------------------------------------------------------------
    // Minimal BSP builder for collision tests
    // -----------------------------------------------------------------------

    /// Build a minimal valid BSP binary in memory.
    ///
    /// Geometry: a solid brush covering x ∈ [-500, 0], y/z ∈ [-500, 500].
    /// One BSP node splits at x=0: front (x>0) → empty leaf, back (x<0) → solid leaf.
    pub(crate) fn build_minimal_bsp() -> Vec<u8> {
        let mut buf = vec![0u8; 600];
        let mut cursor = 0usize;

        // Helper: write little-endian values into buf
        macro_rules! w_u32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as u32).to_le_bytes());
                cursor += 4;
            };
        }
        macro_rules! w_i32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as i32).to_le_bytes());
                cursor += 4;
            };
        }
        macro_rules! w_i16 {
            ($v:expr) => {
                buf[cursor..cursor + 2].copy_from_slice(&($v as i16).to_le_bytes());
                cursor += 2;
            };
        }
        macro_rules! w_u16 {
            ($v:expr) => {
                buf[cursor..cursor + 2].copy_from_slice(&($v as u16).to_le_bytes());
                cursor += 2;
            };
        }
        macro_rules! w_f32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as f32).to_le_bytes());
                cursor += 4;
            };
        }

        // ---- Header ----
        w_u32!(IDBSPHEADER);
        w_u32!(BSPVERSION);

        // ---- Lump directory (19 entries × 8 bytes) ----
        // We'll fill these after writing lump data.
        let lump_dir_start = cursor;
        cursor += 19 * 8;
        let _data_start = cursor; // 160

        // Track lump positions
        let mut lumps = [(0u32, 0u32); 19];

        // ---- Lump 5: Texinfo (1 entry × 76 bytes) ----
        lumps[5] = (cursor as u32, 76);
        // vecs[2][4] = 32 bytes of zeros, flags=0, value=0
        cursor += 32; // vecs
        w_i32!(0); // flags
        w_i32!(0); // value
                   // texture name "solid" (32 bytes)
        let name = b"solid\0";
        buf[cursor..cursor + name.len()].copy_from_slice(name);
        cursor += 32;
        w_i32!(-1); // nexttexinfo

        // ---- Lump 8: Leafs (2 entries × 28 bytes) ----
        lumps[8] = (cursor as u32, 56);
        // Leaf 0: SOLID
        w_i32!(CONTENTS_SOLID); // contents
        w_i16!(0i16); // cluster
        w_i16!(1i16); // area
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // mins
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // maxs
        w_u16!(0u16); // firstleafface
        w_u16!(0u16); // numleaffaces
        w_u16!(0u16); // firstleafbrush
        w_u16!(1u16); // numleafbrushes

        // Leaf 1: EMPTY
        w_i32!(0); // contents
        w_i16!(0i16); // cluster
        w_i16!(1i16); // area
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // mins
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // maxs
        w_u16!(0u16);
        w_u16!(0u16); // faces
        w_u16!(0u16); // firstleafbrush
        w_u16!(0u16); // numleafbrushes

        // ---- Lump 10: Leaf brushes (1 entry × 2 bytes) ----
        lumps[10] = (cursor as u32, 2);
        w_u16!(0u16); // brush index 0

        // ---- Lump 1: Planes (6 entries × 20 bytes) ----
        lumps[1] = (cursor as u32, 120);

        // Plane 0: +X at dist=0 (BSP split + brush +X face)
        w_f32!(1.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_i32!(0);

        // Plane 1: -X at dist=500 (brush -X face: x ≥ -500)
        w_f32!(-1.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(3);

        // Plane 2: +Y at dist=500 (brush +Y face)
        w_f32!(0.0f32);
        w_f32!(1.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(1);

        // Plane 3: -Y at dist=500 (brush -Y face: y ≥ -500)
        w_f32!(0.0f32);
        w_f32!(-1.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(4);

        // Plane 4: +Z at dist=500 (brush +Z face)
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(1.0f32);
        w_f32!(500.0f32);
        w_i32!(2);

        // Plane 5: -Z at dist=500 (brush -Z face: z ≥ -500)
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(-1.0f32);
        w_f32!(500.0f32);
        w_i32!(5);

        // ---- Lump 14: Brushes (1 entry × 12 bytes) ----
        lumps[14] = (cursor as u32, 12);
        w_i32!(0); // first_brush_side
        w_i32!(6); // num_sides
        w_i32!(CONTENTS_SOLID); // contents

        // ---- Lump 15: Brush sides (6 entries × 4 bytes) ----
        lumps[15] = (cursor as u32, 24);
        for plane_idx in 0..6u16 {
            w_u16!(plane_idx); // plane index
            w_i16!(0i16); // texinfo index 0
        }

        // ---- Lump 13: Models (1 entry × 48 bytes) ----
        lumps[13] = (cursor as u32, 48);
        // mins
        w_f32!(-500.0f32);
        w_f32!(-500.0f32);
        w_f32!(-500.0f32);
        // maxs
        w_f32!(500.0f32);
        w_f32!(500.0f32);
        w_f32!(500.0f32);
        // origin
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        // headnode, firstface, numfaces
        w_i32!(0);
        w_i32!(0);
        w_i32!(0);

        // ---- Lump 4: Nodes (1 entry × 28 bytes) ----
        lumps[4] = (cursor as u32, 28);
        w_i32!(0); // planenum (plane 0: X at 0)
        w_i32!(-2); // child[0]: front → leaf 1 (empty), encoded as -(1+1)
        w_i32!(-1); // child[1]: back → leaf 0 (solid), encoded as -(1+0)
                    // mins/maxs/face info (ignored by collision loader)
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_u16!(0u16);
        w_u16!(0u16);

        // ---- Lump 17: Areas (2 entries × 8 bytes) ----
        lumps[17] = (cursor as u32, 16);
        // Area 0 (sentinel)
        w_i32!(0);
        w_i32!(0);
        // Area 1
        w_i32!(0);
        w_i32!(0);

        // ---- Lump 18: Area portals (0 entries) ----
        lumps[18] = (cursor as u32, 0);

        // Truncate buffer to actual size
        buf.truncate(cursor);

        // ---- Write lump directory ----
        let mut lc = lump_dir_start;
        for (ofs, len) in lumps {
            buf[lc..lc + 4].copy_from_slice(&ofs.to_le_bytes());
            buf[lc + 4..lc + 8].copy_from_slice(&len.to_le_bytes());
            lc += 8;
        }

        buf
    }

    #[test]
    fn load_minimal_bsp() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        let result = cm.load_map(&bsp);
        assert!(result.is_ok(), "load_map failed: {:?}", result.err());
        assert_eq!(cm.num_nodes(), 1 + 6); // 1 BSP node + 6 box hull nodes
        assert_eq!(cm.num_planes(), 6 + 12); // 6 BSP planes + 12 box hull planes
        assert!(cm.num_brushes() >= 1);
        assert!(cm.num_leafs() >= 2);
    }

    #[test]
    fn bsp_point_contents_solid() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Point in solid region (x < 0)
        let contents = cm.point_contents(Vec3f::new(-50.0, 0.0, 0.0), 0);
        assert_eq!(contents, CONTENTS_SOLID, "point at x=-50 should be solid");
    }

    #[test]
    fn bsp_point_contents_empty() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Point in empty region (x > 0)
        let contents = cm.point_contents(Vec3f::new(50.0, 0.0, 0.0), 0);
        assert_eq!(contents, 0, "point at x=50 should be empty");
    }

    #[test]
    fn bsp_trace_hits_solid() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Trace from empty region into solid region
        let start = Vec3f::new(50.0, 0.0, 0.0);
        let end = Vec3f::new(-50.0, 0.0, 0.0);
        let trace = cm.box_trace(start, end, Vec3f::ZERO, Vec3f::ZERO, 0, CONTENTS_SOLID);

        assert!(
            trace.fraction < 1.0,
            "trace should hit solid, got fraction={}",
            trace.fraction
        );
        assert_eq!(trace.contents, CONTENTS_SOLID);
        // Hit point should be near x=0 (the solid boundary)
        assert!(
            trace.endpos.x > -1.0,
            "hit point x={} should be near 0",
            trace.endpos.x
        );
        assert!(
            trace.endpos.x < 2.0,
            "hit point x={} should be near 0",
            trace.endpos.x
        );
    }

    #[test]
    fn bsp_trace_through_empty() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Trace entirely within empty region — should not hit anything
        let start = Vec3f::new(10.0, 0.0, 0.0);
        let end = Vec3f::new(100.0, 0.0, 0.0);
        let trace = cm.box_trace(start, end, Vec3f::ZERO, Vec3f::ZERO, 0, CONTENTS_SOLID);

        assert_eq!(trace.fraction, 1.0);
        assert!(!trace.allsolid);
        assert!(!trace.startsolid);
        assert_eq!(trace.endpos, end);
    }

    #[test]
    fn bsp_trace_starts_in_solid() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Trace starting inside solid
        let start = Vec3f::new(-50.0, 0.0, 0.0);
        let end = Vec3f::new(-100.0, 0.0, 0.0);
        let trace = cm.box_trace(start, end, Vec3f::ZERO, Vec3f::ZERO, 0, CONTENTS_SOLID);

        assert!(trace.startsolid, "trace should start in solid");
        assert!(trace.allsolid, "trace should be entirely in solid");
    }

    #[test]
    fn bsp_headnode_for_box_trace() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Create a box entity at origin with extents [-16, -16, -16] to [16, 16, 16]
        let head = cm.headnode_for_box(
            Vec3f::new(-16.0, -16.0, -16.0),
            Vec3f::new(16.0, 16.0, 16.0),
        );

        // Trace a ray from (100,0,0) toward (0,0,0) — should hit the box
        let start = Vec3f::new(100.0, 0.0, 0.0);
        let end = Vec3f::new(0.0, 0.0, 0.0);
        let trace = cm.box_trace(start, end, Vec3f::ZERO, Vec3f::ZERO, head, CONTENTS_MONSTER);

        assert!(trace.fraction < 1.0, "should hit the box entity");
        // Hit point should be near x=16 (box +X face)
        assert!(
            trace.endpos.x > 14.0 && trace.endpos.x < 18.0,
            "hit at x={}, expected near 16",
            trace.endpos.x
        );
    }

    #[test]
    fn md4_empty_digest() {
        // MD4("") = 31d6cfe0 d16ae931 b73c59d7 e0c089c0 (hex bytes)
        // In LE u32 words: state[0]=0xe0cfd631, state[1]=0x31e96ad1,
        //                   state[2]=0xd7593cb7, state[3]=0xc089c0e0
        let digest = md4_digest(b"");
        assert_eq!(digest[0], 0xe0cf_d631);
        assert_eq!(digest[1], 0x31e9_6ad1);
        assert_eq!(digest[2], 0xd759_3cb7);
        assert_eq!(digest[3], 0xc089_c0e0);
    }

    #[test]
    fn com_block_checksum_empty() {
        // XOR of the four MD4("") digest words.
        let expected = 0xe0cf_d631_u32 ^ 0x31e9_6ad1 ^ 0xd759_3cb7 ^ 0xc089_c0e0;
        assert_eq!(expected, 0xc6f6_40b7);
        assert_eq!(com_block_checksum(b""), expected);
    }

    // -----------------------------------------------------------------------
    // Floor BSP builder — solid slab below z=0
    // -----------------------------------------------------------------------

    /// Build a BSP with a floor at z=0.
    ///
    /// Geometry: solid brush covering x/y ∈ [-500, 500], z ∈ [-500, 0].
    /// BSP node splits at z=0: front (z>0) → empty, back (z<0) → solid.
    pub(crate) fn build_floor_bsp() -> Vec<u8> {
        let mut buf = vec![0u8; 600];
        let mut cursor = 0usize;

        macro_rules! w_u32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as u32).to_le_bytes());
                cursor += 4;
            };
        }
        macro_rules! w_i32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as i32).to_le_bytes());
                cursor += 4;
            };
        }
        macro_rules! w_i16 {
            ($v:expr) => {
                buf[cursor..cursor + 2].copy_from_slice(&($v as i16).to_le_bytes());
                cursor += 2;
            };
        }
        macro_rules! w_u16 {
            ($v:expr) => {
                buf[cursor..cursor + 2].copy_from_slice(&($v as u16).to_le_bytes());
                cursor += 2;
            };
        }
        macro_rules! w_f32 {
            ($v:expr) => {
                buf[cursor..cursor + 4].copy_from_slice(&($v as f32).to_le_bytes());
                cursor += 4;
            };
        }

        w_u32!(IDBSPHEADER);
        w_u32!(BSPVERSION);

        let lump_dir_start = cursor;
        cursor += 19 * 8;
        let mut lumps = [(0u32, 0u32); 19];

        // Texinfo (1 × 76 bytes)
        lumps[5] = (cursor as u32, 76);
        cursor += 32; // vecs
        w_i32!(0);
        w_i32!(0); // flags, value
        let name = b"solid\0";
        buf[cursor..cursor + name.len()].copy_from_slice(name);
        cursor += 32; // texture name
        w_i32!(-1); // nexttexinfo

        // Leafs (2 × 28 bytes)
        lumps[8] = (cursor as u32, 56);
        // Leaf 0: SOLID
        w_i32!(CONTENTS_SOLID);
        w_i16!(0i16);
        w_i16!(1i16); // cluster, area
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // mins
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16); // maxs
        w_u16!(0u16);
        w_u16!(0u16); // faces
        w_u16!(0u16);
        w_u16!(1u16); // firstleafbrush, numleafbrushes
                      // Leaf 1: EMPTY
        w_i32!(0);
        w_i16!(0i16);
        w_i16!(1i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_u16!(0u16);
        w_u16!(0u16);
        w_u16!(0u16);
        w_u16!(0u16);

        // Leaf brushes (1 × 2 bytes)
        lumps[10] = (cursor as u32, 2);
        w_u16!(0u16);

        // Planes (6 × 20 bytes) — floor slab
        lumps[1] = (cursor as u32, 120);
        // Plane 0: +Z at dist=0 (floor surface, split plane)
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(1.0f32);
        w_f32!(0.0f32);
        w_i32!(2);
        // Plane 1: -Z at dist=500
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(-1.0f32);
        w_f32!(500.0f32);
        w_i32!(5);
        // Plane 2: +X at dist=500
        w_f32!(1.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(0);
        // Plane 3: -X at dist=500
        w_f32!(-1.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(3);
        // Plane 4: +Y at dist=500
        w_f32!(0.0f32);
        w_f32!(1.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(1);
        // Plane 5: -Y at dist=500
        w_f32!(0.0f32);
        w_f32!(-1.0f32);
        w_f32!(0.0f32);
        w_f32!(500.0f32);
        w_i32!(4);

        // Brush (1 × 12 bytes)
        lumps[14] = (cursor as u32, 12);
        w_i32!(0);
        w_i32!(6);
        w_i32!(CONTENTS_SOLID);

        // Brush sides (6 × 4 bytes)
        lumps[15] = (cursor as u32, 24);
        for plane_idx in 0..6u16 {
            w_u16!(plane_idx);
            w_i16!(0i16);
        }

        // Model (1 × 48 bytes)
        lumps[13] = (cursor as u32, 48);
        w_f32!(-500.0f32);
        w_f32!(-500.0f32);
        w_f32!(-500.0f32); // mins
        w_f32!(500.0f32);
        w_f32!(500.0f32);
        w_f32!(0.0f32); // maxs
        w_f32!(0.0f32);
        w_f32!(0.0f32);
        w_f32!(0.0f32); // origin
        w_i32!(0);
        w_i32!(0);
        w_i32!(0); // headnode, firstface, numfaces

        // Node (1 × 28 bytes): split at z=0
        lumps[4] = (cursor as u32, 28);
        w_i32!(0); // planenum (plane 0: +Z at z=0)
        w_i32!(-2); // child[0]: front (z>0) → leaf 1 (empty)
        w_i32!(-1); // child[1]: back (z<0) → leaf 0 (solid)
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_i16!(0i16);
        w_u16!(0u16);
        w_u16!(0u16);

        // Areas (2 × 8 bytes)
        lumps[17] = (cursor as u32, 16);
        w_i32!(0);
        w_i32!(0);
        w_i32!(0);
        w_i32!(0);

        // Area portals (0 entries)
        lumps[18] = (cursor as u32, 0);

        buf.truncate(cursor);

        let mut lc = lump_dir_start;
        for (ofs, len) in lumps {
            buf[lc..lc + 4].copy_from_slice(&ofs.to_le_bytes());
            buf[lc + 4..lc + 8].copy_from_slice(&len.to_le_bytes());
            lc += 8;
        }
        buf
    }

    // -----------------------------------------------------------------------
    // Box-sweep trace (player-sized bounding box)
    // -----------------------------------------------------------------------

    #[test]
    fn bsp_box_sweep_hits_wall() {
        let bsp = build_minimal_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Player-sized box from (50,0,0) toward (-50,0,0).
        // The wall is at x=0; with player half-width 16, the box should stop at x≈16.
        let player_mins = Vec3f::new(-16.0, -16.0, -24.0);
        let player_maxs = Vec3f::new(16.0, 16.0, 32.0);
        let trace = cm.box_trace(
            Vec3f::new(50.0, 0.0, 0.0),
            Vec3f::new(-50.0, 0.0, 0.0),
            player_mins,
            player_maxs,
            0,
            CONTENTS_SOLID,
        );

        assert!(trace.fraction < 1.0, "box sweep should hit the wall");
        // endpos.x should be near 16 (player center when -16 edge touches x=0)
        assert!(
            (trace.endpos.x - 16.0).abs() < 1.0,
            "expected stop near x=16, got x={}",
            trace.endpos.x
        );
    }

    #[test]
    fn bsp_box_sweep_floor_stops_fall() {
        let bsp = build_floor_bsp();
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        // Drop a player-sized box from z=100 to z=-100. Floor at z=0.
        // Player mins.z = -24, so box bottom at z-24. Should stop when z-24 = 0 → z = 24.
        let player_mins = Vec3f::new(-16.0, -16.0, -24.0);
        let player_maxs = Vec3f::new(16.0, 16.0, 32.0);
        let trace = cm.box_trace(
            Vec3f::new(0.0, 0.0, 100.0),
            Vec3f::new(0.0, 0.0, -100.0),
            player_mins,
            player_maxs,
            0,
            CONTENTS_SOLID,
        );

        assert!(trace.fraction < 1.0, "box should hit the floor");
        assert!(
            (trace.endpos.z - 24.0).abs() < 1.0,
            "expected stop near z=24, got z={}",
            trace.endpos.z
        );
    }

    // -----------------------------------------------------------------------
    // BSP loader error paths
    // -----------------------------------------------------------------------

    #[test]
    fn bsp_wrong_version_rejected() {
        let mut bsp = build_minimal_bsp();
        // Overwrite version (bytes 4..8) with wrong value 99
        bsp[4..8].copy_from_slice(&99u32.to_le_bytes());
        let mut cm = CollisionMap::new();
        let result = cm.load_map(&bsp);
        assert!(result.is_err(), "wrong version should be rejected");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("wrong version"), "error: {err}");
    }

    #[test]
    fn bsp_wrong_magic_rejected() {
        let mut bsp = build_minimal_bsp();
        // Overwrite magic (bytes 0..4) with garbage
        bsp[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        let mut cm = CollisionMap::new();
        let result = cm.load_map(&bsp);
        assert!(result.is_err(), "wrong magic should be rejected");
    }

    // -----------------------------------------------------------------------
    // Malformed-BSP helpers: patch one field of a valid test BSP
    // -----------------------------------------------------------------------

    /// (offset, length) of lump `idx` from the BSP header.
    fn lump(bsp: &[u8], idx: usize) -> (usize, usize) {
        let at = 8 + idx * 8;
        (
            try_read_u32(bsp, at).unwrap() as usize,
            try_read_u32(bsp, at + 4).unwrap() as usize,
        )
    }

    fn put_i32(bsp: &mut [u8], at: usize, v: i32) {
        bsp[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn put_u16(bsp: &mut [u8], at: usize, v: u16) {
        bsp[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// Overwrite the i32 at `rel` bytes into lump `idx`.
    fn patch_i32(bsp: &mut [u8], idx: usize, rel: usize, v: i32) {
        let at = lump(bsp, idx).0 + rel;
        put_i32(bsp, at, v);
    }

    /// Overwrite the u16 at `rel` bytes into lump `idx`.
    fn patch_u16(bsp: &mut [u8], idx: usize, rel: usize, v: u16) {
        let at = lump(bsp, idx).0 + rel;
        put_u16(bsp, at, v);
    }

    /// Append `bytes` and point lump `idx` at them.
    fn with_lump(mut bsp: Vec<u8>, idx: usize, bytes: &[u8]) -> Vec<u8> {
        let ofs = bsp.len() as u32;
        bsp.extend_from_slice(bytes);
        let at = 8 + idx * 8;
        bsp[at..at + 4].copy_from_slice(&ofs.to_le_bytes());
        bsp[at + 4..at + 8].copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        bsp
    }

    /// Build a BSP whose only solid is a ceiling: everything above
    /// `height` is solid, everything below is empty. Derived from the floor
    /// BSP by moving the split plane and flipping which side is solid.
    pub(crate) fn build_ceiling_bsp(height: f32) -> Vec<u8> {
        let mut bsp = build_floor_bsp();
        let planes = lump(&bsp, LUMP_PLANES).0;
        // Plane 0 (+Z) becomes the split at `height`; plane 1 (-Z) becomes
        // the brush's bottom face, z >= height.
        bsp[planes + 12..planes + 16].copy_from_slice(&height.to_le_bytes());
        let bottom = planes + DPLANE_SIZE + 12;
        bsp[bottom..bottom + 4].copy_from_slice(&(-height).to_le_bytes());
        // Brush uses sides 1..6 (no top face: unbounded upward).
        patch_i32(&mut bsp, LUMP_BRUSHES, 0, 1);
        patch_i32(&mut bsp, LUMP_BRUSHES, 4, 5);
        // Front of the split (above) is the solid leaf 0, back is empty.
        patch_i32(&mut bsp, LUMP_NODES, 4, -1);
        patch_i32(&mut bsp, LUMP_NODES, 8, -2);
        bsp
    }

    #[test]
    fn ceiling_bsp_is_solid_above_height_only() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_ceiling_bsp(100.0)).unwrap();
        assert_eq!(
            cm.point_contents(Vec3f::new(0.0, 0.0, 150.0), 0),
            CONTENTS_SOLID
        );
        assert_eq!(cm.point_contents(Vec3f::new(0.0, 0.0, 50.0), 0), 0);
        let t = cm.box_trace(
            Vec3f::new(0.0, 0.0, 50.0),
            Vec3f::new(0.0, 0.0, 150.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            0,
            CONTENTS_SOLID,
        );
        assert!((t.endpos.z - 100.0).abs() < 0.1, "endpos {:?}", t.endpos);
    }

    /// Patch the minimal BSP with `patch` and assert load_map rejects it.
    fn assert_rejected(what: &str, patch: impl FnOnce(&mut Vec<u8>)) {
        let mut bsp = build_minimal_bsp();
        patch(&mut bsp);
        let mut cm = CollisionMap::new();
        let result = cm.load_map(&bsp);
        assert!(
            matches!(result, Err(Q2Error::Drop(_))),
            "{what}: expected Err(Drop), got {result:?}"
        );
    }

    #[test]
    fn load_map_truncated_texinfo_returns_err() {
        // 160-byte header + texinfo lump {ofs 160, len 76} but only 50 bytes.
        let mut bsp = build_minimal_bsp();
        bsp.truncate(160);
        let at = 8 + LUMP_TEXINFO * 8;
        bsp[at..at + 4].copy_from_slice(&160u32.to_le_bytes());
        bsp[at + 4..at + 8].copy_from_slice(&76u32.to_le_bytes());
        bsp.extend_from_slice(&[0u8; 50]);
        let mut cm = CollisionMap::new();
        assert!(matches!(cm.load_map(&bsp), Err(Q2Error::Drop(_))));
    }

    #[test]
    fn load_map_rejects_lump_past_end_of_file() {
        assert_rejected("areas lump past EOF", |bsp| {
            let at = 8 + LUMP_AREAS * 8;
            let len = bsp.len() as u32;
            bsp[at..at + 4].copy_from_slice(&len.to_le_bytes());
            bsp[at + 4..at + 8].copy_from_slice(&8u32.to_le_bytes());
        });
    }

    #[test]
    fn load_map_rejects_bad_node_references() {
        assert_rejected("plane index -1", |b| patch_i32(b, LUMP_NODES, 0, -1));
        assert_rejected("plane index past end", |b| patch_i32(b, LUMP_NODES, 0, 6));
        assert_rejected("child node 9999", |b| patch_i32(b, LUMP_NODES, 4, 9999));
        assert_rejected("child node is itself", |b| patch_i32(b, LUMP_NODES, 4, 0));
        assert_rejected("child leaf past end", |b| patch_i32(b, LUMP_NODES, 8, -100));
    }

    #[test]
    fn load_map_rejects_bad_brush_ranges() {
        assert_rejected("negative num_sides", |b| patch_i32(b, LUMP_BRUSHES, 4, -1));
        assert_rejected("negative first side", |b| patch_i32(b, LUMP_BRUSHES, 0, -1));
        assert_rejected("sides past end", |b| patch_i32(b, LUMP_BRUSHES, 4, 7));
        assert_rejected("brushside plane past end", |b| {
            patch_u16(b, LUMP_BRUSHSIDES, 0, 99)
        });
        assert_rejected("bad brushside texinfo", |b| {
            patch_u16(b, LUMP_BRUSHSIDES, 2, 5)
        });
    }

    #[test]
    fn load_map_rejects_bad_leaf_brush_references() {
        assert_rejected("leaf brush range past end", |b| {
            patch_u16(b, LUMP_LEAFS, 26, 2)
        });
        assert_rejected("leafbrush names missing brush", |b| {
            patch_u16(b, LUMP_LEAFBRUSHES, 0, 1)
        });
        assert_rejected("model headnode past end", |b| {
            patch_i32(b, LUMP_MODELS, 36, 5)
        });
        assert_rejected("model headleaf past end", |b| {
            patch_i32(b, LUMP_MODELS, 36, -9)
        });
    }

    #[test]
    fn load_map_rejects_loader_error_paths() {
        assert_rejected("funny plane lump size", |b| {
            let at = 8 + LUMP_PLANES * 8 + 4;
            put_i32(b, at, 121);
        });
        assert_rejected("no leafs", |b| put_i32(b, 8 + LUMP_LEAFS * 8 + 4, 0));
        assert_rejected("leaf 0 not solid", |b| patch_i32(b, LUMP_LEAFS, 0, 0));
        assert_rejected("no empty leaf", |b| patch_i32(b, LUMP_LEAFS, 28, 1));
    }

    #[test]
    fn load_map_rejects_when_no_room_for_box_tree() {
        // MAX_MAP_LEAFBRUSHES entries leave no slot for the box leaf brush;
        // C errors "Not enough room for box tree" instead of truncating.
        let bsp = with_lump(
            build_minimal_bsp(),
            LUMP_LEAFBRUSHES,
            &vec![0u8; MAX_MAP_LEAFBRUSHES * 2],
        );
        let mut cm = CollisionMap::new();
        let err = cm.load_map(&bsp).unwrap_err().to_string();
        assert!(err.contains("Not enough room for box tree"), "{err}");
    }

    #[test]
    fn failed_load_leaves_empty_map_not_stale_state() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        let mut bad = build_minimal_bsp();
        let node = lump(&bad, LUMP_NODES).0;
        put_i32(&mut bad, node + 4, 0);
        assert!(cm.load_map(&bad).is_err());
        assert_eq!(cm.num_planes(), 0);
        assert_eq!(cm.num_nodes(), 0);
        let t = cm.box_trace(Vec3f::ZERO, Vec3f::X, Vec3f::ZERO, Vec3f::ZERO, 0, !0);
        assert_eq!(t.fraction, 1.0);
    }

    #[test]
    fn empty_map_box_queries_terminate() {
        // load_map(&[]) used to build a box hull whose node 0 pointed at
        // itself, so point_contents looped forever.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut cm = CollisionMap::new();
            cm.load_map(&[]).unwrap();
            let h = cm.headnode_for_box(Vec3f::splat(-16.0), Vec3f::splat(16.0));
            let contents = cm.point_contents(Vec3f::splat(1000.0), h);
            let trace = cm.box_trace(
                Vec3f::splat(100.0),
                Vec3f::splat(200.0),
                Vec3f::ZERO,
                Vec3f::ZERO,
                h,
                CONTENTS_MONSTER,
            );
            tx.send((contents, trace.fraction)).unwrap();
        });
        let (contents, fraction) = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("empty-map box query did not terminate");
        assert_eq!(contents, 0);
        assert_eq!(fraction, 1.0);
    }

    #[test]
    fn check_count_wraps_like_c_instead_of_panicking() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        cm.check_count = i32::MAX;
        let t = cm.box_trace(
            Vec3f::new(50.0, 0.0, 0.0),
            Vec3f::new(-50.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            0,
            CONTENTS_SOLID,
        );
        assert!(t.fraction < 1.0);
        assert_eq!(cm.check_count, i32::MIN);
    }

    #[test]
    fn box_on_plane_side_axial_ties_follow_c() {
        // C BOX_ON_PLANE_SIDE: dist <= mins -> 1, dist >= maxs -> 2, else 3.
        let plane = CPlane {
            normal: Vec3f::new(0.0, 0.0, 1.0),
            dist: 64.0,
            plane_type: 2,
            sign_bits: 0,
        };
        let touching_back = (Vec3f::new(0.0, 0.0, 0.0), Vec3f::new(1.0, 1.0, 64.0));
        assert_eq!(
            CollisionMap::box_on_plane_side(&touching_back.0, &touching_back.1, &plane),
            2
        );
        let touching_front = (Vec3f::new(0.0, 0.0, 64.0), Vec3f::new(1.0, 1.0, 70.0));
        assert_eq!(
            CollisionMap::box_on_plane_side(&touching_front.0, &touching_front.1, &plane),
            1
        );
    }

    #[test]
    fn box_on_plane_side_non_axial_uses_sign_bits_corners() {
        let n = Vec3f::new(1.0, -1.0, 0.0).normalize();
        let plane = CPlane {
            normal: n,
            dist: 0.0,
            plane_type: 3,
            sign_bits: sign_bits_for_plane(n),
        };
        let front = (Vec3f::new(10.0, -20.0, 0.0), Vec3f::new(20.0, -10.0, 1.0));
        let back = (Vec3f::new(-20.0, 10.0, 0.0), Vec3f::new(-10.0, 20.0, 1.0));
        let cross = (Vec3f::new(-5.0, -5.0, 0.0), Vec3f::new(5.0, 5.0, 1.0));
        assert_eq!(
            CollisionMap::box_on_plane_side(&front.0, &front.1, &plane),
            1
        );
        assert_eq!(CollisionMap::box_on_plane_side(&back.0, &back.1, &plane), 2);
        assert_eq!(
            CollisionMap::box_on_plane_side(&cross.0, &cross.1, &plane),
            3
        );
    }

    #[test]
    fn box_leafnums_respects_max_count() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        let (all, top) = cm.box_leafnums_headnode(Vec3f::splat(-8.0), Vec3f::splat(8.0), 1024, 0);
        assert_eq!(all.len(), 2);
        assert_eq!(top, 0);
        let (capped, _) = cm.box_leafnums_headnode(Vec3f::splat(-8.0), Vec3f::splat(8.0), 1, 0);
        assert_eq!(capped.len(), 1);
    }

    #[test]
    fn transformed_box_trace_rotates_into_model_frame_and_back() {
        // World model rotated 180 degrees about z and moved to x=100: its
        // solid half-space (local x<0) becomes world x>100.
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        let t = cm.transformed_box_trace(
            Vec3f::new(50.0, 0.0, 0.0),
            Vec3f::new(150.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            0,
            CONTENTS_SOLID,
            Vec3f::new(100.0, 0.0, 0.0),
            Vec3f::new(0.0, 180.0, 0.0),
        );
        assert!((t.endpos.x - 100.0).abs() < 1.0, "endpos {:?}", t.endpos);
        assert!(
            (t.plane.normal.x + 1.0).abs() < 1e-5,
            "normal {:?}",
            t.plane.normal
        );
    }

    #[test]
    fn transformed_box_trace_ignores_angles_for_box_hull() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        let h = cm.headnode_for_box(Vec3f::splat(-16.0), Vec3f::splat(16.0));
        let t = cm.transformed_box_trace(
            Vec3f::new(200.0, 0.0, 0.0),
            Vec3f::new(0.0, 0.0, 0.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            h,
            CONTENTS_MONSTER,
            Vec3f::new(100.0, 0.0, 0.0),
            Vec3f::new(0.0, 45.0, 0.0),
        );
        assert!((t.endpos.x - 116.0).abs() < 1.0, "endpos {:?}", t.endpos);
    }

    #[test]
    fn load_map_keeps_entity_string_like_cm_entitystring() {
        let entities = b"{ \"classname\" \"worldspawn\" }\0";
        let mut bsp = build_floor_bsp();
        let ofs = bsp.len() as u32;
        bsp.extend_from_slice(entities);
        put_i32(&mut bsp, 8, ofs as i32);
        put_i32(&mut bsp, 12, entities.len() as i32);
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();
        assert_eq!(cm.entity_string(), "{ \"classname\" \"worldspawn\" }");
    }

    /// Encoded bbox as C SV_LinkEdict packs it: x = 8 * (solid & 31),
    /// zd = 8 * ((solid >> 5) & 31), zu = 8 * ((solid >> 10) & 63) - 32.
    fn encoded_solid(x: i32, zd: i32, zu: i32) -> i32 {
        (x / 8) | ((zd / 8) << 5) | (((zu + 32) / 8) << 10)
    }

    fn clip_ent(number: i32, solid: i32, origin: Vec3f) -> q2_shared::EntityState {
        q2_shared::EntityState {
            number,
            solid,
            origin,
            ..Default::default()
        }
    }

    /// Point trace along +x at z=50, above the floor BSP: misses the world.
    fn clip_trace(cm: &mut CollisionMap, ents: &ClipEntities) -> Trace {
        let (start, end) = (Vec3f::new(0.0, 0.0, 50.0), Vec3f::new(200.0, 0.0, 50.0));
        let mut tr = cm.world_trace(start, Vec3f::ZERO, Vec3f::ZERO, end, MASK_PLAYERSOLID);
        assert_eq!(tr.fraction, 1.0);
        cm.clip_move_to_entities(start, Vec3f::ZERO, Vec3f::ZERO, end, ents, &mut tr);
        tr
    }

    #[test]
    fn clip_move_to_entities_box_entity_blocks_and_is_reported() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        let ents = [clip_ent(
            5,
            encoded_solid(16, 16, 16),
            Vec3f::new(100.0, 0.0, 50.0),
        )];
        let tr = clip_trace(
            &mut cm,
            &ClipEntities {
                entities: &ents,
                player_number: 1,
                model_clip: &[],
            },
        );
        assert!(tr.fraction < 1.0);
        assert!((tr.endpos.x - 84.0).abs() < 0.1, "endpos {:?}", tr.endpos);
        assert_eq!(tr.ent_index, Some(5));
    }

    #[test]
    fn clip_move_to_entities_skips_player_and_non_solid_entities() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        let blocker = encoded_solid(16, 16, 16);
        let ents = [
            clip_ent(1, blocker, Vec3f::new(100.0, 0.0, 50.0)), // the player
            clip_ent(2, 0, Vec3f::new(120.0, 0.0, 50.0)),       // not solid
        ];
        let tr = clip_trace(
            &mut cm,
            &ClipEntities {
                entities: &ents,
                player_number: 1,
                model_clip: &[],
            },
        );
        assert_eq!(tr.fraction, 1.0);
        assert_eq!(tr.ent_index, None);
    }

    #[test]
    fn clip_move_to_entities_uses_model_clip_for_bmodels() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        // solid 31 = bmodel: model index 1 clips with the world hull (head
        // node 0) moved up by 100, so a downward trace stops at z=100.
        let mut door = clip_ent(7, 31, Vec3f::new(0.0, 0.0, 100.0));
        door.modelindex = 1;
        let model_clip = [None, Some(0)];
        let ents = ClipEntities {
            entities: std::slice::from_ref(&door),
            player_number: 1,
            model_clip: &model_clip,
        };
        let (start, end) = (Vec3f::new(0.0, 0.0, 150.0), Vec3f::new(0.0, 0.0, 50.0));
        let mut tr = cm.world_trace(start, Vec3f::ZERO, Vec3f::ZERO, end, MASK_PLAYERSOLID);
        cm.clip_move_to_entities(start, Vec3f::ZERO, Vec3f::ZERO, end, &ents, &mut tr);
        assert!((tr.endpos.z - 100.0).abs() < 0.1, "endpos {:?}", tr.endpos);
        assert_eq!(tr.ent_index, Some(7));

        // No clip model registered for the index: C skips the entity.
        let ents = ClipEntities {
            entities: std::slice::from_ref(&door),
            player_number: 1,
            model_clip: &[],
        };
        let mut tr = cm.world_trace(start, Vec3f::ZERO, Vec3f::ZERO, end, MASK_PLAYERSOLID);
        cm.clip_move_to_entities(start, Vec3f::ZERO, Vec3f::ZERO, end, &ents, &mut tr);
        assert_eq!(tr.fraction, 1.0);
    }

    #[test]
    fn clip_move_to_entities_keeps_startsolid_and_stops_when_allsolid() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        let ents = [clip_ent(
            5,
            encoded_solid(16, 16, 16),
            Vec3f::new(100.0, 0.0, 50.0),
        )];
        let ents = ClipEntities {
            entities: &ents,
            player_number: 1,
            model_clip: &[],
        };
        let (start, end) = (Vec3f::new(0.0, 0.0, 50.0), Vec3f::new(200.0, 0.0, 50.0));

        // C: a closer entity hit replaces the trace but keeps startsolid.
        let mut tr = Trace {
            fraction: 1.0,
            startsolid: true,
            endpos: end,
            ..Default::default()
        };
        cm.clip_move_to_entities(start, Vec3f::ZERO, Vec3f::ZERO, end, &ents, &mut tr);
        assert!(tr.startsolid);
        assert_eq!(tr.ent_index, Some(5));

        // C returns before tracing anything once the trace is allsolid.
        let mut tr = Trace {
            fraction: 0.0,
            allsolid: true,
            startsolid: true,
            ..Default::default()
        };
        cm.clip_move_to_entities(start, Vec3f::ZERO, Vec3f::ZERO, end, &ents, &mut tr);
        assert_eq!(tr.ent_index, None);
    }

    #[test]
    fn world_trace_marks_world_entity_only_on_hit_like_cl_pmtrace() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_floor_bsp()).unwrap();
        let mask = q2_shared::constants::MASK_PLAYERSOLID;

        let hit = cm.world_trace(
            Vec3f::new(0.0, 0.0, 100.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::new(0.0, 0.0, -100.0),
            mask,
        );
        assert!(hit.fraction < 1.0);
        assert_eq!(hit.ent_index, Some(0));

        let miss = cm.world_trace(
            Vec3f::new(0.0, 0.0, 100.0),
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::new(0.0, 0.0, 50.0),
            mask,
        );
        assert_eq!(miss.fraction, 1.0);
        assert_eq!(miss.ent_index, None);
    }

    #[test]
    fn area_portals_connect_and_disconnect_areas() {
        let mut areas = Vec::new();
        for (num, first) in [(0i32, 0i32), (1, 0), (1, 1)] {
            areas.extend_from_slice(&num.to_le_bytes());
            areas.extend_from_slice(&first.to_le_bytes());
        }
        let mut portals = Vec::new();
        for (portal, other) in [(1i32, 2i32), (1, 1)] {
            portals.extend_from_slice(&portal.to_le_bytes());
            portals.extend_from_slice(&other.to_le_bytes());
        }
        let bsp = with_lump(build_minimal_bsp(), LUMP_AREAS, &areas);
        let bsp = with_lump(bsp, LUMP_AREAPORTALS, &portals);
        let mut cm = CollisionMap::new();
        cm.load_map(&bsp).unwrap();

        assert!(!cm.areas_connected(1, 2).unwrap());
        cm.set_area_portal_state(1, true).unwrap();
        assert!(cm.areas_connected(1, 2).unwrap());
        cm.set_area_portal_state(1, false).unwrap();
        assert!(!cm.areas_connected(1, 2).unwrap());

        // C CM_SetAreaPortalState: "areaportal > numareaportals" is ERR_DROP;
        // portalnum == numareaportals (2 here) is accepted.
        assert!(cm.set_area_portal_state(2, true).is_ok());
        assert!(matches!(
            cm.set_area_portal_state(3, true),
            Err(Q2Error::Drop(_))
        ));

        // C CM_AreasConnected: "area > numareas" is ERR_DROP. Rust also
        // rejects negative and == numareas (3 here), which C reads past.
        for (a1, a2) in [(1, 3), (3, 1), (-1, 1), (1, -1)] {
            assert!(
                matches!(cm.areas_connected(a1, a2), Err(Q2Error::Drop(_))),
                "areas ({a1}, {a2})"
            );
        }
    }

    #[test]
    fn headnode_visible_checks_leaf_cluster_bits() {
        let mut cm = CollisionMap::new();
        cm.load_map(&build_minimal_bsp()).unwrap();
        // Both leafs of the minimal map are in cluster 0.
        assert!(cm.headnode_visible(0, &[0b1]));
        assert!(!cm.headnode_visible(0, &[0b10]));
        assert!(!cm.headnode_visible(0, &[]));
    }

    #[test]
    fn md4_matches_rfc1320_vectors() {
        let hex = |data: &[u8]| -> String {
            md4_digest(data)
                .iter()
                .flat_map(|w| w.to_le_bytes())
                .map(|b| format!("{b:02x}"))
                .collect()
        };
        assert_eq!(hex(b"abc"), "a448017aaf21d8525fc10ae87aa6729d");
        assert_eq!(hex(b"message digest"), "d9130a8164549fe818874806e1c7014b");
        // 80 bytes: crosses the 56-byte padding boundary into a second block.
        let digits =
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890";
        assert_eq!(hex(digits), "e33b4ddc9c38f2199c3e7b164fcc0536");
    }

    #[test]
    fn bsp_truncated_header_rejected() {
        // Only 4 bytes — too short for even the header
        let bsp = vec![0x49, 0x42, 0x53, 0x50]; // "IBSP" magic only
        let mut cm = CollisionMap::new();
        let result = cm.load_map(&bsp);
        assert!(result.is_err(), "truncated BSP should be rejected");
    }
}
