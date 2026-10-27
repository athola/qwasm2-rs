//! Lightmap extents and atlas packing (C `Mod_CalcSurfaceExtents` in
//! `gl3_model.c`, `GL3_LM_AllocBlock` / `GL3_LM_CreateSurfaceLightmap` in
//! `gl3_lightmap.c`, `GL3_BuildLightMap` in `gl3_light.c`).
//!
//! Only the first lightmap style of a face is packed; the shader scales it by
//! 1.0 like C does for the default (all-white) light style table.

use crate::bsp::{BspData, BspFace, BspTexInfo};

/// C `BLOCK_WIDTH` / `BLOCK_HEIGHT` for GL3.
pub const BLOCK_WIDTH: usize = 1024;
pub const BLOCK_HEIGHT: usize = 512;
const LIGHTMAP_BYTES: usize = 4;
/// World units per lightmap sample.
pub const LUXEL_SIZE: i32 = 16;

/// Texture-space bounding box of a face, snapped to the lightmap grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extents {
    /// C `texturemins`: floor(min/16)*16 for s and t.
    pub texture_mins: [i32; 2],
    /// C `extents`: (ceil(max/16) - floor(min/16)) * 16 for s and t.
    pub extents: [i32; 2],
}

impl Extents {
    /// Lightmap size in samples, C `(extents >> 4) + 1`.
    pub fn size(&self) -> (usize, usize) {
        let samples = |extent: i32| usize::try_from((extent >> 4) + 1).unwrap_or(0);
        (samples(self.extents[0]), samples(self.extents[1]))
    }
}

/// Texture coordinate of a world position before dividing by image size.
pub fn texel_coords(texinfo: &BspTexInfo, position: [f32; 3]) -> [f32; 2] {
    let project = |vec: &[f32; 4]| {
        position[0] * vec[0] + position[1] * vec[1] + position[2] * vec[2] + vec[3]
    };
    [project(&texinfo.vecs[0]), project(&texinfo.vecs[1])]
}

/// C `Mod_CalcSurfaceExtents`; `None` when the face has no vertices.
pub fn calc_surface_extents(texinfo: &BspTexInfo, positions: &[[f32; 3]]) -> Option<Extents> {
    if positions.is_empty() {
        return None;
    }
    let mut mins = [f32::INFINITY; 2];
    let mut maxs = [f32::NEG_INFINITY; 2];
    for &position in positions {
        let coords = texel_coords(texinfo, position);
        for axis in 0..2 {
            mins[axis] = mins[axis].min(coords[axis]);
            maxs[axis] = maxs[axis].max(coords[axis]);
        }
    }
    let mut extents = Extents {
        texture_mins: [0; 2],
        extents: [0; 2],
    };
    for axis in 0..2 {
        // floor/ceil, not truncation: negative coordinates round away from 0.
        let low = (mins[axis] / 16.0).floor() as i32;
        let high = (maxs[axis] / 16.0).ceil() as i32;
        extents.texture_mins[axis] = low * LUXEL_SIZE;
        extents.extents[axis] = (high - low) * LUXEL_SIZE;
    }
    Some(extents)
}

/// C `GL3_LM_AllocBlock`: skyline allocator over one page.
pub struct BlockAllocator {
    allocated: Vec<usize>,
}

impl BlockAllocator {
    pub fn new() -> Self {
        Self {
            allocated: vec![0; BLOCK_WIDTH],
        }
    }

    /// Find the lowest free spot for a `w` x `h` block, or `None` if the page is full.
    pub fn alloc(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
        let mut best = BLOCK_HEIGHT;
        let mut spot = None;
        // C iterates `i < BLOCK_WIDTH - w`, so the last column is never a start.
        for i in 0..BLOCK_WIDTH.saturating_sub(w) {
            let mut best2 = 0;
            let mut fits = true;
            for column in &self.allocated[i..i + w] {
                if *column >= best {
                    fits = false;
                    break;
                }
                best2 = best2.max(*column);
            }
            if fits {
                spot = Some(i);
                best = best2;
            }
        }
        let x = spot?;
        if best + h > BLOCK_HEIGHT {
            return None;
        }
        self.allocated[x..x + w].fill(best + h);
        Some((x, best))
    }
}

impl Default for BlockAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Where a face's lightmap landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LightmapSlot {
    pub page: usize,
    pub x: usize,
    pub y: usize,
}

/// All lightmap pages for a map, RGBA8 `BLOCK_WIDTH` x `BLOCK_HEIGHT` each.
pub struct LightmapAtlas {
    pub pages: Vec<Vec<u8>>,
    allocator: BlockAllocator,
}

impl LightmapAtlas {
    pub fn new() -> Self {
        Self {
            pages: Vec::new(),
            allocator: BlockAllocator::new(),
        }
    }

