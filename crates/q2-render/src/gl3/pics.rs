//! 2D drawing maths (C `gl3_draw.c`, `R_FindPic` in `ref_shared.c`). GL-free.
//!
//! Every 2D draw becomes one quad of [`UI_VERTEX_FLOATS`]-float vertices
//! (position(2), uv(2), colour(4)) in screen pixels, origin top-left.

pub const UI_VERTEX_FLOATS: usize = 8;
/// Two triangles per quad.
pub const QUAD_VERTICES: usize = 6;
/// Console font cell, `size = 0.0625` in C.
const CONCHARS_CELL: f32 = 0.0625;
pub const CONCHARS: &str = "conchars";
/// Edge of a console character in pixels (`scale` 1).
pub const CHAR_SIZE: i32 = 8;

/// File paths to try for a pic name, in order.
///
/// C `R_FindPic`: a leading `/` or `\` is dropped and the rest used as is;
/// anything else is `pics/<name>.pcx`. A name that already carries a path
/// (contains `/` or `\`) is also tried as given first, so full paths such as
/// `pics/inventory.pcx` work; names with an extension are never wrapped.
pub fn pic_paths(name: &str) -> Vec<String> {
    if let Some(stripped) = name.strip_prefix(['/', '\\']) {
        return vec![stripped.to_string()];
    }
    let wrapped = format!("pics/{name}.pcx");
    if !name.contains(['/', '\\']) {
        return vec![wrapped];
    }
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if file.contains('.') {
        vec![name.to_string()]
    } else {
        vec![name.to_string(), wrapped]
    }
}

/// Texture rectangle `[s0, t0, s1, t1]` of console character `ch` in the 16x16
/// grid, or `None` for a space (C skips `(num & 127) == 32`).
pub fn char_uv(ch: i32) -> Option<[f32; 4]> {
    let num = (ch & 255) as u32;
    if num & 127 == 32 {
        return None;
    }
    let (row, col) = ((num >> 4) as f32, (num & 15) as f32);
    let (s0, t0) = (col * CONCHARS_CELL, row * CONCHARS_CELL);
    Some([s0, t0, s0 + CONCHARS_CELL, t0 + CONCHARS_CELL])
}

/// Which texture a queued quad samples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadImage {
    /// Index into the [`ImageStore`].
    Pic(usize),
    /// 1x1 white texel, for fills.
    White,
}

/// A queued 2D draw.
#[derive(Debug, Clone, PartialEq)]
pub struct Quad {
    pub image: QuadImage,
    pub vertices: [f32; QUAD_VERTICES * UI_VERTEX_FLOATS],
}

/// Axis-aligned quad covering `(x, y)`..`(x + w, y + h)`.
pub fn quad(
    image: QuadImage,
    (x, y, w, h): (i32, i32, i32, i32),
    uv: [f32; 4],
    color: [f32; 4],
) -> Quad {
    let (left, top) = (x as f32, y as f32);
    let (right, bottom) = ((x + w) as f32, (y + h) as f32);
    let corner =
        |px: f32, py: f32, s: f32, t: f32| [px, py, s, t, color[0], color[1], color[2], color[3]];
    let top_left = corner(left, top, uv[0], uv[1]);
    let bottom_left = corner(left, bottom, uv[0], uv[3]);
    let top_right = corner(right, top, uv[2], uv[1]);
    let bottom_right = corner(right, bottom, uv[2], uv[3]);
    let mut vertices = [0.0; QUAD_VERTICES * UI_VERTEX_FLOATS];
    for (slot, vertex) in vertices.chunks_exact_mut(UI_VERTEX_FLOATS).zip([
        top_left,
        bottom_left,
        top_right,
        top_right,
        bottom_left,
        bottom_right,
    ]) {
        slot.copy_from_slice(&vertex);
    }
    Quad { image, vertices }
}

/// Fill quad for palette index `color` (C `Draw_Fill`); `None` for an index
/// outside 0..=255, where C raises a fatal error.
pub fn fill_quad(
    rect: (i32, i32, i32, i32),
    color: i32,
    rgb: impl Fn(u8) -> [u8; 3],
) -> Option<Quad> {
    let index = u8::try_from(color).ok()?;
    let [r, g, b] = rgb(index);
    let channel = |c: u8| f32::from(c) / 255.0;
    Some(quad(
        QuadImage::White,
        rect,
        [0.0; 4],
        [channel(r), channel(g), channel(b), 1.0],
    ))
}

/// Orthographic matrix for a `width` x `height` pixel canvas with the origin
/// at the top-left (C `HMM_Orthographic(0, w, h, 0, ...)`), column-major.
pub fn ortho_matrix(width: i32, height: i32) -> [f32; 16] {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    #[rustfmt::skip]
    let m = [
        2.0 / w, 0.0,      0.0, 0.0,
        0.0,    -2.0 / h,  0.0, 0.0,
        0.0,     0.0,     -1.0, 0.0,
       -1.0,     1.0,      0.0, 1.0,
    ];
    m
}

