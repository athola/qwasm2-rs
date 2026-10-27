//! World texture loading: `.wal` through the colormap palette to RGBA, and the
//! GL upload (C `GL3_FindImage` / `LoadWal` / `GL3_Upload32` in `gl3_image.c`).

use std::collections::HashMap;

use glow::HasContext;

use crate::bsp::BspTexInfo;
use crate::files::palette::Palette;
use crate::files::wal::{load_wal, WalTexture};

/// C `ri.FS_LoadFile`: bytes of a game file, `None` when it does not exist.
pub type FileLoader = Box<dyn Fn(&str) -> Option<Vec<u8>>>;

const PALETTE_PATH: &str = "pics/colormap.pcx";
const NOTEXTURE_SIZE: usize = 8;

/// CPU-side RGBA8 texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    /// C `image->is_lava`: name contains "lava"; lava is not dimmed like water.
    pub is_lava: bool,
}

/// Read the 256-colour palette used by every `.wal` (C `GL3_GetPalette`).
pub fn load_palette(loader: &FileLoader) -> Option<Palette> {
    let bytes = loader(PALETTE_PATH)?;
    match Palette::from_pcx(&bytes) {
        Ok(palette) => Some(palette),
        Err(error) => {
            tracing::error!("GL3: bad {PALETTE_PATH}: {error:?}");
            None
        }
    }
}

/// Index every pixel through the palette; index 255 becomes alpha 0 (C `d_8to24table`).
pub fn wal_to_image(wal: &WalTexture, palette: &Palette, name: &str) -> Image {
    let mut rgba = Vec::with_capacity(wal.pixels.len() * 4);
    for &index in &wal.pixels {
        rgba.extend_from_slice(&palette.rgba(index));
    }
    Image {
        width: wal.width,
        height: wal.height,
        rgba,
        is_lava: name.contains("lava"),
    }
}

