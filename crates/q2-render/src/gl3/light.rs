//! Light under a point, for shading entities (C `GL3_LightPoint` and
//! `RecursiveLightPoint` in `gl3_light.c`). GL-free.
//!
//! [`LightTree`] keeps only what the trace needs from the BSP (the node tree,
//! per-face extents and lightmap samples) because `load_bsp` borrows the
//! `BspData` and does not retain it. Only the first lightmap style is read,
//! matching what the world shader draws; light styles and `r_modulate` (1.0)
//! are not modelled.

use q2_render_api::DLight;

use super::lightmap::calc_surface_extents;
use super::world::{face_positions, SURF_SKY, SURF_WARP};
use crate::bsp::BspData;

/// C `GL3_LightPoint` traces this far below the point.
const TRACE_DEPTH: f32 = 2048.0;

struct LightNode {
    normal: [f32; 3],
    dist: f32,
    /// Node index when `>= 0`, leaf `-(n + 1)` otherwise.
    children: [i32; 2],
    first_face: usize,
    num_faces: usize,
}

struct LightFace {
    vecs: [[f32; 4]; 2],
    texture_mins: [i32; 2],
    extents: [i32; 2],
    /// Byte offset into the lighting lump; `None` for faces without a lightmap.
    samples: Option<usize>,
}

pub struct LightTree {
    nodes: Vec<LightNode>,
    /// Parallel to the BSP face array; `None` for faces C gives no lightmap
    /// (sky, warp) or whose geometry is malformed.
    faces: Vec<Option<LightFace>>,
    lighting: Vec<u8>,
}

impl LightTree {
    pub fn from_bsp(bsp: &BspData) -> Self {
        let nodes = bsp
            .nodes
            .iter()
            .map(|node| {
                let plane = usize::try_from(node.plane_idx)
                    .ok()
                    .and_then(|index| bsp.planes.get(index));
                LightNode {
                    normal: plane.map_or([0.0; 3], |p| [p.normal.x, p.normal.y, p.normal.z]),
                    dist: plane.map_or(0.0, |p| p.dist),
                    children: node.children,
                    first_face: usize::from(node.first_face),
                    num_faces: usize::from(node.num_faces),
                }
            })
            .collect();
        let faces = bsp
            .faces
            .iter()
            .map(|face| {
                let texinfo = bsp.texinfo.get(usize::try_from(face.texinfo_idx).ok()?)?;
                if texinfo.flags & (SURF_SKY | SURF_WARP) != 0 {
                    return None;
                }
                let positions = face_positions(bsp, face)?;
                let extents = calc_surface_extents(texinfo, &positions)?;
                Some(LightFace {
                    vecs: texinfo.vecs,
                    texture_mins: extents.texture_mins,
                    extents: extents.extents,
                    samples: usize::try_from(face.light_ofs).ok(),
                })
            })
            .collect();
        Self {
            nodes,
            faces,
            lighting: bsp.lightmap_data.clone(),
        }
    }

    /// Light colour (0..1 per channel, may exceed 1 with dynamic lights) at
    /// `origin`: the lightmap of the first face hit by a ray straight down,
    /// plus dynamic lights. Maps without lighting data are fullbright.
    pub fn light_point(&self, origin: [f32; 3], dlights: &[DLight]) -> [f32; 3] {
        if self.lighting.is_empty() {
            return [1.0; 3];
        }
        let end = [origin[0], origin[1], origin[2] - TRACE_DEPTH];
        let mut color = self.trace(0, origin, end).unwrap_or([0.0; 3]);
        for light in dlights {
            let delta = [
                origin[0] - light.origin.x,
                origin[1] - light.origin.y,
                origin[2] - light.origin.z,
            ];
            let distance = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
            let add = (light.intensity - distance) * (1.0 / 256.0);
            if add > 0.0 {
                color[0] += add * light.color.x;
                color[1] += add * light.color.y;
                color[2] += add * light.color.z;
            }
        }
        color
    }

