//! Cache of standalone images: 2D pics and model skins (C `GL3_FindImage`
//! for `it_pic` / `it_skin`). Decoding is GL-free; upload happens only when a
//! context is given, so sizes can be queried without one.

use std::collections::HashMap;

use glow::HasContext;

use super::texture::{upload_rgba, FileLoader, Image, CLAMPED_SAMPLING};
use crate::files::{palette::Palette, pcx::load_pcx, tga::load_tga, wal::load_wal};

pub struct ImageEntry {
    pub width: usize,
    pub height: usize,
    /// `None` when no GL context was available at load time.
    pub texture: Option<glow::Texture>,
}

#[derive(Default)]
pub struct ImageStore {
    /// Resolved path to entry index; failures are kept as `None` so a missing
    /// file is looked up and logged once.
    by_path: HashMap<String, Option<usize>>,
    pub entries: Vec<ImageEntry>,
}

/// Decode a `.pcx`, `.tga` or `.wal` by extension. PCX and WAL pixels go
/// through the colormap palette like C's `d_8to24table` (index 255 is
/// transparent); a PCX falls back to its own palette when there is none.
pub fn decode_image(loader: &FileLoader, palette: Option<&Palette>, path: &str) -> Option<Image> {
    let bytes = loader(path)?;
    let extension = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let decoded = match extension.as_str() {
        "pcx" => load_pcx(&bytes).and_then(|pcx| {
            let own_palette = pcx.palette.map(Palette);
            let palette = palette
                .or(own_palette.as_ref())
                .ok_or_else(|| crate::files::malformed("pcx", "no palette available"))?;
            let rgba = pcx
                .pixels
                .iter()
                .flat_map(|&index| palette.rgba(index))
                .collect();
            Ok(Image {
                width: pcx.width,
                height: pcx.height,
                rgba,
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
            Some(palette) => {
                load_wal(&bytes).map(|wal| super::texture::wal_to_image(&wal, palette, path))
            }
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
    /// is missing or corrupt (logged once).
    pub fn find(
        &mut self,
        gl: Option<&glow::Context>,
        loader: &FileLoader,
        palette: Option<&Palette>,
        path: &str,
    ) -> Option<usize> {
        if let Some(&known) = self.by_path.get(path) {
            return known;
        }
        let loaded = decode_image(loader, palette, path).and_then(|image| {
            let texture = match gl {
                Some(gl) => match upload_rgba(
                    gl,
                    image.width,
                    image.height,
                    &image.rgba,
                    &CLAMPED_SAMPLING,
                ) {
                    Ok(texture) => Some(texture),
                    Err(error) => {
                        tracing::error!("GL3: failed to upload {path}: {error}");
                        return None;
                    }
                },
                None => None,
            };
            Some(ImageEntry {
                width: image.width,
                height: image.height,
                texture,
            })
        });
        if loaded.is_none() {
            tracing::warn!("GL3: can't find image {path}");
        }
        let index = loaded.map(|entry| {
            self.entries.push(entry);
            self.entries.len() - 1
        });
        self.by_path.insert(path.to_string(), index);
        index
    }

    /// First of `paths` that loads (C `R_FindPic` tries several names).
    pub fn find_first(
        &mut self,
        gl: Option<&glow::Context>,
        loader: &FileLoader,
        palette: Option<&Palette>,
        paths: &[String],
    ) -> Option<usize> {
        paths
            .iter()
            .find_map(|path| self.find(gl, loader, palette, path))
    }

    /// Delete every texture and forget all images.
    pub fn free(&mut self, gl: &glow::Context) {
        for entry in self.entries.drain(..) {
            if let Some(texture) = entry.texture {
                // SAFETY: the texture was created on this `gl` by
                // `upload_rgba` in `find`, and `drain` removed the only
                // reference to it.
                unsafe { gl.delete_texture(texture) };
            }
        }
        self.by_path.clear();
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
        let image = decode_image(&loader, Some(&Palette(entries)), "pics/x.pcx").unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        // The global palette wins over the PCX's own (i, 0, 0).
        assert_eq!(&image.rgba[0..4], &[0, 1, 0, 255]);
        assert_eq!(&image.rgba[4..8], &[0, 255, 0, 0]);
    }

    #[test]
    fn pcx_without_a_global_palette_uses_its_own() {
        let loader = loader_for("a.pcx", tiny_pcx(), Rc::new(Cell::new(0)));
        let image = decode_image(&loader, None, "a.pcx").unwrap();
        assert_eq!(&image.rgba[0..4], &[1, 0, 0, 255]);
    }

    #[test]
    fn unknown_extension_and_missing_file_decode_to_none() {
        let loader = loader_for("a.bmp", vec![1, 2, 3], Rc::new(Cell::new(0)));
        assert!(decode_image(&loader, None, "a.bmp").is_none());
        assert!(decode_image(&loader, None, "gone.pcx").is_none());
    }

    #[test]
    fn find_caches_hits_and_misses_without_a_gl_context() {
        let hits = Rc::new(Cell::new(0));
        let loader = loader_for("pics/x.pcx", tiny_pcx(), Rc::clone(&hits));
        let mut store = ImageStore::default();
        let first = store.find(None, &loader, None, "pics/x.pcx").unwrap();
        let again = store.find(None, &loader, None, "pics/x.pcx").unwrap();
        assert_eq!(first, again);
        assert_eq!(hits.get(), 1);
        assert_eq!(
            (store.entries[first].width, store.entries[first].height),
            (2, 2)
        );
        assert!(store.entries[first].texture.is_none());

        assert!(store.find(None, &loader, None, "pics/nope.pcx").is_none());
        assert!(store.find(None, &loader, None, "pics/nope.pcx").is_none());
        assert_eq!(store.entries.len(), 1);
    }

    #[test]
    fn find_first_takes_the_first_path_that_loads() {
        let loader = loader_for("second.pcx", tiny_pcx(), Rc::new(Cell::new(0)));
        let mut store = ImageStore::default();
        let paths = ["first.pcx".to_string(), "second.pcx".to_string()];
        let index = store.find_first(None, &loader, None, &paths).unwrap();
        assert_eq!(store.entries[index].width, 2);
        assert!(store.find_first(None, &loader, None, &paths[..1]).is_none());
    }
}