    /// Reserve `w` x `h` samples and copy in `samples` (RGB triples, row-major);
    /// `None` fills the block fullbright like C does for `lightofs == -1`.
    /// Returns `None` when the block cannot fit even on an empty page.
    pub fn add(&mut self, w: usize, h: usize, samples: Option<&[u8]>) -> Option<LightmapSlot> {
        if w == 0 || h == 0 {
            return None;
        }
        let (x, y, page) = match self.allocator.alloc(w, h) {
            Some((x, y)) if !self.pages.is_empty() => (x, y, self.pages.len() - 1),
            _ => {
                let mut fresh = BlockAllocator::new();
                let (x, y) = fresh.alloc(w, h)?;
                self.allocator = fresh;
                self.pages
                    .push(vec![0; BLOCK_WIDTH * BLOCK_HEIGHT * LIGHTMAP_BYTES]);
                (x, y, self.pages.len() - 1)
            }
        };
        let buffer = &mut self.pages[page];
        for row in 0..h {
            for column in 0..w {
                let texel = ((y + row) * BLOCK_WIDTH + x + column) * LIGHTMAP_BYTES;
                let rgba = match samples {
                    Some(rgb) => {
                        let at = (row * w + column) * 3;
                        let (r, g, b) = (rgb[at], rgb[at + 1], rgb[at + 2]);
                        [r, g, b, r.max(g).max(b)]
                    }
                    None => [255; 4],
                };
                buffer[texel..texel + 4].copy_from_slice(&rgba);
            }
        }
        Some(LightmapSlot { page, x, y })
    }
}

impl Default for LightmapAtlas {
    fn default() -> Self {
        Self::new()
    }
}

/// Style-0 samples for a face: `w * h * 3` bytes at `lightofs`, or `None`
/// when the face has no lightmap data or the offset leaves the lump.
pub fn face_samples<'a>(bsp: &'a BspData, face: &BspFace, w: usize, h: usize) -> Option<&'a [u8]> {
    let start = usize::try_from(face.light_ofs).ok()?;
    let end = start.checked_add(w.checked_mul(h)?.checked_mul(3)?)?;
    bsp.lightmap_data.get(start..end)
}