/// Magenta/black checker shown for textures that fail to load (C `gl3_notexture`).
pub fn notexture() -> Image {
    let mut rgba = Vec::with_capacity(NOTEXTURE_SIZE * NOTEXTURE_SIZE * 4);
    for y in 0..NOTEXTURE_SIZE {
        for x in 0..NOTEXTURE_SIZE {
            let on = (x / 4 + y / 4) % 2 == 0;
            let color = if on {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            rgba.extend_from_slice(&color);
        }
    }
    Image {
        width: NOTEXTURE_SIZE,
        height: NOTEXTURE_SIZE,
        rgba,
        is_lava: false,
    }
}

/// Distinct images for a map plus the image each texinfo uses.
pub struct WorldImages {
    /// Index 0 is always [`notexture`].
    pub images: Vec<Image>,
    /// Parallel to the BSP texinfo array.
    pub texinfo_image: Vec<usize>,
}

/// Load `textures/<name>.wal` once per distinct name. A missing or corrupt
/// texture maps to the notexture image and is logged once.
pub fn load_world_images(
    texinfo: &[BspTexInfo],
    loader: &FileLoader,
    palette: Option<&Palette>,
) -> WorldImages {
    let mut images = vec![notexture()];
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    let mut texinfo_image = Vec::with_capacity(texinfo.len());

    for info in texinfo {
        let name = info.texture.as_str();
        let index = *by_name
            .entry(name)
            .or_insert_with(|| match load_one(name, loader, palette) {
                Some(image) => {
                    images.push(image);
                    images.len() - 1
                }
                None => 0,
            });
        texinfo_image.push(index);
    }
    WorldImages {
        images,
        texinfo_image,
    }
}

fn load_one(name: &str, loader: &FileLoader, palette: Option<&Palette>) -> Option<Image> {
    let path = format!("textures/{name}.wal");
    let Some(bytes) = loader(&path) else {
        tracing::warn!("GL3: can't find {path}, using notexture");
        return None;
    };
    let Some(palette) = palette else {
        tracing::warn!("GL3: no palette, {path} replaced by notexture");
        return None;
    };
    match load_wal(&bytes) {
        Ok(wal) => Some(wal_to_image(&wal, palette, name)),
        Err(error) => {
            tracing::warn!("GL3: bad {path} ({error:?}), using notexture");
            None
        }
    }
}

/// How an uploaded texture is wrapped and filtered.
pub struct Sampling {
    pub wrap: u32,
    pub min_filter: u32,
    pub mipmaps: bool,
}

/// World diffuse maps: REPEAT, C default `GL_LINEAR_MIPMAP_NEAREST`.
pub const WORLD_SAMPLING: Sampling = Sampling {
    wrap: glow::REPEAT,
    min_filter: glow::LINEAR_MIPMAP_NEAREST,
    mipmaps: true,
};

/// Lightmap pages and sky faces: bilinear, no mipmaps, no wrap.
pub const CLAMPED_SAMPLING: Sampling = Sampling {
    wrap: glow::CLAMP_TO_EDGE,
    min_filter: glow::LINEAR,
    mipmaps: false,
};

/// Upload RGBA8 pixels as a new 2D texture, leaving it bound to the active unit.
pub fn upload_rgba(
    gl: &glow::Context,
    width: usize,
    height: usize,
    rgba: &[u8],
    sampling: &Sampling,
) -> Result<glow::Texture, String> {
    let (Ok(gl_width), Ok(gl_height)) = (i32::try_from(width), i32::try_from(height)) else {
        return Err("texture too large".into());
    };
    if rgba.len() != width * height * 4 {
        return Err(format!(
            "{} bytes for a {width}x{height} texture",
            rgba.len()
        ));
    }
    // SAFETY: the texture is created on `gl` and bound before the parameter
    // and upload calls; `rgba` is a live slice of exactly `width * height * 4`
    // bytes (checked above) that outlives the `tex_image_2d` call.
    unsafe {
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, sampling.wrap as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, sampling.wrap as i32);
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            sampling.min_filter as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            gl_width,
            gl_height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(rgba)),
        );
        if sampling.mipmaps {
            gl.generate_mipmap(glow::TEXTURE_2D);
        }
        Ok(texture)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn gradient_palette() -> Palette {
        let mut entries = [[0u8; 3]; 256];
        for (i, entry) in entries.iter_mut().enumerate() {
            *entry = [i as u8, 0, 255 - i as u8];
        }
        Palette(entries)
    }

    fn wal_bytes(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0u8; 100];
        bytes[32..36].copy_from_slice(&width.to_le_bytes());
        bytes[36..40].copy_from_slice(&height.to_le_bytes());
        bytes[40..44].copy_from_slice(&100u32.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes
    }

    fn texinfo(name: &str) -> BspTexInfo {
        BspTexInfo {
            vecs: [[0.0; 4]; 2],
            flags: 0,
            value: 0,
            texture: name.into(),
            next_texinfo: -1,
        }
    }

    #[test]
    fn wal_pixels_go_through_palette_and_index_255_is_transparent() {
        let wal = load_wal(&wal_bytes(2, 1, &[1, 255])).unwrap();
        let image = wal_to_image(&wal, &gradient_palette(), "e1u1/x");
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, vec![1, 0, 254, 255, 255, 0, 0, 0]);
        assert!(!image.is_lava);
    }

    #[test]
    fn lava_textures_are_recognised_by_name() {
        let wal = load_wal(&wal_bytes(1, 1, &[0])).unwrap();
        assert!(wal_to_image(&wal, &gradient_palette(), "e1u1/lava").is_lava);
    }

    #[test]
    fn notexture_is_an_opaque_two_colour_checker() {
        let image = notexture();
        assert_eq!(image.rgba.len(), image.width * image.height * 4);
        assert_eq!(&image.rgba[0..4], &[255, 0, 255, 255]);
        // Pixel (4, 0) is in the next 4x4 block, so the other colour.
        assert_eq!(&image.rgba[16..20], &[0, 0, 0, 255]);
        assert!(image.rgba.chunks(4).all(|px| px[3] == 255));
    }

    #[test]
    fn world_images_dedupe_by_name_and_fall_back_when_missing() {
        let requested = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&requested);
        let loader: FileLoader = Box::new(move |path| {
            log.lock().unwrap().push(path.to_string());
            (path == "textures/a/wall.wal").then(|| wal_bytes(1, 1, &[7]))
        });
        let infos = [
            texinfo("a/wall"),
            texinfo("a/missing"),
            texinfo("a/wall"),
            texinfo("a/missing"),
        ];
        let loaded = load_world_images(&infos, &loader, Some(&gradient_palette()));
        assert_eq!(loaded.texinfo_image, vec![1, 0, 1, 0]);
        assert_eq!(loaded.images.len(), 2);
        assert_eq!(loaded.images[0], notexture());
        // Each distinct name is fetched (and so logged) exactly once.
        assert_eq!(requested.lock().unwrap().len(), 2);
    }

    #[test]
    fn corrupt_wal_and_missing_palette_fall_back_to_notexture() {
        let loader: FileLoader = Box::new(|path| {
            if path.ends_with("bad.wal") {
                Some(vec![0; 10])
            } else {
                Some(wal_bytes(1, 1, &[7]))
            }
        });
        let infos = [texinfo("bad"), texinfo("good")];
        let with_palette = load_world_images(&infos, &loader, Some(&gradient_palette()));
        assert_eq!(with_palette.texinfo_image, vec![0, 1]);
        let without_palette = load_world_images(&infos, &loader, None);
        assert_eq!(without_palette.texinfo_image, vec![0, 0]);
    }

    #[test]
    fn palette_loads_from_colormap_and_rejects_garbage() {
        let mut pcx = vec![0u8; 200];
        pcx.extend((0..=255u8).flat_map(|i| [i, i, i]));
        let good: FileLoader = Box::new(move |path| (path == PALETTE_PATH).then(|| pcx.clone()));
        assert_eq!(load_palette(&good).unwrap().0[9], [9, 9, 9]);
        let bad: FileLoader = Box::new(|_| Some(vec![1, 2, 3]));
        assert!(load_palette(&bad).is_none());
        let absent: FileLoader = Box::new(|_| None);
        assert!(load_palette(&absent).is_none());
    }
}
