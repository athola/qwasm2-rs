//! Cache of standalone images: 2D pics and model skins (C `GL3_FindImage`
//! for `it_pic` / `it_skin`). Decoding is GL-free; upload happens only when a
//! context is given, so sizes can be queried without one.
//!
//! Entries carry the registration sequence they were last found in, so
//! `end_registration` can free skins the new map did not use (C
//! `GL3_FreeUnusedImages`). World textures are not in this store: they belong
//! to the world mesh and are freed with it.

use std::collections::{HashMap, VecDeque};

use glow::HasContext;
use q2_render_api::ImageHandle;

use super::texture::{upload_rgba, FileLoader, Image, Sampling, CLAMPED_SAMPLING, WORLD_SAMPLING};
use crate::files::palette::TRANSPARENT_INDEX;
use crate::files::{palette::Palette, pcx::load_pcx, tga::load_tga, wal::load_wal};

/// C `imagetype_t` for the images this store loads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    /// `it_pic`: 2D draws, no mipmaps.
    Pic,
    /// `it_skin`: MD2 skins, background flood-filled and mipmapped.
    Skin,
}

/// C `GL3_LoadPic` mipmaps everything but pics and skies.
pub fn sampling(kind: ImageKind) -> &'static Sampling {
    match kind {
        ImageKind::Pic => &CLAMPED_SAMPLING,
        ImageKind::Skin => &WORLD_SAMPLING,
    }
}

pub struct ImageEntry {
    pub width: usize,
    pub height: usize,
    /// `None` when no GL context was available at load time.
    pub texture: Option<glow::Texture>,
    kind: ImageKind,
    /// Registration sequence of the last lookup that returned it.
    sequence: u32,
}

/// `register_image` handle for store index `index` (`NONE` is 0).
pub fn image_handle(index: usize) -> ImageHandle {
    ImageHandle::new(i32::try_from(index + 1).unwrap_or(i32::MAX))
}

/// Store index named by a `register_image` handle.
pub fn handle_index(handle: ImageHandle) -> Option<usize> {
    usize::try_from(handle.raw()).ok()?.checked_sub(1)
}

pub struct ImageStore {
    /// Resolved path to entry index; failures are kept as `None` so a missing
    /// file is looked up and logged once.
    by_path: HashMap<String, Option<usize>>,
    /// Pic name to entry index, so per-character and HUD lookups skip
    /// building candidate paths after the first call.
    by_pic_name: HashMap<String, Option<usize>>,
    /// Freed entries stay as `None` so an old index never names a later image.
    entries: Vec<Option<ImageEntry>>,
    sequence: u32,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self {
            by_path: HashMap::new(),
            by_pic_name: HashMap::new(),
            entries: Vec::new(),
            sequence: 1,
        }
    }
}

/// C `FloodFillSkin`: the colour in the top-left corner is taken as the
/// skin's background, and the region of it connected to the corner is
/// replaced by a neighbouring opaque colour so mipmaps get no halo. C looks
/// for an opaque-black fill colour by comparing palette entries with
/// `0x000000FF`, which no Quake 2 palette entry matches, so it is index 0.
/// C's queue is a 4096-entry ring; this one grows instead of wrapping.
pub fn flood_fill_skin(skin: &mut [u8], width: usize, height: usize) {
    const FILLED_COLOR: u8 = 0;
    let Some(&fill_color) = skin.first() else {
        return;
    };
    // 255 marks visited pixels, so neither it nor the fill can be replaced.
    if fill_color == FILLED_COLOR || fill_color == TRANSPARENT_INDEX || skin.len() < width * height
    {
        return;
    }
    let mut queue = VecDeque::from([(0usize, 0usize)]);
    while let Some((x, y)) = queue.pop_front() {
        let mut found = FILLED_COLOR;
        let here = x + width * y;
        let mut step = |neighbour: usize, nx: usize, ny: usize| {
            if skin[neighbour] == fill_color {
                skin[neighbour] = TRANSPARENT_INDEX;
                queue.push_back((nx, ny));
            } else if skin[neighbour] != TRANSPARENT_INDEX {
                found = skin[neighbour];
            }
        };
        if x > 0 {
            step(here - 1, x - 1, y);
        }
        if x + 1 < width {
            step(here + 1, x + 1, y);
        }
        if y > 0 {
            step(here - width, x, y - 1);
        }
        if y + 1 < height {
            step(here + width, x, y + 1);
        }
        skin[here] = found;
    }
}

