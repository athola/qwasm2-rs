//! Sky box geometry and image lookup (C `GL3_SetSky`, `MakeSkyVec` and
//! `GL3_DrawSkyBox` in `gl3_warp.c`, `GetSkyImage` in `files/stb.c`).
//!
//! C clips world sky polygons against the box and draws only the covered
//! parts. Here the whole box is drawn first with depth testing off and the
//! world, whose sky faces are omitted, is drawn over it. The visible result is
//! the same because sky faces are the only holes in the world.

use super::texture::{FileLoader, Image};
use crate::files::{palette::Palette, pcx::load_pcx, tga::load_tga};

/// File name suffix per face, in the order C stores `sky_images`.
pub const SKY_SUFFIXES: [&str; 6] = ["rt", "bk", "lf", "ft", "up", "dn"];
/// C `skytexorder`: image used by box face `i`.
pub const SKY_TEX_ORDER: [usize; 6] = [0, 2, 1, 3, 4, 5];
/// Sky used when the map never calls `set_sky` (stock `unit1` environment).
pub const DEFAULT_SKY: &str = "unit1_";
/// C `dist` for `r_farsee == 0`.
const SKY_DIST: f32 = 2300.0;
/// C `sky_min` / `sky_max`: keep samples off the edge to avoid bilerp seams.
const SKY_MIN: f32 = 1.0 / 512.0;
const SKY_MAX: f32 = 511.0 / 512.0;

/// C `st_to_vec`: for each output axis, a 1-based source axis, negative to flip.
const ST_TO_VEC: [[i32; 3]; 6] = [
    [3, -1, 2],
    [-3, 1, 2],
    [1, 3, 2],
    [-1, -3, 2],
    [-2, -1, 3],
    [2, -1, -3],
];

/// Floats per vertex in the shared world layout: position(3) uv(2) lightmap uv(2).
pub const VERTEX_FLOATS: usize = 7;

/// Paths tried for one face, in C order: PCX then TGA.
pub fn sky_image_paths(sky: &str, suffix: &str) -> [String; 2] {
    [
        format!("env/{sky}{suffix}.pcx"),
        format!("env/{sky}{suffix}.tga"),
    ]
}

/// Load one sky face; `None` when neither file decodes.
pub fn load_sky_image(loader: &FileLoader, sky: &str, suffix: &str) -> Option<Image> {
    let [pcx_path, tga_path] = sky_image_paths(sky, suffix);
    if let Some(bytes) = loader(&pcx_path) {
        match load_pcx(&bytes) {
            Ok(pcx) => {
                if let Some(entries) = pcx.palette {
                    let palette = Palette(entries);
                    let mut rgba = Vec::with_capacity(pcx.pixels.len() * 4);
                    for &index in &pcx.pixels {
                        rgba.extend_from_slice(&palette.rgba(index));
                    }
                    return Some(Image {
                        width: pcx.width,
                        height: pcx.height,
                        rgba,
                        is_lava: false,
                    });
                }
                tracing::warn!("GL3: {pcx_path} has no palette");
            }
            Err(error) => tracing::warn!("GL3: bad {pcx_path}: {error:?}"),
        }
    }
    let bytes = loader(&tga_path)?;
    match load_tga(&bytes) {
        Ok(tga) => Some(Image {
            width: tga.width,
            height: tga.height,
            rgba: tga.rgba,
            is_lava: false,
        }),
        Err(error) => {
            tracing::warn!("GL3: bad {tga_path}: {error:?}");
            None
        }
    }
}

/// C `MakeSkyVec`: world-relative position and texture coordinate of box
/// corner `(s, t)` in `-1..=1` on face `axis`.
pub fn sky_vertex(s: f32, t: f32, axis: usize) -> ([f32; 3], [f32; 2]) {
    let b = [s * SKY_DIST, t * SKY_DIST, SKY_DIST];
    let mut position = [0.0; 3];
    for (out, &source) in position.iter_mut().zip(&ST_TO_VEC[axis]) {
        let value = b[usize::try_from(source.abs() - 1).unwrap_or(0)];
        *out = if source < 0 { -value } else { value };
    }
    let u = ((s + 1.0) * 0.5).clamp(SKY_MIN, SKY_MAX);
    let v = ((t + 1.0) * 0.5).clamp(SKY_MIN, SKY_MAX);
    (position, [u, 1.0 - v])
}

/// All six faces as triangles, face `i` occupying vertices `6*i..6*i+6`.
pub fn sky_box_vertices() -> Vec<f32> {
    let mut out = Vec::with_capacity(6 * 6 * VERTEX_FLOATS);
    for axis in 0..6 {
        // Corner order of C's GL_TRIANGLE_FAN for a full face.
        let corners = [(-1.0, -1.0), (-1.0, 1.0), (1.0, 1.0), (1.0, -1.0)];
        for index in [0, 1, 2, 0, 2, 3] {
            let (s, t) = corners[index];
            let (position, uv) = sky_vertex(s, t, axis);
            out.extend_from_slice(&position);
            out.extend_from_slice(&uv);
            out.extend_from_slice(&[0.0, 0.0]);
        }
    }
    out
}

