//! Turns the world model's faces into one vertex buffer with per-texture draw
//! ranges (C `GL3_LM_BuildPolygonFromSurface` + `GL3_LM_CreateSurfaceLightmap`
//! + the texture chains of `gl3_surf.c`). GL-free so it can be unit tested.
//!
//! Vertex layout, [`VERTEX_FLOATS`] floats: position(3), texture uv(2),
//! lightmap uv(2), light style per lightmap layer(4, -1 when unused).
//! Triangles are fan-triangulated from the face's edge loop.

use super::lightmap::{
    calc_surface_extents, face_layers, lightmap_coords, style_count, texel_coords, LightmapAtlas,
    MAX_LIGHTMAPS_PER_SURFACE,
};
use super::texture::WorldImages;
use crate::bsp::{BspData, BspFace, BspTexInfo};
use q2_shared::constants::CONTENTS_SOLID;

pub const SURF_SKY: i32 = 0x4;
pub const SURF_WARP: i32 = 0x8;
pub const SURF_TRANS33: i32 = 0x10;
pub const SURF_TRANS66: i32 = 0x20;
pub const SURF_FLOWING: i32 = 0x40;
pub const SURF_NODRAW: i32 = 0x80;

/// Floats per world vertex.
pub const VERTEX_FLOATS: usize = 11;

/// Fragment path a batch uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Shading {
    /// Diffuse times lightmap.
    Lit,
    /// Turbulent UV warp (water, lava), no lightmap.
    Warp,
    /// Diffuse only (translucent windows).
    Plain,
}

/// Blend level from `SURF_TRANS33` / `SURF_TRANS66`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Translucency {
    Opaque,
    Third,
    TwoThirds,
}

impl Translucency {
    /// C `GL3_DrawAlphaSurfaces` alpha values.
    pub fn alpha(self) -> f32 {
        match self {
            Translucency::Opaque => 1.0,
            Translucency::Third => 0.333,
            Translucency::TwoThirds => 0.666,
        }
    }
}

/// Consecutive vertices drawn with one texture, lightmap page and state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub shading: Shading,
    pub translucency: Translucency,
    /// Indices into [`WorldImages::images`], one per animation frame,
    /// starting with the face's own texture.
    pub frames: Vec<usize>,
    /// Lightmap atlas page; 0 and unused unless `shading` is `Lit`.
    pub lightmap_page: usize,
    pub flowing: bool,
    pub first_vertex: i32,
    pub vertex_count: i32,
    /// Areas of the non-solid leafs listing the batch's faces, sorted; empty
    /// for inline models, which C never tests by area.
    pub areas: Vec<i16>,
}

impl Batch {
    /// Whether the batch's faces are seen through `areabits` (C
    /// `R_AreaVisible` on the leafs that mark them in `RecursiveWorldNode`):
    /// `None` shows everything; otherwise some area's bit must be set.
    /// Areas past the sent bits, or negative, are hidden.
    pub fn in_visible_area(&self, areabits: Option<&[u8]>) -> bool {
        let Some(bits) = areabits else {
            return true;
        };
        self.areas.iter().any(|&area| {
            usize::try_from(area).is_ok_and(|area| {
                bits.get(area >> 3)
                    .is_some_and(|byte| byte & (1 << (area & 7)) != 0)
            })
        })
    }

    /// Image for animation frame `frame` (C `R_TextureAnimation`: the
    /// entity frame, or twice the time in seconds for the world).
    pub fn image(&self, frame: i32) -> usize {
        let count = i32::try_from(self.frames.len()).unwrap_or(i32::MAX).max(1);
        let index = usize::try_from(frame.rem_euclid(count)).unwrap_or(0);
        self.frames.get(index).copied().unwrap_or(0)
    }
}