/// Consecutive quads sharing an image, as `(image, first vertex, vertex count)`.
pub fn batch_runs(quads: &[Quad]) -> Vec<(QuadImage, usize, usize)> {
    let mut runs: Vec<(QuadImage, usize, usize)> = Vec::new();
    for (index, quad) in quads.iter().enumerate() {
        match runs.last_mut() {
            Some((image, _, count)) if *image == quad.image => *count += QUAD_VERTICES,
            _ => runs.push((quad.image, index * QUAD_VERTICES, QUAD_VERTICES)),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_names_are_pcx_files_under_pics() {
        assert_eq!(pic_paths("conchars"), ["pics/conchars.pcx"]);
        assert_eq!(pic_paths("num_0"), ["pics/num_0.pcx"]);
    }

    #[test]
    fn names_with_a_path_are_tried_as_given_before_wrapping() {
        assert_eq!(pic_paths("pics/pause.pcx"), ["pics/pause.pcx"]);
        assert_eq!(pic_paths("misc\\x.tga"), ["misc\\x.tga"]);
        assert_eq!(
            pic_paths("misc/pause"),
            ["misc/pause", "pics/misc/pause.pcx"]
        );
    }

    #[test]
    fn a_leading_slash_names_an_exact_file() {
        assert_eq!(pic_paths("/pics/foo.pcx"), ["pics/foo.pcx"]);
        assert_eq!(pic_paths("\\env/sky.tga"), ["env/sky.tga"]);
    }

    #[test]
    fn console_chars_index_a_16_by_16_grid() {
        // 'A' = 65: row 4, column 1.
        assert_eq!(char_uv(65), Some([0.0625, 0.25, 0.125, 0.3125]));
        assert_eq!(char_uv(0), Some([0.0, 0.0, 0.0625, 0.0625]));
        // The last cell, and the high bit set (coloured text) wraps with & 255.
        assert_eq!(char_uv(255), Some([0.9375, 0.9375, 1.0, 1.0]));
        assert_eq!(char_uv(256 + 65), char_uv(65));
    }

    #[test]
    fn spaces_are_not_drawn_in_either_half_of_the_font() {
        assert_eq!(char_uv(32), None);
        assert_eq!(char_uv(32 + 128), None);
    }

    #[test]
    fn quad_covers_the_rectangle_with_matching_uv_corners() {
        let q = quad(
            QuadImage::Pic(3),
            (10, 20, 30, 40),
            [0.0, 0.25, 1.0, 0.75],
            [1.0, 0.5, 0.25, 1.0],
        );
        let vertex = |i: usize| &q.vertices[i * UI_VERTEX_FLOATS..(i + 1) * UI_VERTEX_FLOATS];
        // Top-left, bottom-left, top-right, then top-right, bottom-left, bottom-right.
        assert_eq!(vertex(0), &[10.0, 20.0, 0.0, 0.25, 1.0, 0.5, 0.25, 1.0]);
        assert_eq!(vertex(1)[0..4], [10.0, 60.0, 0.0, 0.75]);
        assert_eq!(vertex(2)[0..4], [40.0, 20.0, 1.0, 0.25]);
        assert_eq!(vertex(3), vertex(2));
        assert_eq!(vertex(4), vertex(1));
        assert_eq!(vertex(5)[0..4], [40.0, 60.0, 1.0, 0.75]);
    }

    #[test]
    fn fill_uses_the_palette_colour_and_rejects_bad_indices() {
        let rgb = |index: u8| [index, 255 - index, 0];
        let q = fill_quad((0, 0, 8, 8), 51, rgb).unwrap();
        assert_eq!(q.image, QuadImage::White);
        assert_eq!(&q.vertices[4..8], &[51.0 / 255.0, 204.0 / 255.0, 0.0, 1.0]);
        assert!(fill_quad((0, 0, 8, 8), 256, rgb).is_none());
        assert!(fill_quad((0, 0, 8, 8), -1, rgb).is_none());
    }

    #[test]
    fn ortho_maps_the_top_left_and_bottom_right_of_the_canvas_to_ndc_corners() {
        let m = ortho_matrix(640, 480);
        let ndc = |x: f32, y: f32| [m[0] * x + m[12], m[5] * y + m[13]];
        assert_eq!(ndc(0.0, 0.0), [-1.0, 1.0]);
        assert_eq!(ndc(640.0, 480.0), [1.0, -1.0]);
        assert_eq!(ndc(320.0, 240.0), [0.0, 0.0]);
    }

    #[test]
    fn consecutive_quads_with_one_image_share_a_run() {
        let q = |image| quad(image, (0, 0, 1, 1), [0.0; 4], [1.0; 4]);
        let quads = [
            q(QuadImage::Pic(1)),
            q(QuadImage::Pic(1)),
            q(QuadImage::White),
            q(QuadImage::Pic(1)),
        ];
        assert_eq!(
            batch_runs(&quads),
            vec![
                (QuadImage::Pic(1), 0, 12),
                (QuadImage::White, 12, 6),
                (QuadImage::Pic(1), 18, 6),
            ]
        );
        assert!(batch_runs(&[]).is_empty());
    }
}