/// Column-major rotation by `degrees` around `axis` (C `HMM_Rotate`);
/// identity for a zero-length axis.
pub fn rotation_matrix(degrees: f32, axis: [f32; 3]) -> [f32; 16] {
    let length = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let mut matrix = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    if length == 0.0 {
        return matrix;
    }
    let [x, y, z] = axis.map(|component| component / length);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let inv = 1.0 - cos;
    matrix[0] = x * x * inv + cos;
    matrix[1] = x * y * inv + z * sin;
    matrix[2] = x * z * inv - y * sin;
    matrix[4] = x * y * inv - z * sin;
    matrix[5] = y * y * inv + cos;
    matrix[6] = y * z * inv + x * sin;
    matrix[8] = x * z * inv + y * sin;
    matrix[9] = y * z * inv - x * sin;
    matrix[10] = z * z * inv + cos;
    matrix
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcx_with_palette(width: u16, height: u16, pixels: &[u8]) -> Vec<u8> {
        let mut file = vec![0u8; 128];
        file[0] = 0x0a;
        file[1] = 5;
        file[2] = 1;
        file[3] = 8;
        file[8..10].copy_from_slice(&(width - 1).to_le_bytes());
        file[10..12].copy_from_slice(&(height - 1).to_le_bytes());
        file.extend_from_slice(pixels); // raw bytes < 0xC0 are literal
        file.push(0x0c);
        file.extend((0..=255u8).flat_map(|i| [i, 0, 0]));
        file
    }

    #[test]
    fn face_paths_follow_env_sky_suffix_pattern() {
        assert_eq!(
            sky_image_paths("unit1_", "rt"),
            ["env/unit1_rt.pcx", "env/unit1_rt.tga"]
        );
    }

    #[test]
    fn suffixes_and_texture_order_match_c() {
        assert_eq!(SKY_SUFFIXES, ["rt", "bk", "lf", "ft", "up", "dn"]);
        assert_eq!(SKY_TEX_ORDER, [0, 2, 1, 3, 4, 5]);
    }

    #[test]
    fn pcx_is_preferred_over_tga() {
        let pcx = pcx_with_palette(2, 1, &[5, 6]);
        let loader: FileLoader = Box::new(move |path| {
            (path == "env/sky_up.pcx")
                .then(|| pcx.clone())
                .or_else(|| (path == "env/sky_up.tga").then(|| vec![0; 3]))
        });
        let image = load_sky_image(&loader, "sky_", "up").unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(&image.rgba[0..4], &[5, 0, 0, 255]);
    }

    #[test]
    fn tga_is_used_when_pcx_is_absent_or_corrupt() {
        let mut tga = vec![0u8; 18];
        tga[2] = 2;
        tga[12] = 1;
        tga[14] = 1;
        tga[16] = 24;
        tga[17] = 0x20;
        tga.extend_from_slice(&[3, 2, 1]);
        let tga_only = tga.clone();
        let loader: FileLoader =
            Box::new(move |path| path.ends_with(".tga").then(|| tga_only.clone()));
        assert_eq!(
            load_sky_image(&loader, "s", "dn").unwrap().rgba,
            vec![1, 2, 3, 255]
        );

        let corrupt_pcx: FileLoader = Box::new(move |path| {
            if path.ends_with(".pcx") {
                Some(vec![1; 40])
            } else {
                Some(tga.clone())
            }
        });
        assert_eq!(
            load_sky_image(&corrupt_pcx, "s", "dn").unwrap().rgba,
            vec![1, 2, 3, 255]
        );
    }

    #[test]
    fn missing_sky_image_is_none() {
        let loader: FileLoader = Box::new(|_| None);
        assert!(load_sky_image(&loader, "s", "rt").is_none());
    }

    #[test]
    fn sky_vertex_follows_st_to_vec_for_face_zero() {
        // axis 0: v = (b[2], -b[0], b[1]) = (dist, -s*dist, t*dist)
        let (position, uv) = sky_vertex(0.5, -0.25, 0);
        assert_eq!(position, [SKY_DIST, -0.5 * SKY_DIST, -0.25 * SKY_DIST]);
        assert!((uv[0] - 0.75).abs() < 1e-6);
        assert!((uv[1] - (1.0 - 0.375)).abs() < 1e-6);
    }

    #[test]
    fn sky_vertex_flips_axes_for_up_and_down_faces() {
        // axis 4: (-b[1], -b[0], b[2]); axis 5: (b[1], -b[0], -b[2])
        assert_eq!(
            sky_vertex(1.0, 2.0, 4).0,
            [-2.0 * SKY_DIST, -SKY_DIST, SKY_DIST]
        );
        assert_eq!(
            sky_vertex(1.0, 2.0, 5).0,
            [2.0 * SKY_DIST, -SKY_DIST, -SKY_DIST]
        );
    }

    #[test]
    fn sky_vertex_clamps_uv_off_the_texture_edge() {
        let (_, uv) = sky_vertex(-1.0, 1.0, 2);
        assert!((uv[0] - SKY_MIN).abs() < 1e-7);
        assert!((uv[1] - (1.0 - SKY_MAX)).abs() < 1e-7);
    }

    #[test]
    fn sky_box_has_six_faces_of_two_triangles() {
        let vertices = sky_box_vertices();
        assert_eq!(vertices.len(), 36 * VERTEX_FLOATS);
        // Every vertex lies on the cube surface: one coordinate is +-dist.
        for vertex in vertices.chunks(VERTEX_FLOATS) {
            assert!(vertex[..3].iter().any(|c| c.abs() == SKY_DIST));
        }
    }

    #[test]
    fn rotation_by_90_degrees_about_z_maps_x_to_y() {
        let m = rotation_matrix(90.0, [0.0, 0.0, 1.0]);
        // Column-major: first column is the image of +X.
        assert!(m[0].abs() < 1e-6 && (m[1] - 1.0).abs() < 1e-6 && m[2].abs() < 1e-6);
        assert!((m[5]).abs() < 1e-6 && (m[4] + 1.0).abs() < 1e-6);
        assert!((m[10] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn rotation_with_zero_axis_is_identity() {
        let m = rotation_matrix(45.0, [0.0; 3]);
        assert_eq!((m[0], m[5], m[10], m[15]), (1.0, 1.0, 1.0, 1.0));
        assert_eq!(m[1], 0.0);
    }
}