/// Decode a `.pcx`, `.tga` or `.wal` by extension. PCX and WAL pixels go
/// through the colormap palette like C's `GL3_Upload8` (index 255 is
/// transparent); a PCX falls back to its own palette when there is none.
/// 8-bit skins are flood-filled first, as C `GL3_LoadPic` does.
pub fn decode_image(
    loader: &FileLoader,
    palette: Option<&Palette>,
    path: &str,
    kind: ImageKind,
) -> Option<Image> {
    let bytes = loader(path)?;
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let fill = |pixels: &mut [u8], width: usize, height: usize| {
        if kind == ImageKind::Skin {
            flood_fill_skin(pixels, width, height);
        }
    };
    let decoded = match extension.as_str() {
        "pcx" => load_pcx(&bytes).and_then(|mut pcx| {
            let own_palette = pcx.palette.map(Palette);
            let palette = palette
                .or(own_palette.as_ref())
                .ok_or_else(|| crate::files::malformed("pcx", "no palette available"))?;
            fill(&mut pcx.pixels, pcx.width, pcx.height);
            Ok(Image {
                width: pcx.width,
                height: pcx.height,
                rgba: palette.indexed_to_rgba(&pcx.pixels, pcx.width),
                is_lava: false,
            })
        }),
        "tga" => load_tga(&bytes).map(|tga| Image {
            width: tga.width,
            height: tga.height,
            rgba: tga.rgba,
            is_lava: false,
        }),
        "wal" => match palette {
            Some(palette) => load_wal(&bytes).map(|mut wal| {
                fill(&mut wal.pixels, wal.width, wal.height);
                super::texture::wal_to_image(&wal, palette, path)
            }),
            None => Err(crate::files::malformed("wal", "no palette available")),
        },
        _ => Err(crate::files::malformed("image", "unsupported extension")),
    };
    match decoded {
        Ok(image) => Some(image),
        Err(error) => {
            tracing::warn!("GL3: bad image {path}: {error:?}");
            None
        }
    }
}

impl ImageStore {
    /// Index of the image at `path`, loading it on first use. `None` when it
    /// is missing or corrupt (logged once). The first lookup's `kind` decides
    /// how it is decoded and sampled, as C keys images by name alone.
    pub fn find(
        &mut self,
        gl: Option<&glow::Context>,
        loader: &FileLoader,
        palette: Option<&Palette>,
        path: &str,
        kind: ImageKind,
    ) -> Option<usize> {
        if let Some(&known) = self.by_path.get(path) {
            if let Some(index) = known {
                self.touch(index);
            }
            return known;
        }
        let loaded = decode_image(loader, palette, path, kind).and_then(|image| {
            let texture = match gl {
                Some(gl) => {
                    match upload_rgba(gl, image.width, image.height, &image.rgba, sampling(kind)) {
                        Ok(texture) => Some(texture),
                        Err(error) => {
                            tracing::error!("GL3: failed to upload {path}: {error}");
                            return None;
                        }
                    }
                }
                None => None,
            };
            Some(ImageEntry {
                width: image.width,
                height: image.height,
                texture,
                kind,
                sequence: self.sequence,
            })
        });
        if loaded.is_none() {
            tracing::warn!("GL3: can't find image {path}");
        }
        let index = loaded.map(|entry| {
            self.entries.push(Some(entry));
            self.entries.len() - 1
        });
        self.by_path.insert(path.to_string(), index);
        index
    }