pub struct WorldMesh {
    /// Vertices of the world and of every inline submodel.
    pub vertices: Vec<f32>,
    /// World batches (model 0): opaque first, translucent last.
    pub batches: Vec<Batch>,
    /// Batches of inline model `*n` at index `n - 1`, same ordering.
    pub submodels: Vec<Vec<Batch>>,
    pub lightmaps: LightmapAtlas,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BatchKey {
    translucency: Translucency,
    shading: Shading,
    frames: Vec<usize>,
    lightmap_page: usize,
    flowing: bool,
    /// Last, so splitting by area only breaks ties of the draw order.
    areas: Vec<i16>,
}

/// Corner positions of a face from its surfedge loop; `None` when the loop
/// is out of range or has fewer than 3 usable corners.
pub(super) fn face_positions(bsp: &BspData, face: &BspFace) -> Option<Vec<[f32; 3]>> {
    let count = usize::try_from(face.num_edges).ok().filter(|&n| n >= 3)?;
    let first = usize::try_from(face.first_edge).ok()?;
    let positions: Vec<[f32; 3]> = bsp
        .surface_edges
        .get(first..)?
        .iter()
        .take(count)
        .filter_map(|&surfedge| {
            let edge = bsp
                .edges
                .get(usize::try_from(surfedge.unsigned_abs()).ok()?)?;
            let vertex = if surfedge >= 0 { edge.v[0] } else { edge.v[1] };
            let p = bsp.vertices.get(usize::from(vertex))?.position;
            Some([p.x, p.y, p.z])
        })
        .collect();
    (positions.len() >= 3).then_some(positions)
}

/// Images of the animation that starts at texinfo `start` (C
/// `Mod_LoadTexinfo` numframes): follow `next_texinfo` until it returns to
/// `start` or ends. `0` and out-of-range links end the chain like C's
/// `next > 0` test; a chain that never returns stops after every texinfo
/// has been visited once, where C would loop forever.
fn animation_frames(texinfo: &[BspTexInfo], images: &WorldImages, start: usize) -> Vec<usize> {
    let image = |index: usize| images.texinfo_image.get(index).copied().unwrap_or(0);
    let next = |index: usize| {
        usize::try_from(texinfo.get(index)?.next_texinfo)
            .ok()
            .filter(|&next| next > 0 && next < texinfo.len())
    };
    let mut frames = vec![image(start)];
    let mut step = next(start);
    while let Some(index) = step {
        if index == start || frames.len() >= texinfo.len() {
            break;
        }
        frames.push(image(index));
        step = next(index);
    }
    frames
}

/// Faces of model `model` (0 is the world); without a model lump the world
/// is every face.
fn model_face_range(bsp: &BspData, model: usize) -> std::ops::Range<usize> {
    let index = |v: i32| usize::try_from(v).ok();
    let range = match bsp.models.get(model) {
        Some(m) => match (index(m.first_face), index(m.num_faces)) {
            (Some(first), Some(count)) => first..first.saturating_add(count),
            _ => 0..0,
        },
        None if model == 0 => 0..bsp.faces.len(),
        None => 0..0,
    };
    range.start.min(bsp.faces.len())..range.end.min(bsp.faces.len())
}

fn classify(texinfo: &BspTexInfo) -> (Shading, Translucency) {
    let translucency = if texinfo.flags & SURF_TRANS33 != 0 {
        Translucency::Third
    } else if texinfo.flags & SURF_TRANS66 != 0 {
        Translucency::TwoThirds
    } else {
        Translucency::Opaque
    };
    let shading = if texinfo.flags & SURF_WARP != 0 {
        Shading::Warp
    } else if translucency != Translucency::Opaque {
        // C creates no lightmap for translucent surfaces.
        Shading::Plain
    } else {
        Shading::Lit
    };
    (shading, translucency)
}

/// Areas of the non-solid leafs listing each face, sorted and deduplicated
/// (the leafs whose `marksurfaces` C `RecursiveWorldNode` marks).
fn face_areas(bsp: &BspData) -> Vec<Vec<i16>> {
    let mut areas = vec![Vec::new(); bsp.faces.len()];
    for leaf in bsp.leafs.iter().filter(|l| l.contents != CONTENTS_SOLID) {
        let first = usize::from(leaf.first_leaf_face);
        let listed = first..first + usize::from(leaf.num_leaf_faces);
        for &face in bsp.leaf_faces.get(listed).unwrap_or_default() {
            if let Some(face_areas) = areas.get_mut(usize::from(face)) {
                face_areas.push(leaf.area);
            }
        }
    }
    for face_areas in &mut areas {
        face_areas.sort_unstable();
        face_areas.dedup();
    }
    areas
}

/// Build the mesh of the world and all inline submodels, sharing one vertex
/// buffer and lightmap atlas (C keeps every model's surfaces in one lump).
pub fn build_world_mesh(bsp: &BspData, images: &WorldImages) -> WorldMesh {
    let mut atlas = LightmapAtlas::new();
    let mut vertices = Vec::new();
    let world_face_areas = face_areas(bsp);
    let mut per_model: Vec<Vec<Batch>> = (0..bsp.models.len().max(1))
        .map(|model| {
            let areas: &[Vec<i16>] = if model == 0 { &world_face_areas } else { &[] };
            build_model_batches(bsp, images, model, areas, &mut atlas, &mut vertices)
        })
        .collect();
    let batches = per_model.remove(0);
    WorldMesh {
        vertices,
        batches,
        submodels: per_model,
        lightmaps: atlas,
    }
}

/// Append one model's faces to `vertices` and return its draw batches. Sky and
/// nodraw faces are omitted; malformed faces (bad indices, lightmap too large
/// for a page) are skipped. `face_areas` is indexed by face number; faces
/// past it get no areas.
fn build_model_batches(
    bsp: &BspData,
    images: &WorldImages,
    model: usize,
    face_areas: &[Vec<i16>],
    atlas: &mut LightmapAtlas,
    vertices: &mut Vec<f32>,
) -> Vec<Batch> {
    let mut pending: Vec<(BatchKey, Vec<f32>)> = Vec::new();

    for face_index in model_face_range(bsp, model) {
        let face = &bsp.faces[face_index];
        let Some(texinfo_index) = usize::try_from(face.texinfo_idx).ok() else {
            continue;
        };
        let Some(texinfo) = bsp.texinfo.get(texinfo_index) else {
            continue;
        };
        if texinfo.flags & (SURF_SKY | SURF_NODRAW) != 0 {
            continue;
        }
        let Some(positions) = face_positions(bsp, face) else {
            continue;
        };
        let (shading, translucency) = classify(texinfo);
        let frames = animation_frames(&bsp.texinfo, images, texinfo_index);
        let image = frames[0];
        let (width, height) = images
            .images
            .get(image)
            .map_or((1, 1), |img| (img.width.max(1), img.height.max(1)));

        let lightmap = if shading == Shading::Lit {
            let placed = calc_surface_extents(texinfo, &positions).and_then(|extents| {
                let (w, h) = extents.size();
                let slot = atlas.add(w, h, &face_layers(bsp, face, w, h))?;
                Some((extents, slot))
            });
            match placed {
                Some(placed) => Some(placed),
                None => {
                    tracing::warn!(
                        "GL3: face with texinfo {texinfo_index} has no lightmap space, skipped"
                    );
                    continue;
                }
            }
        } else {
            None
        };

        // The shader scales lightmap layer n by the brightness of style n.
        let styles: [f32; MAX_LIGHTMAPS_PER_SURFACE] = match lightmap {
            Some(_) => {
                let used = style_count(face);
                std::array::from_fn(|n| {
                    if n < used {
                        f32::from(face.styles[n])
                    } else {
                        -1.0
                    }
                })
            }
            None => [-1.0; MAX_LIGHTMAPS_PER_SURFACE],
        };
        let mut corners = Vec::with_capacity(positions.len() * VERTEX_FLOATS);
        for &position in &positions {
            let texel = texel_coords(texinfo, position);
            // Warp shader works on raw texel coordinates (divides by 64 itself).
            let uv = if shading == Shading::Warp {
                texel
            } else {
                [texel[0] / width as f32, texel[1] / height as f32]
            };
            let lm_uv = lightmap.as_ref().map_or([0.0, 0.0], |(extents, slot)| {
                lightmap_coords(texel, extents, slot)
            });
            corners.extend_from_slice(&position);
            corners.extend_from_slice(&uv);
            corners.extend_from_slice(&lm_uv);
            corners.extend_from_slice(&styles);
        }

        let mut triangles = Vec::with_capacity((positions.len() - 2) * 3 * VERTEX_FLOATS);
        for k in 1..positions.len() - 1 {
            for corner in [0, k, k + 1] {
                triangles.extend_from_slice(
                    &corners[corner * VERTEX_FLOATS..(corner + 1) * VERTEX_FLOATS],
                );
            }
        }
        let key = BatchKey {
            translucency,
            shading,
            frames,
            lightmap_page: lightmap.as_ref().map_or(0, |(_, slot)| slot.page),
            flowing: texinfo.flags & SURF_FLOWING != 0,
            areas: face_areas.get(face_index).cloned().unwrap_or_default(),
        };
        pending.push((key, triangles));
    }

    pending.sort_by(|(a, _), (b, _)| a.cmp(b));
    let mut batches: Vec<Batch> = Vec::new();
    for (key, triangles) in pending {
        let count = i32::try_from(triangles.len() / VERTEX_FLOATS).unwrap_or(i32::MAX);
        let first = i32::try_from(vertices.len() / VERTEX_FLOATS).unwrap_or(i32::MAX);
        vertices.extend_from_slice(&triangles);
        let same = batches.last().is_some_and(|batch| {
            (
                batch.shading,
                batch.translucency,
                &batch.frames,
                batch.lightmap_page,
                batch.flowing,
                &batch.areas,
            ) == (
                key.shading,
                key.translucency,
                &key.frames,
                key.lightmap_page,
                key.flowing,
                &key.areas,
            )
        });
        if let (true, Some(last)) = (same, batches.last_mut()) {
            last.vertex_count += count;
        } else {
            batches.push(Batch {
                shading: key.shading,
                translucency: key.translucency,
                frames: key.frames,
                lightmap_page: key.lightmap_page,
                flowing: key.flowing,
                first_vertex: first,
                vertex_count: count,
                areas: key.areas,
            });
        }
    }
    batches
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bsp::{BspEdge, BspModel, BspVertex};
    use crate::gl3::lightmap::{BLOCK_HEIGHT, BLOCK_WIDTH};
    use crate::gl3::texture::{notexture, Image};
    use q2_shared::types::Vec3f;

    fn texinfo(flags: i32, s: [f32; 4], t: [f32; 4]) -> BspTexInfo {
        BspTexInfo {
            vecs: [s, t],
            flags,
            value: 0,
            texture: "t".into(),
            next_texinfo: -1,
        }
    }

    fn empty_bsp() -> BspData {
        BspData {
            planes: vec![],
            vertices: vec![],
            visibility: vec![],
            nodes: vec![],
            texinfo: vec![],
            faces: vec![],
            lightmap_data: vec![],
            leafs: vec![],
            leaf_faces: vec![],
            // Edge 0 is the unused sentinel, as in real maps.
            edges: vec![BspEdge { v: [0, 0] }],
            surface_edges: vec![],
            models: vec![],
        }
    }

    /// Append a face whose edge loop visits `corners` in order.
    fn add_face(bsp: &mut BspData, corners: &[[f32; 3]], texinfo_idx: i16, light_ofs: i32) {
        let base = u16::try_from(bsp.vertices.len()).unwrap();
        let first_edge = i32::try_from(bsp.surface_edges.len()).unwrap();
        for c in corners {
            bsp.vertices.push(BspVertex {
                position: Vec3f::new(c[0], c[1], c[2]),
            });
        }
        let n = u16::try_from(corners.len()).unwrap();
        for k in 0..n {
            bsp.edges.push(BspEdge {
                v: [base + k, base + (k + 1) % n],
            });
            bsp.surface_edges
                .push(i32::try_from(bsp.edges.len() - 1).unwrap());
        }
        bsp.faces.push(BspFace {
            plane_idx: 0,
            side: 0,
            first_edge,
            num_edges: i16::try_from(n).unwrap(),
            texinfo_idx,
            styles: [0, 255, 255, 255],
            light_ofs,
        });
    }

    fn images(sizes: &[(usize, usize)], texinfo_image: Vec<usize>) -> WorldImages {
        let mut all = vec![notexture()];
        for &(width, height) in sizes {
            all.push(Image {
                width,
                height,
                rgba: vec![0; width * height * 4],
                is_lava: false,
            });
        }
        WorldImages {
            images: all,
            texinfo_image,
        }
    }

    const TRI: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [64.0, 0.0, 0.0], [0.0, 64.0, 0.0]];