/// Lightmap coordinate of a vertex (C `GL3_LM_BuildPolygonFromSurface`):
/// texel coordinate relative to `texturemins`, offset to the slot and to the
/// luxel centre (+8), over the page size in texels.
pub fn lightmap_coords(texel: [f32; 2], extents: &Extents, slot: &LightmapSlot) -> [f32; 2] {
    let coord = |axis: usize, origin: usize, block: usize| {
        (texel[axis] - extents.texture_mins[axis] as f32 + origin as f32 * 16.0 + 8.0)
            / (block * 16) as f32
    };
    [
        coord(0, slot.x, BLOCK_WIDTH),
        coord(1, slot.y, BLOCK_HEIGHT),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texinfo(s: [f32; 4], t: [f32; 4]) -> BspTexInfo {
        BspTexInfo {
            vecs: [s, t],
            flags: 0,
            value: 0,
            texture: "t".into(),
            next_texinfo: -1,
        }
    }

    #[test]
    fn extents_snap_to_16_unit_grid() {
        // s = x, t = y. Spans x 5..40 -> floor(5/16)=0, ceil(40/16)=3.
        let ti = texinfo([1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]);
        let ext = calc_surface_extents(&ti, &[[5.0, 0.0, 0.0], [40.0, 16.0, 7.0]]).unwrap();
        assert_eq!(ext.texture_mins, [0, 0]);
        assert_eq!(ext.extents, [48, 16]);
        assert_eq!(ext.size(), (4, 2));
    }

    #[test]
    fn extents_use_floor_and_ceil_for_negative_coordinates() {
        // x -20..-3: floor(-20/16) = -2 (not -1), ceil(-3/16) = 0 (-0).
        let ti = texinfo([1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]);
        let ext = calc_surface_extents(&ti, &[[-20.0, -17.0, 0.0], [-3.0, -16.0, 0.0]]).unwrap();
        assert_eq!(ext.texture_mins, [-32, -32]);
        assert_eq!(ext.extents, [32, 16]);
    }

    #[test]
    fn extents_apply_offset_and_all_three_axes() {
        // s = z + 8, so z 0..24 gives s 8..32 -> mins 0, extent 32.
        let ti = texinfo([0.0, 0.0, 1.0, 8.0], [1.0, 0.0, 0.0, 0.0]);
        let ext = calc_surface_extents(&ti, &[[0.0, 0.0, 0.0], [0.0, 0.0, 24.0]]).unwrap();
        assert_eq!(ext.texture_mins[0], 0);
        assert_eq!(ext.extents[0], 32);
        assert_eq!(ext.extents[1], 0);
        assert_eq!(ext.size(), (3, 1));
    }

    #[test]
    fn extents_of_empty_face_are_none() {
        let ti = texinfo([1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]);
        assert!(calc_surface_extents(&ti, &[]).is_none());
    }

    #[test]
    fn allocator_places_blocks_side_by_side_then_stacks() {
        let mut alloc = BlockAllocator::new();
        assert_eq!(alloc.alloc(10, 5), Some((0, 0)));
        // The scan keeps the last strictly-lower candidate: columns 0..10 are
        // at height 5, so the free floor at x=10 wins.
        assert_eq!(alloc.alloc(10, 5), Some((10, 0)));
    }

    #[test]
    fn allocator_fills_page_then_reports_full() {
        let mut alloc = BlockAllocator::new();
        // One block spanning almost the whole width; stack until height runs out.
        let w = BLOCK_WIDTH - 1;
        assert_eq!(alloc.alloc(w, BLOCK_HEIGHT / 2), Some((0, 0)));
        assert_eq!(
            alloc.alloc(w, BLOCK_HEIGHT / 2),
            Some((0, BLOCK_HEIGHT / 2))
        );
        assert_eq!(alloc.alloc(w, 1), None);
    }

    #[test]
    fn allocator_rejects_block_taller_or_wider_than_page() {
        let mut alloc = BlockAllocator::new();
        assert_eq!(alloc.alloc(4, BLOCK_HEIGHT + 1), None);
        assert_eq!(alloc.alloc(BLOCK_WIDTH, 1), None);
    }

    #[test]
    fn atlas_starts_new_page_when_full() {
        let mut atlas = LightmapAtlas::new();
        let w = BLOCK_WIDTH - 1;
        let first = atlas.add(w, BLOCK_HEIGHT, None).unwrap();
        assert_eq!(first.page, 0);
        let second = atlas.add(w, 4, None).unwrap();
        assert_eq!((second.page, second.x, second.y), (1, 0, 0));
        assert_eq!(atlas.pages.len(), 2);
    }

    #[test]
    fn atlas_rejects_block_that_cannot_fit_an_empty_page() {
        let mut atlas = LightmapAtlas::new();
        assert!(atlas.add(BLOCK_WIDTH + 1, 1, None).is_none());
        assert!(atlas.add(1, BLOCK_HEIGHT + 1, None).is_none());
        assert!(atlas.add(0, 1, None).is_none());
    }

    #[test]
    fn atlas_copies_samples_with_max_component_alpha() {
        let mut atlas = LightmapAtlas::new();
        // 2x1 block: two RGB samples.
        let slot = atlas.add(2, 1, Some(&[10, 200, 30, 1, 2, 3])).unwrap();
        let row = &atlas.pages[0][slot.y * BLOCK_WIDTH * 4 + slot.x * 4..][..8];
        assert_eq!(row, &[10, 200, 30, 200, 1, 2, 3, 3]);
    }

    #[test]
    fn atlas_fills_missing_samples_fullbright() {
        let mut atlas = LightmapAtlas::new();
        let slot = atlas.add(1, 1, None).unwrap();
        let at = (slot.y * BLOCK_WIDTH + slot.x) * 4;
        assert_eq!(&atlas.pages[0][at..at + 4], &[255, 255, 255, 255]);
    }

    #[test]
    fn lightmap_coords_match_c_formula() {
        // s_tex 24, mins 16, slot x=3 -> (24 - 16 + 48 + 8) / (1024*16)
        let ext = Extents {
            texture_mins: [16, 0],
            extents: [32, 16],
        };
        let slot = LightmapSlot {
            page: 0,
            x: 3,
            y: 1,
        };
        let uv = lightmap_coords([24.0, 4.0], &ext, &slot);
        assert!((uv[0] - 64.0 / 16384.0).abs() < 1e-9);
        // t: (4 - 0 + 16 + 8) / (512*16)
        assert!((uv[1] - 28.0 / 8192.0).abs() < 1e-9);
    }

    #[test]
    fn face_samples_rejects_missing_or_out_of_range_offsets() {
        let mut bsp = BspData {
            planes: vec![],
            vertices: vec![],
            visibility: vec![],
            nodes: vec![],
            texinfo: vec![],
            faces: vec![],
            lightmap_data: vec![0; 12],
            leafs: vec![],
            leaf_faces: vec![],
            edges: vec![],
            surface_edges: vec![],
            models: vec![],
        };
        let mut face = BspFace {
            plane_idx: 0,
            side: 0,
            first_edge: 0,
            num_edges: 3,
            texinfo_idx: 0,
            styles: [0; 4],
            light_ofs: 3,
        };
        assert_eq!(face_samples(&bsp, &face, 2, 1).unwrap().len(), 6);
        assert!(face_samples(&bsp, &face, 2, 2).is_none()); // 3 + 12 > 12
        face.light_ofs = -1;
        assert!(face_samples(&bsp, &face, 1, 1).is_none());
        bsp.lightmap_data.clear();
        face.light_ofs = 0;
        assert!(face_samples(&bsp, &face, 1, 1).is_none());
    }
}