    /// Index of the pic `name` resolves to (C `R_FindPic`), remembered by name.
    pub fn find_pic(
        &mut self,
        gl: Option<&glow::Context>,
        loader: &FileLoader,
        palette: Option<&Palette>,
        name: &str,
    ) -> Option<usize> {
        if let Some(&known) = self.by_pic_name.get(name) {
            return known;
        }
        let found = self.find_first(gl, loader, palette, &super::pics::pic_paths(name));
        self.by_pic_name.insert(name.to_string(), found);
        found
    }

    /// First of `paths` that loads as a pic (C `R_FindPic` tries several names).
    pub fn find_first(
        &mut self,
        gl: Option<&glow::Context>,
        loader: &FileLoader,
        palette: Option<&Palette>,
        paths: &[String],
    ) -> Option<usize> {
        paths
            .iter()
            .find_map(|path| self.find(gl, loader, palette, path, ImageKind::Pic))
    }

    pub fn get(&self, index: usize) -> Option<&ImageEntry> {
        self.entries.get(index)?.as_ref()
    }

    /// Mark an entry as used by the current sequence (C `GL3_FindImage` sets
    /// `registration_sequence` on every hit).
    pub fn touch(&mut self, index: usize) {
        if let Some(Some(entry)) = self.entries.get_mut(index) {
            entry.sequence = self.sequence;
        }
    }

    /// Start a registration sequence (C `registration_sequence++`).
    pub fn begin_sequence(&mut self) {
        self.sequence = self.sequence.wrapping_add(1);
    }

    /// C `GL3_FreeUnusedImages`: free every image not found in the current
    /// sequence, except pics. `gl` is `None` only for textures never uploaded.
    pub fn free_unused(&mut self, gl: Option<&glow::Context>) {
        let sequence = self.sequence;
        for slot in &mut self.entries {
            let unused = slot
                .as_ref()
                .is_some_and(|entry| entry.kind != ImageKind::Pic && entry.sequence != sequence);
            if !unused {
                continue;
            }
            let texture = slot.take().and_then(|entry| entry.texture);
            if let (Some(gl), Some(texture)) = (gl, texture) {
                // SAFETY: the texture was created on this `gl` by
                // `upload_rgba` in `find`, and `take` removed the only
                // reference to it.
                unsafe { gl.delete_texture(texture) };
            }
        }
        let entries = &self.entries;
        let live = |known: &mut Option<usize>| {
            known.is_none_or(|index| entries.get(index).is_some_and(Option::is_some))
        };
        self.by_path.retain(|_, known| live(known));
        self.by_pic_name.retain(|_, known| live(known));
    }

    /// Delete every texture and forget all images.
    pub fn free(&mut self, gl: &glow::Context) {
        for entry in self.entries.drain(..).flatten() {
            if let Some(texture) = entry.texture {
                // SAFETY: the texture was created on this `gl` by
                // `upload_rgba` in `find`, and `drain` removed the only
                // reference to it.
                unsafe { gl.delete_texture(texture) };
            }
        }
        self.by_path.clear();
        self.by_pic_name.clear();
    }
}