    fn vertex(mesh: &WorldMesh, index: usize) -> &[f32] {
        &mesh.vertices[index * VERTEX_FLOATS..(index + 1) * VERTEX_FLOATS]
    }

    #[test]
    fn texture_uv_is_projection_over_image_size() {
        // s = y + 4, t = -z; image 64x32. Point (32, 16, 8) -> s=20, t=-8.
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [0.0, 1.0, 0.0, 4.0], [0.0, 0.0, -1.0, 0.0]));
        add_face(
            &mut bsp,
            &[[32.0, 16.0, 8.0], [32.0, 48.0, 8.0], [32.0, 16.0, 40.0]],
            0,
            -1,
        );
        let mesh = build_world_mesh(&bsp, &images(&[(64, 32)], vec![1]));
        assert_eq!(
            &vertex(&mesh, 0)[0..5],
            &[32.0, 16.0, 8.0, 20.0 / 64.0, -8.0 / 32.0]
        );
        // Second corner: y=48 -> s=52; third: z=40 -> t=-40.
        assert_eq!(&vertex(&mesh, 1)[3..5], &[52.0 / 64.0, -8.0 / 32.0]);
        assert_eq!(&vertex(&mesh, 2)[3..5], &[20.0 / 64.0, -40.0 / 32.0]);
    }

    #[test]
    fn lightmap_uv_uses_extents_and_slot() {
        // s = x, t = y over a 64x64 triangle: mins 0, extents 64 -> 5x5 luxels at (0,0).
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        add_face(&mut bsp, &TRI, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        // Vertex (64, 0): (64 - 0 + 0 + 8) / (1024*16), (0 + 8) / (512*16)
        let uv = &vertex(&mesh, 1)[5..7];
        assert!((uv[0] - 72.0 / (BLOCK_WIDTH * 16) as f32).abs() < 1e-7);
        assert!((uv[1] - 8.0 / (BLOCK_HEIGHT * 16) as f32).abs() < 1e-7);
        assert_eq!(mesh.lightmaps.pages.len(), 1);
    }

    #[test]
    fn faces_without_lightmap_data_are_fullbright() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        add_face(&mut bsp, &TRI, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(
            &mesh.lightmaps.pages[0].layers[0][0..4],
            &[255, 255, 255, 255]
        );
    }

    #[test]
    fn faces_with_samples_copy_them_into_the_atlas() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        add_face(&mut bsp, &TRI, 0, 0);
        bsp.lightmap_data = (0..75).map(|b| b as u8).collect(); // 5x5x3
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(&mesh.lightmaps.pages[0].layers[0][0..4], &[0, 1, 2, 2]);
    }

    #[test]
    fn batches_group_by_texture_and_keep_vertex_ranges_contiguous() {
        let mut bsp = empty_bsp();
        for _ in 0..2 {
            bsp.texinfo
                .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        }
        // Faces alternate texinfo 0,1,0: texinfo 0 and 2 share image 1.
        add_face(&mut bsp, &TRI, 0, -1);
        add_face(&mut bsp, &TRI, 1, -1);
        add_face(&mut bsp, &TRI, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64), (32, 32)], vec![1, 2]));
        assert_eq!(mesh.batches.len(), 2);
        assert_eq!(
            (mesh.batches[0].frames[0], mesh.batches[0].vertex_count),
            (1, 6)
        );
        assert_eq!(
            (mesh.batches[1].frames[0], mesh.batches[1].vertex_count),
            (2, 3)
        );
        assert_eq!(mesh.batches[0].first_vertex, 0);
        assert_eq!(mesh.batches[1].first_vertex, 6);
        assert_eq!(mesh.vertices.len(), 9 * VERTEX_FLOATS);
    }

    #[test]
    fn sky_and_nodraw_faces_are_not_in_the_mesh() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(SURF_SKY, [1.0, 0.0, 0.0, 0.0], [0.0; 4]));
        bsp.texinfo
            .push(texinfo(SURF_NODRAW, [1.0, 0.0, 0.0, 0.0], [0.0; 4]));
        add_face(&mut bsp, &TRI, 0, -1);
        add_face(&mut bsp, &TRI, 1, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1, 1]));
        assert!(mesh.vertices.is_empty() && mesh.batches.is_empty());
    }

    /// Texinfos `links[i]` -> next, each with its own image `i + 1`.
    fn chained(links: &[i32]) -> (Vec<BspTexInfo>, WorldImages) {
        let texinfo = links
            .iter()
            .map(|&next| BspTexInfo {
                next_texinfo: next,
                ..texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0])
            })
            .collect();
        let sizes = vec![(64, 64); links.len()];
        (texinfo, images(&sizes, (1..=links.len()).collect()))
    }

    #[test]
    fn animation_follows_next_texinfo_from_the_face_texinfo_until_it_loops() {
        let (texinfo, images) = chained(&[-1, 2, 3, 1]);
        assert_eq!(animation_frames(&texinfo, &images, 1), vec![2, 3, 4]);
        assert_eq!(animation_frames(&texinfo, &images, 3), vec![4, 2, 3]);
        assert_eq!(animation_frames(&texinfo, &images, 0), vec![1]);
    }

    #[test]
    fn zero_and_out_of_range_links_end_the_animation() {
        // C treats nexttexinfo 0 as "no next", not as texinfo 0.
        let (texinfo, images) = chained(&[1, 0, 9]);
        assert_eq!(animation_frames(&texinfo, &images, 0), vec![1, 2]);
        assert_eq!(animation_frames(&texinfo, &images, 2), vec![3]);
    }

    #[test]
    fn chain_that_never_returns_to_its_start_is_bounded() {
        let (texinfo, images) = chained(&[1, 2, 1]);
        assert_eq!(animation_frames(&texinfo, &images, 0), vec![1, 2, 3]);
    }

    #[test]
    fn batch_image_picks_the_frame_modulo_the_chain() {
        let batch = Batch {
            shading: Shading::Lit,
            translucency: Translucency::Opaque,
            frames: vec![5, 6, 7],
            lightmap_page: 0,
            flowing: false,
            first_vertex: 0,
            vertex_count: 0,
            areas: vec![],
        };
        assert_eq!(
            [0, 1, 2, 3, 7, -1].map(|frame| batch.image(frame)),
            [5, 6, 7, 5, 6, 7]
        );
    }

    #[test]
    fn faces_at_different_points_of_one_animation_are_batched_apart() {
        let mut bsp = empty_bsp();
        let (texinfo, images) = chained(&[-1, 2, 1]);
        bsp.texinfo = texinfo;
        add_face(&mut bsp, &TRI, 1, -1);
        add_face(&mut bsp, &TRI, 2, -1);
        add_face(&mut bsp, &TRI, 1, -1);
        let mesh = build_world_mesh(&bsp, &images);
        let frames: Vec<_> = mesh
            .batches
            .iter()
            .map(|b| (b.frames.clone(), b.vertex_count))
            .collect();
        assert_eq!(frames, vec![(vec![2, 3], 6), (vec![3, 2], 3)]);
    }

    #[test]
    fn lit_vertices_carry_the_face_light_styles_and_others_none() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        bsp.texinfo.push(texinfo(
            SURF_WARP,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ));
        add_face(&mut bsp, &TRI, 0, -1);
        bsp.faces[0].styles = [0, 6, 255, 9];
        add_face(&mut bsp, &TRI, 1, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1, 1]));
        let lit = mesh
            .batches
            .iter()
            .find(|b| b.shading == Shading::Lit)
            .unwrap();
        let warp = mesh
            .batches
            .iter()
            .find(|b| b.shading == Shading::Warp)
            .unwrap();
        // Styles after the first 255 are ignored, like C's style loops.
        let first = |batch: &Batch| vertex(&mesh, batch.first_vertex as usize)[7..11].to_vec();
        assert_eq!(first(lit), [0.0, 6.0, -1.0, -1.0]);
        assert_eq!(first(warp), [-1.0; 4]);
    }

    #[test]
    fn warp_faces_keep_raw_texel_uv_and_get_no_lightmap() {
        let mut bsp = empty_bsp();
        bsp.texinfo.push(texinfo(
            SURF_WARP,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ));
        add_face(&mut bsp, &TRI, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(mesh.batches[0].shading, Shading::Warp);
        assert_eq!(&vertex(&mesh, 1)[3..7], &[64.0, 0.0, 0.0, 0.0]);
        assert!(mesh.lightmaps.pages.is_empty());
    }

    #[test]
    fn translucent_faces_sort_after_opaque_and_carry_alpha() {
        let mut bsp = empty_bsp();
        bsp.texinfo.push(texinfo(
            SURF_TRANS66,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ));
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        bsp.texinfo.push(texinfo(
            SURF_TRANS33 | SURF_WARP,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ));
        add_face(&mut bsp, &TRI, 0, -1);
        add_face(&mut bsp, &TRI, 1, -1);
        add_face(&mut bsp, &TRI, 2, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(8, 8)], vec![1, 1, 1]));
        let kinds: Vec<_> = mesh
            .batches
            .iter()
            .map(|b| (b.shading, b.translucency))
            .collect();
        assert_eq!(
            kinds,
            vec![
                (Shading::Lit, Translucency::Opaque),
                (Shading::Warp, Translucency::Third),
                (Shading::Plain, Translucency::TwoThirds),
            ]
        );
        assert_eq!(Translucency::Third.alpha(), 0.333);
        assert_eq!(Translucency::TwoThirds.alpha(), 0.666);
        assert_eq!(Translucency::Opaque.alpha(), 1.0);
    }

    #[test]
    fn flowing_faces_are_batched_apart() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        bsp.texinfo.push(texinfo(
            SURF_FLOWING,
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ));
        add_face(&mut bsp, &TRI, 0, -1);
        add_face(&mut bsp, &TRI, 1, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(8, 8)], vec![1, 1]));
        assert_eq!(mesh.batches.len(), 2);
        assert_ne!(mesh.batches[0].flowing, mesh.batches[1].flowing);
    }

    #[test]
    fn quads_are_fan_triangulated() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        let quad = [
            [0.0, 0.0, 0.0],
            [16.0, 0.0, 0.0],
            [16.0, 16.0, 0.0],
            [0.0, 16.0, 0.0],
        ];
        add_face(&mut bsp, &quad, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[(8, 8)], vec![1]));
        assert_eq!(mesh.batches[0].vertex_count, 6);
        assert_eq!(&vertex(&mesh, 3)[0..3], &[0.0, 0.0, 0.0]);
        assert_eq!(&vertex(&mesh, 4)[0..3], &[16.0, 16.0, 0.0]);
        assert_eq!(&vertex(&mesh, 5)[0..3], &[0.0, 16.0, 0.0]);
    }

    #[test]
    fn malformed_faces_are_skipped_not_panicked_on() {
        let base = {
            let mut bsp = empty_bsp();
            bsp.texinfo
                .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
            add_face(&mut bsp, &TRI, 0, -1);
            bsp
        };
        let imgs = images(&[(8, 8)], vec![1]);
        let mut neg_edges = base_clone(&base);
        neg_edges.faces[0].num_edges = -1;
        assert!(build_world_mesh(&neg_edges, &imgs).vertices.is_empty());
        let mut neg_first = base_clone(&base);
        neg_first.faces[0].first_edge = -1;
        assert!(build_world_mesh(&neg_first, &imgs).vertices.is_empty());
        let mut neg_texinfo = base_clone(&base);
        neg_texinfo.faces[0].texinfo_idx = -1;
        assert!(build_world_mesh(&neg_texinfo, &imgs).vertices.is_empty());
        let mut min_edge = base_clone(&base);
        min_edge.surface_edges[1] = i32::MIN;
        assert!(build_world_mesh(&min_edge, &imgs).vertices.is_empty());
        let mut huge_edge = base_clone(&base);
        huge_edge.surface_edges[0] = i32::MAX;
        assert!(build_world_mesh(&huge_edge, &imgs).vertices.is_empty());
    }

    fn base_clone(bsp: &BspData) -> BspData {
        let mut copy = empty_bsp();
        copy.texinfo = bsp.texinfo.clone();
        copy.faces = bsp.faces.clone();
        copy.vertices = bsp.vertices.clone();
        copy.edges = bsp.edges.clone();
        copy.surface_edges = bsp.surface_edges.clone();
        copy
    }

    #[test]
    fn negative_model_face_range_draws_nothing() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        add_face(&mut bsp, &TRI, 0, -1);
        let model = |first_face, num_faces| BspModel {
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            origin: Vec3f::ZERO,
            head_node: 0,
            first_face,
            num_faces,
        };
        let imgs = images(&[(8, 8)], vec![1]);
        bsp.models = vec![model(0, -1)];
        assert!(build_world_mesh(&bsp, &imgs).vertices.is_empty());
        bsp.models = vec![model(-1, 1)];
        assert!(build_world_mesh(&bsp, &imgs).vertices.is_empty());
        bsp.models = vec![model(0, 1)];
        assert!(!build_world_mesh(&bsp, &imgs).vertices.is_empty());
    }

    #[test]
    fn submodel_faces_get_their_own_batches_after_the_world_in_one_buffer() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        for _ in 0..3 {
            add_face(&mut bsp, &TRI, 0, -1);
        }
        let model = |first_face, num_faces| BspModel {
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            origin: Vec3f::ZERO,
            head_node: 0,
            first_face,
            num_faces,
        };
        // World owns face 0; *1 owns faces 1 and 2; *2 owns none.
        bsp.models = vec![model(0, 1), model(1, 2), model(3, 0)];
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));

        assert_eq!(mesh.submodels.len(), 2);
        assert_eq!(
            (mesh.batches[0].first_vertex, mesh.batches[0].vertex_count),
            (0, 3)
        );
        assert_eq!(mesh.submodels[0].len(), 1);
        assert_eq!(
            (
                mesh.submodels[0][0].first_vertex,
                mesh.submodels[0][0].vertex_count
            ),
            (3, 6)
        );
        assert!(mesh.submodels[1].is_empty());
        assert_eq!(mesh.vertices.len(), 9 * VERTEX_FLOATS);
        // All three faces share the atlas page the world allocated first.
        assert_eq!(mesh.lightmaps.pages.len(), 1);
    }

    #[test]
    fn face_too_big_for_a_lightmap_page_is_dropped() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        // 20000 units -> 1252 luxels, wider than the 1024-sample page.
        add_face(
            &mut bsp,
            &[[0.0, 0.0, 0.0], [20000.0, 0.0, 0.0], [0.0, 16.0, 0.0]],
            0,
            -1,
        );
        let mesh = build_world_mesh(&bsp, &images(&[(8, 8)], vec![1]));
        assert!(mesh.vertices.is_empty());
    }

    #[test]
    fn faces_missing_from_texinfo_image_table_use_notexture() {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        add_face(&mut bsp, &TRI, 0, -1);
        let mesh = build_world_mesh(&bsp, &images(&[], vec![]));
        assert_eq!(mesh.batches[0].frames[0], 0);
        // notexture is 8x8, so s = 64 / 8.
        assert_eq!(vertex(&mesh, 1)[3], 8.0);
    }

    /// A leaf of `contents` in `area` listing `faces`.
    fn add_leaf(bsp: &mut BspData, contents: i32, area: i16, faces: &[u16]) {
        bsp.leafs.push(crate::bsp::BspLeaf {
            contents,
            cluster: 0,
            area,
            first_leaf_face: u16::try_from(bsp.leaf_faces.len()).unwrap(),
            num_leaf_faces: u16::try_from(faces.len()).unwrap(),
            first_leaf_brush: 0,
            num_leaf_brushes: 0,
        });
        bsp.leaf_faces.extend_from_slice(faces);
    }

    /// One texture; `faces` triangles, all in the world model.
    fn one_texture_bsp(faces: usize) -> BspData {
        let mut bsp = empty_bsp();
        bsp.texinfo
            .push(texinfo(0, [1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]));
        for _ in 0..faces {
            add_face(&mut bsp, &TRI, 0, -1);
        }
        bsp
    }

    /// (first vertex, count) of the world batches drawn with `areabits`.
    fn drawn(mesh: &WorldMesh, areabits: Option<&[u8]>) -> Vec<(i32, i32)> {
        mesh.batches
            .iter()
            .filter(|batch| batch.in_visible_area(areabits))
            .map(|batch| (batch.first_vertex, batch.vertex_count))
            .collect()
    }

    #[test]
    fn faces_only_in_leafs_of_hidden_areas_are_not_drawn() {
        let mut bsp = one_texture_bsp(3);
        add_leaf(&mut bsp, 0, 1, &[0, 2]);
        add_leaf(&mut bsp, 0, 2, &[1]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        // Faces 0 and 2 (area 1) share a batch; face 1 (area 2) is apart.
        assert_eq!(drawn(&mesh, Some(&[0b0010])), [(0, 6)]);
        assert_eq!(drawn(&mesh, Some(&[0b0100])), [(6, 3)]);
        assert_eq!(drawn(&mesh, Some(&[0b0110])), [(0, 6), (6, 3)]);
        assert!(drawn(&mesh, Some(&[0])).is_empty());
    }

    #[test]
    fn face_listed_by_a_hidden_and_a_visible_leaf_is_drawn() {
        let mut bsp = one_texture_bsp(1);
        add_leaf(&mut bsp, 0, 1, &[0]);
        add_leaf(&mut bsp, 0, 2, &[0]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(drawn(&mesh, Some(&[0b0010])), [(0, 3)]);
        assert_eq!(drawn(&mesh, Some(&[0b0100])), [(0, 3)]);
    }

    #[test]
    fn solid_leafs_give_their_faces_no_area() {
        let mut bsp = one_texture_bsp(1);
        add_leaf(&mut bsp, q2_shared::constants::CONTENTS_SOLID, 1, &[0]);
        add_leaf(&mut bsp, 0, 2, &[0]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert!(drawn(&mesh, Some(&[0b0010])).is_empty());
        assert_eq!(drawn(&mesh, Some(&[0b0100])), [(0, 3)]);
    }

    #[test]
    fn areas_past_the_sent_bits_or_negative_are_hidden() {
        let mut bsp = one_texture_bsp(2);
        add_leaf(&mut bsp, 0, 9, &[0]);
        add_leaf(&mut bsp, 0, -1, &[1]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert!(drawn(&mesh, Some(&[0xff])).is_empty());
    }

    #[test]
    fn without_area_bits_every_world_face_is_drawn_even_in_no_leaf() {
        let mut bsp = one_texture_bsp(2);
        add_leaf(&mut bsp, 0, 1, &[0]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(drawn(&mesh, None).len(), 2);
        // C marks surfaces only through leafs: one in none never draws.
        assert_eq!(drawn(&mesh, Some(&[0xff])).len(), 1);
    }

    #[test]
    fn submodel_faces_carry_no_areas_so_their_batches_stay_whole() {
        let mut bsp = one_texture_bsp(3);
        let model = |first_face, num_faces| BspModel {
            mins: Vec3f::ZERO,
            maxs: Vec3f::ZERO,
            origin: Vec3f::ZERO,
            head_node: 0,
            first_face,
            num_faces,
        };
        bsp.models = vec![model(0, 1), model(1, 2)];
        add_leaf(&mut bsp, 0, 1, &[0, 1]);
        add_leaf(&mut bsp, 0, 2, &[2]);
        let mesh = build_world_mesh(&bsp, &images(&[(64, 64)], vec![1]));
        assert_eq!(mesh.batches[0].areas, [1]);
        assert_eq!(mesh.submodels[0].len(), 1);
        assert!(mesh.submodels[0][0].areas.is_empty());
    }
}