    /// C `RecursiveLightPoint`: `None` when the segment hits no lit face.
    fn trace(&self, child: i32, start: [f32; 3], end: [f32; 3]) -> Option<[f32; 3]> {
        let node = self.nodes.get(usize::try_from(child).ok()?)?;
        let front = dot(start, node.normal) - node.dist;
        let back = dot(end, node.normal) - node.dist;
        let side = usize::from(front < 0.0);
        if (back < 0.0) == (side == 1) {
            return self.trace(node.children[side], start, end);
        }

        let fraction = front / (front - back);
        let mid = [
            start[0] + (end[0] - start[0]) * fraction,
            start[1] + (end[1] - start[1]) * fraction,
            start[2] + (end[2] - start[2]) * fraction,
        ];
        if let Some(color) = self.trace(node.children[side], start, mid) {
            return Some(color);
        }
        if let Some(color) = self.face_color(node, mid) {
            return Some(color);
        }
        self.trace(node.children[1 - side], mid, end)
    }

    /// Lightmap sample of the first face of `node` that contains `mid`.
    fn face_color(&self, node: &LightNode, mid: [f32; 3]) -> Option<[f32; 3]> {
        let range = node.first_face..node.first_face.saturating_add(node.num_faces);
        for face in self.faces.get(range)?.iter().flatten() {
            // C stores the projection in ints, truncating toward zero.
            let s = (dot(mid, [face.vecs[0][0], face.vecs[0][1], face.vecs[0][2]])
                + face.vecs[0][3]) as i32;
            let t = (dot(mid, [face.vecs[1][0], face.vecs[1][1], face.vecs[1][2]])
                + face.vecs[1][3]) as i32;
            if s < face.texture_mins[0] || t < face.texture_mins[1] {
                continue;
            }
            let (ds, dt) = (s - face.texture_mins[0], t - face.texture_mins[1]);
            if ds > face.extents[0] || dt > face.extents[1] {
                continue;
            }
            let Some(offset) = face.samples else {
                // Faces without a lightmap render fullbright in the world.
                return Some([1.0; 3]);
            };
            let columns = usize::try_from((face.extents[0] >> 4) + 1).ok()?;
            let luxel = usize::try_from(dt >> 4).ok()? * columns + usize::try_from(ds >> 4).ok()?;
            let at = offset.checked_add(luxel * 3)?;
            let rgb = self.lighting.get(at..at + 3)?;
            return Some([
                f32::from(rgb[0]) / 255.0,
                f32::from(rgb[1]) / 255.0,
                f32::from(rgb[2]) / 255.0,
            ]);
        }
        None
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bsp::{BspEdge, BspFace, BspNode, BspPlane, BspTexInfo, BspVertex};
    use q2_shared::types::Vec3f;

    /// One node splitting on z = 0 with two leaf children, and one face in
    /// that plane: s = x, t = y over (0,0)-(64,64), i.e. 5x5 luxels.
    fn floor_bsp(flags: i32, light_ofs: i32) -> BspData {
        let corners = [
            [0.0, 0.0, 0.0],
            [64.0, 0.0, 0.0],
            [64.0, 64.0, 0.0],
            [0.0, 64.0, 0.0],
        ];
        let vertices = corners
            .iter()
            .map(|c| BspVertex {
                position: Vec3f::new(c[0], c[1], c[2]),
            })
            .collect();
        let mut edges = vec![BspEdge { v: [0, 0] }];
        for k in 0..4u16 {
            edges.push(BspEdge {
                v: [k, (k + 1) % 4],
            });
        }
        // 5x5 luxels of (i, i, 0); luxel (ds=1, dt=2) is index 11.
        let lightmap_data = (0..25u8).flat_map(|i| [i * 10, i * 5, 0]).collect();
        BspData {
            planes: vec![BspPlane {
                normal: Vec3f::new(0.0, 0.0, 1.0),
                dist: 0.0,
                plane_type: 2,
            }],
            vertices,
            visibility: vec![],
            nodes: vec![BspNode {
                plane_idx: 0,
                children: [-1, -2],
                mins: [0; 3],
                maxs: [0; 3],
                first_face: 0,
                num_faces: 1,
            }],
            texinfo: vec![BspTexInfo {
                vecs: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]],
                flags,
                value: 0,
                texture: "t".into(),
                next_texinfo: -1,
            }],
            faces: vec![BspFace {
                plane_idx: 0,
                side: 0,
                first_edge: 0,
                num_edges: 4,
                texinfo_idx: 0,
                styles: [0, 255, 255, 255],
                light_ofs,
            }],
            lightmap_data,
            leafs: vec![],
            leaf_faces: vec![],
            edges,
            surface_edges: vec![1, 2, 3, 4],
            models: vec![],
        }
    }

    fn near(actual: [f32; 3], expected: [f32; 3]) {
        for axis in 0..3 {
            assert!(
                (actual[axis] - expected[axis]).abs() < 1e-5,
                "got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn samples_the_luxel_under_the_point() {
        let tree = LightTree::from_bsp(&floor_bsp(0, 0));
        // s = 20 -> ds 1, t = 36 -> dt 2 -> luxel 2*5+1 = 11 -> (110, 55, 0).
        near(
            tree.light_point([20.0, 36.0, 100.0], &[]),
            [110.0 / 255.0, 55.0 / 255.0, 0.0],
        );
    }

    #[test]
    fn face_extents_come_from_the_texture_projection() {
        let tree = LightTree::from_bsp(&floor_bsp(0, 0));
        let face = tree.faces[0].as_ref().expect("lit face");
        assert_eq!(face.extents, [64, 64]);
        assert_eq!(face.texture_mins, [0, 0]);
    }

    #[test]
    fn point_beside_the_face_hits_nothing_and_is_dark() {
        let tree = LightTree::from_bsp(&floor_bsp(0, 0));
        near(tree.light_point([200.0, 36.0, 100.0], &[]), [0.0; 3]);
    }

    #[test]
    fn point_below_the_plane_never_crosses_it() {
        let tree = LightTree::from_bsp(&floor_bsp(0, 0));
        near(tree.light_point([20.0, 36.0, -5.0], &[]), [0.0; 3]);
    }

    #[test]
    fn faces_without_samples_are_fullbright() {
        let tree = LightTree::from_bsp(&floor_bsp(0, -1));
        near(tree.light_point([20.0, 36.0, 100.0], &[]), [1.0; 3]);
    }

    #[test]
    fn warp_and_sky_faces_carry_no_light() {
        for flags in [SURF_WARP, SURF_SKY] {
            let tree = LightTree::from_bsp(&floor_bsp(flags, 0));
            near(tree.light_point([20.0, 36.0, 100.0], &[]), [0.0; 3]);
        }
    }

    #[test]
    fn maps_without_lighting_data_are_fullbright_even_with_dlights() {
        let mut bsp = floor_bsp(0, 0);
        bsp.lightmap_data.clear();
        let tree = LightTree::from_bsp(&bsp);
        let lamp = DLight {
            origin: Vec3f::new(20.0, 36.0, 100.0),
            color: Vec3f::new(1.0, 1.0, 1.0),
            intensity: 256.0,
        };
        near(tree.light_point([20.0, 36.0, 100.0], &[lamp]), [1.0; 3]);
    }

    #[test]
    fn dynamic_light_adds_intensity_falloff_over_256() {
        let tree = LightTree::from_bsp(&floor_bsp(0, -1));
        let lamp = DLight {
            origin: Vec3f::new(20.0, 36.0, 100.0 + 64.0),
            color: Vec3f::new(1.0, 0.5, 0.0),
            intensity: 192.0,
        };
        // (192 - 64) / 256 = 0.5 of the lamp colour on top of fullbright.
        near(
            tree.light_point([20.0, 36.0, 100.0], &[lamp]),
            [1.5, 1.25, 1.0],
        );
        let far = DLight {
            origin: Vec3f::new(20.0, 36.0, 1000.0),
            color: Vec3f::new(1.0, 1.0, 1.0),
            intensity: 100.0,
        };
        near(tree.light_point([20.0, 36.0, 100.0], &[far]), [1.0; 3]);
    }

    #[test]
    fn empty_tree_is_dark_not_a_panic() {
        let mut bsp = floor_bsp(0, 0);
        bsp.nodes.clear();
        near(
            LightTree::from_bsp(&bsp).light_point([0.0, 0.0, 10.0], &[]),
            [0.0; 3],
        );
    }
}