#[cfg(test)]
/// 2x2 PCX: header, RLE pixels [1, 255, 2, 3], palette entry i = (i, 0, 0).
pub(super) fn tiny_pcx() -> Vec<u8> {
    let mut bytes = vec![0u8; 128];
    bytes[0] = 0x0a;
    bytes[1] = 5;
    bytes[2] = 1;
    bytes[3] = 8;
    bytes[8..10].copy_from_slice(&1u16.to_le_bytes()); // xmax
    bytes[10..12].copy_from_slice(&1u16.to_le_bytes()); // ymax
                                                        // 255 and anything >= 0xC0 must be sent as a run of one.
    bytes.extend_from_slice(&[1, 0xC1, 255, 2, 3]);
    bytes.push(12);
    bytes.extend((0..=255u8).flat_map(|i| [i, 0, 0]));
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn loader_for(path: &'static str, bytes: Vec<u8>, hits: Rc<Cell<u32>>) -> FileLoader {
        Box::new(move |requested| {
            (requested == path).then(|| {
                hits.set(hits.get() + 1);
                bytes.clone()
            })
        })
    }

    #[test]
    fn pcx_goes_through_the_palette_and_255_is_transparent() {
        let hits = Rc::new(Cell::new(0));
        let loader = loader_for("pics/x.pcx", tiny_pcx(), hits);
        let mut entries = [[0u8; 3]; 256];
        for (i, entry) in entries.iter_mut().enumerate() {
            *entry = [0, i as u8, 0];
        }
        let image = decode_image(
            &loader,
            Some(&Palette(entries)),
            "pics/x.pcx",
            ImageKind::Pic,
        )
        .unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        // The global palette wins over the PCX's own (i, 0, 0).
        assert_eq!(&image.rgba[0..4], &[0, 1, 0, 255]);
        // Transparent, with the colour of the pixel below it (C Upload8).
        assert_eq!(&image.rgba[4..8], &[0, 3, 0, 0]);
    }

    #[test]
    fn skin_background_is_flood_filled_from_the_corner_like_c() {
        #[rustfmt::skip]
        let mut skin = [
            7, 7, 7,
            7, 3, 7,
        ];
        flood_fill_skin(&mut skin, 3, 2);
        assert_eq!(skin, [0, 3, 3, 0, 3, 3]);
    }

    #[test]
    fn skin_whose_corner_is_transparent_or_the_fill_colour_is_left_alone() {
        for corner in [255, 0] {
            let mut skin = [corner, corner, 5, corner];
            flood_fill_skin(&mut skin, 2, 2);
            assert_eq!(skin, [corner, corner, 5, corner]);
        }
    }

    #[test]
    fn skins_are_flood_filled_and_mipmapped_but_pics_are_not() {
        // tiny_pcx is [1, 255, 2, 3]: as a skin, the corner's colour (1) is
        // background and takes its opaque neighbour's colour (2).
        let loader = loader_for("s.pcx", tiny_pcx(), Rc::new(Cell::new(0)));
        let pic = decode_image(&loader, None, "s.pcx", ImageKind::Pic).unwrap();
        let skin = decode_image(&loader, None, "s.pcx", ImageKind::Skin).unwrap();
        assert_eq!(&pic.rgba[0..4], &[1, 0, 0, 255]);
        assert_eq!(&skin.rgba[0..4], &[2, 0, 0, 255]);
        assert!(sampling(ImageKind::Skin).mipmaps);
        assert!(!sampling(ImageKind::Pic).mipmaps);
    }

    #[test]
    fn pcx_without_a_global_palette_uses_its_own() {
        let loader = loader_for("a.pcx", tiny_pcx(), Rc::new(Cell::new(0)));
        let image = decode_image(&loader, None, "a.pcx", ImageKind::Pic).unwrap();
        assert_eq!(&image.rgba[0..4], &[1, 0, 0, 255]);
    }

    #[test]
    fn unknown_extension_and_missing_file_decode_to_none() {
        let loader = loader_for("a.bmp", vec![1, 2, 3], Rc::new(Cell::new(0)));
        assert!(decode_image(&loader, None, "a.bmp", ImageKind::Pic).is_none());
        assert!(decode_image(&loader, None, "gone.pcx", ImageKind::Pic).is_none());
    }

    #[test]
    fn find_caches_hits_and_misses_without_a_gl_context() {
        let hits = Rc::new(Cell::new(0));
        let loader = loader_for("pics/x.pcx", tiny_pcx(), Rc::clone(&hits));
        let mut store = ImageStore::default();
        let first = store
            .find(None, &loader, None, "pics/x.pcx", ImageKind::Pic)
            .unwrap();
        let again = store
            .find(None, &loader, None, "pics/x.pcx", ImageKind::Pic)
            .unwrap();
        assert_eq!(first, again);
        assert_eq!(hits.get(), 1);
        let entry = store.get(first).unwrap();
        assert_eq!((entry.width, entry.height), (2, 2));
        assert!(entry.texture.is_none());

        assert!(store
            .find(None, &loader, None, "pics/nope.pcx", ImageKind::Pic)
            .is_none());
        assert!(store
            .find(None, &loader, None, "pics/nope.pcx", ImageKind::Pic)
            .is_none());
        assert_eq!(store.entries.len(), 1);
    }

    #[test]
    fn pic_names_resolve_once() {
        let hits = Rc::new(Cell::new(0));
        let loader = loader_for("pics/x.pcx", tiny_pcx(), Rc::clone(&hits));
        let mut store = ImageStore::default();
        let first = store.find_pic(None, &loader, None, "x");
        assert!(first.is_some());
        assert_eq!(store.find_pic(None, &loader, None, "x"), first);
        assert_eq!(store.by_pic_name.len(), 1);
        assert!(store.find_pic(None, &loader, None, "missing").is_none());
        assert_eq!(hits.get(), 1);
    }

    fn three_pcx_loader(hits: Rc<Cell<u32>>) -> FileLoader {
        let pcx = tiny_pcx();
        Box::new(move |path| {
            matches!(path, "a.pcx" | "b.pcx" | "p.pcx").then(|| {
                hits.set(hits.get() + 1);
                pcx.clone()
            })
        })
    }

    #[test]
    fn free_unused_frees_untouched_skins_but_keeps_pics_and_touched_skins() {
        let hits = Rc::new(Cell::new(0));
        let loader = three_pcx_loader(Rc::clone(&hits));
        let mut store = ImageStore::default();
        let a = store.find(None, &loader, None, "a.pcx", ImageKind::Skin);
        let b = store.find(None, &loader, None, "b.pcx", ImageKind::Skin);
        let p = store.find(None, &loader, None, "p.pcx", ImageKind::Pic);
        let (a, b, p) = (a.unwrap(), b.unwrap(), p.unwrap());
        store.begin_sequence();
        store.touch(b);
        store.free_unused(None);
        assert!(store.get(a).is_none());
        assert!(store.get(b).is_some());
        // C GL3_FreeUnusedImages: "don't free pics".
        assert!(store.get(p).is_some());
        // A freed path loads again into a new slot: models hold raw indices,
        // so a stale one must never name a different image.
        let reloaded = store.find(None, &loader, None, "a.pcx", ImageKind::Skin);
        assert_ne!(reloaded, Some(a));
        assert!(reloaded.is_some());
        assert_eq!(hits.get(), 4);
    }

    #[test]
    fn find_of_a_cached_image_touches_it_for_the_current_sequence() {
        let loader = three_pcx_loader(Rc::new(Cell::new(0)));
        let mut store = ImageStore::default();
        let a = store.find(None, &loader, None, "a.pcx", ImageKind::Skin);
        store.begin_sequence();
        assert_eq!(store.find(None, &loader, None, "a.pcx", ImageKind::Skin), a);
        store.free_unused(None);
        assert!(store.get(a.unwrap()).is_some());
    }

    #[test]
    fn image_handles_are_index_plus_one_and_none_is_no_image() {
        assert_eq!(image_handle(0).raw(), 1);
        assert_eq!(handle_index(image_handle(6)), Some(6));
        assert_eq!(handle_index(ImageHandle::NONE), None);
        assert_eq!(handle_index(ImageHandle::new(-2)), None);
    }

    #[test]
    fn find_first_takes_the_first_path_that_loads() {
        let loader = loader_for("second.pcx", tiny_pcx(), Rc::new(Cell::new(0)));
        let mut store = ImageStore::default();
        let paths = ["first.pcx".to_string(), "second.pcx".to_string()];
        let index = store.find_first(None, &loader, None, &paths).unwrap();
        assert_eq!(store.get(index).unwrap().width, 2);
        assert!(store.find_first(None, &loader, None, &paths[..1]).is_none());
    }
}
