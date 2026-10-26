//! Virtual filesystem with PAK archive support.
//!
//! Replaces C `filesystem.c`. Provides:
//! - PAK file loading (Quake 2 `.pak` format)
//! - Directory-based search paths
//! - Unified `load_file` / `open_file` interface
//! - Case-insensitive filename matching
//!
//! # Backend design
//!
//! `PakReader` abstracts the source of raw bytes.  Native builds use
//! `DiskPakReader` (file per-read).  The WASM build uses `JsPakReader`
//! (defined in `q2-platform`) which holds the full PAK as a JS-heap
//! `Uint8Array` and slices individual assets into WASM memory on demand —
//! avoiding an upfront 100 MB copy.
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0

use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Q2Error, Q2Result};

// PAK file magic: "PACK" as little-endian u32
const PAK_MAGIC: u32 =
    (b'P' as u32) | ((b'A' as u32) << 8) | ((b'C' as u32) << 16) | ((b'K' as u32) << 24);

/// PAK filename field length — fixed at 56 bytes (null-padded) per the Quake
/// PAK format spec. Total directory entry = 56 name + 4 offset + 4 size = 64.
const PAK_ENTRY_NAME_LEN: usize = 56;
const PAK_DIR_ENTRY_SIZE: u32 = 64;

/// Maximum files per PAK
const MAX_FILES_IN_PACK: usize = 4096;

/// Numbered paks `pak0`..`pak99` are searched in numeric order (C `MAX_PAKS`).
const MAX_PAKS: u32 = 100;

// ---------------------------------------------------------------------------
// PakReader trait
// ---------------------------------------------------------------------------

/// Source of raw bytes for a PAK archive.
///
/// Implementations can back a PAK with a disk file, an in-memory `Vec<u8>`,
/// or (on WASM) a JS-heap `Uint8Array` — the latter avoids copying the full
/// PAK into Rust memory until individual assets are requested.
pub trait PakReader {
    /// Copy bytes `[offset, offset + len)` from the PAK source into a new
    /// `Vec<u8>`.  Implementations should validate bounds and return
    /// `Q2Error::Drop` on failure.
    fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>>;
}

// ---------------------------------------------------------------------------
// Bundled backend: disk (native)
// ---------------------------------------------------------------------------

struct DiskPakReader {
    path: PathBuf,
}

impl PakReader for DiskPakReader {
    fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
        let file = std::fs::File::open(&self.path)
            .map_err(|e| Q2Error::Drop(format!("open {}: {}", self.path.display(), e)))?;
        // Bound the request by the real file size before allocating: `len`
        // comes from untrusted PAK header/directory fields.
        let file_len = file
            .metadata()
            .map_err(|e| Q2Error::Drop(format!("stat {}: {}", self.path.display(), e)))?
            .len();
        if u64::from(offset) + u64::from(len) > file_len {
            return Err(Q2Error::Drop(format!(
                "{}: read [{offset}, +{len}) past end of file ({file_len} bytes)",
                self.path.display()
            )));
        }
        let mut reader = io::BufReader::new(file);
        reader
            .seek(SeekFrom::Start(offset as u64))
            .map_err(|e| Q2Error::Drop(format!("seek in {}: {}", self.path.display(), e)))?;
        let mut buf = vec![0u8; len as usize];
        reader
            .read_exact(&mut buf)
            .map_err(|e| Q2Error::Drop(format!("read from {}: {}", self.path.display(), e)))?;
        Ok(buf)
    }
}

// ---------------------------------------------------------------------------
// Bundled backend: in-memory (tests + legacy load_from_bytes)
// ---------------------------------------------------------------------------

struct InMemPakReader {
    data: Vec<u8>,
}

impl PakReader for InMemPakReader {
    fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
        let start = offset as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or_else(|| Q2Error::Drop("InMemPakReader: offset overflow".into()))?;
        if end > self.data.len() {
            return Err(Q2Error::Drop(format!(
                "InMemPakReader: read [{start}, {end}) out of bounds (len={})",
                self.data.len()
            )));
        }
        Ok(self.data[start..end].to_vec())
    }
}

// ---------------------------------------------------------------------------
// PackFile
// ---------------------------------------------------------------------------

/// A file entry within a PAK archive.
#[derive(Debug, Clone)]
pub struct PackFile {
    /// Filename (normalized: lowercase, forward slashes)
    pub name: String,
    /// Offset within the PAK file
    pub offset: u32,
    /// Uncompressed size
    pub size: u32,
}

// ---------------------------------------------------------------------------
// Pack
// ---------------------------------------------------------------------------

/// A loaded PAK archive.
///
/// The directory index is always in Rust memory.  Asset bytes are fetched
/// on demand through the `PakReader` backend — for WASM builds this means
/// individual files are sliced from a JS-heap buffer rather than pre-copied.
pub struct Pack {
    /// Display name (path for disk packs, URL or name for WASM packs).
    pub name: String,
    pub(crate) files: HashMap<String, PackFile>,
    reader: Box<dyn PakReader>,
}

impl std::fmt::Debug for Pack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pack")
            .field("name", &self.name)
            .field("file_count", &self.files.len())
            .finish_non_exhaustive()
    }
}

impl Pack {
    /// Number of files indexed in this pack.
    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    /// Open a PAK from any `PakReader` backend.
    ///
    /// Reads only the header (12 bytes) and directory to build the index.
    /// Asset bytes are not loaded until `read_file` is called.
    pub fn open(name: &str, reader: Box<dyn PakReader>) -> Q2Result<Self> {
        // Parse header
        let header = reader.read_slice(0, 12)?;
        let mut cur = Cursor::new(&header);
        let magic = read_u32_le(&mut cur)?;
        if magic != PAK_MAGIC {
            return Err(Q2Error::Drop(format!(
                "{name}: not a PAK file (magic: {magic:#010x})"
            )));
        }
        let dir_offset = read_u32_le(&mut cur)?;
        let dir_len = read_u32_le(&mut cur)?;

        // Validate the entry count before reading the directory so a forged
        // dir_len cannot force a huge allocation (C FS_LoadPAK checks first).
        if !dir_len.is_multiple_of(PAK_DIR_ENTRY_SIZE) {
            return Err(Q2Error::Drop(format!(
                "{name}: invalid PAK directory length {dir_len}"
            )));
        }
        let num_files = (dir_len / PAK_DIR_ENTRY_SIZE) as usize;
        if num_files == 0 {
            return Err(Q2Error::Drop(format!("{name}: PAK directory is empty")));
        }
        if num_files > MAX_FILES_IN_PACK {
            return Err(Q2Error::Drop(format!(
                "{name}: too many files ({num_files} > {MAX_FILES_IN_PACK})"
            )));
        }

        // Parse directory
        let dir_bytes = reader.read_slice(dir_offset, dir_len)?;
        let files = parse_pack_directory(name, &dir_bytes)?;

        tracing::info!("{name}: {} files indexed", files.len());
        Ok(Pack {
            name: name.to_string(),
            files,
            reader,
        })
    }

    /// Load a PAK file from disk.
    pub fn load(path: &Path) -> Q2Result<Self> {
        let display = path.display().to_string();
        Pack::open(
            &display,
            Box::new(DiskPakReader {
                path: path.to_path_buf(),
            }),
        )
    }

    /// Load a PAK from an in-memory byte buffer.
    ///
    /// Copies the buffer into an `InMemPakReader`.  On WASM, prefer
    /// `Pack::open` with a `JsPakReader` (from `q2-platform`) to keep the
    /// full PAK in the JS heap and only copy individual assets on demand.
    pub fn load_from_bytes(name: &str, data: &[u8]) -> Q2Result<Self> {
        Pack::open(
            name,
            Box::new(InMemPakReader {
                data: data.to_vec(),
            }),
        )
    }

    /// Read a file from this PAK archive.
    pub fn read_file(&self, entry: &PackFile) -> Q2Result<Vec<u8>> {
        self.reader.read_slice(entry.offset, entry.size)
    }
}

// ---------------------------------------------------------------------------
// SearchPath / FileSystem
// ---------------------------------------------------------------------------

/// A search path entry — either a directory or a PAK file.
#[derive(Debug)]
pub enum SearchPath {
    Directory(PathBuf),
    Pack(Pack),
}

/// The virtual filesystem.
#[derive(Debug)]
pub struct FileSystem {
    /// Search paths, searched in order (last added = highest priority)
    search_paths: Vec<SearchPath>,
    /// Base game directory name (e.g., "baseq2")
    game_dir: String,
}

impl FileSystem {
    /// Create a new empty filesystem.
    pub fn new(game_dir: &str) -> Self {
        FileSystem {
            search_paths: Vec::new(),
            game_dir: game_dir.to_string(),
        }
    }

    /// Add a pre-loaded PAK directly.
    ///
    /// Use this on WASM to register a `Pack` backed by a `JsPakReader`.
    pub fn add_pack(&mut self, pack: Pack) {
        self.search_paths.push(SearchPath::Pack(pack));
    }

    /// Add a directory to the search path. Also loads any .pak files found in it.
    ///
    /// Priority matches C `FS_AddDirToSearchPath`, lowest first: loose files
    /// in the directory, then `pak0.pak`..`pak99.pak` in numeric order, then
    /// every other `.pak` (sorted by name; C uses readdir order).
    pub fn add_game_directory(&mut self, dir: &Path) -> Q2Result<()> {
        if !dir.is_dir() {
            return Err(Q2Error::Drop(format!("not a directory: {}", dir.display())));
        }

        let mut pak_files: Vec<PathBuf> = Vec::new();
        match std::fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        if ext.eq_ignore_ascii_case("pak") {
                            pak_files.push(path);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Unable to read directory {}: {}", dir.display(), e);
            }
        }
        // Numbered paks first (ascending), then the rest by name.
        let pak_number = |path: &PathBuf| -> Option<u32> {
            let file = path.file_name()?.to_str()?.to_ascii_lowercase();
            let n: u32 = file
                .strip_prefix("pak")?
                .strip_suffix(".pak")?
                .parse()
                .ok()?;
            (n < MAX_PAKS && file == format!("pak{n}.pak")).then_some(n)
        };
        pak_files.sort_by_key(|p| (pak_number(p).is_none(), pak_number(p), p.clone()));

        self.search_paths
            .push(SearchPath::Directory(dir.to_path_buf()));

        for pak_path in &pak_files {
            match Pack::load(pak_path) {
                Ok(pack) => self.search_paths.push(SearchPath::Pack(pack)),
                Err(e) => tracing::warn!("Skipping {}: {}", pak_path.display(), e),
            }
        }

        tracing::info!(
            "Added game directory: {} ({} paks)",
            dir.display(),
            pak_files.len()
        );

        Ok(())
    }

    /// Load an entire file into memory, searching all paths.
    pub fn load_file(&self, path: &str) -> Q2Result<Vec<u8>> {
        let cleaned = clean_path(path)
            .ok_or_else(|| Q2Error::Drop(format!("refusing path outside game dir: {path}")))?;
        let normalized = cleaned.to_lowercase();

        for sp in self.search_paths.iter().rev() {
            match sp {
                SearchPath::Directory(dir) => {
                    if let Some(full_path) = find_in_dir(dir, &cleaned, &normalized) {
                        let data = std::fs::read(&full_path).map_err(|e| {
                            Q2Error::Drop(format!("can't read {}: {}", full_path.display(), e))
                        })?;
                        return Ok(data);
                    }
                }
                SearchPath::Pack(pack) => {
                    if let Some(entry) = pack.files.get(&normalized) {
                        return pack.read_file(entry);
                    }
                }
            }
        }

        Err(Q2Error::Drop(format!("file not found: {}", path)))
    }

    /// Check if a file exists in any search path.
    pub fn file_exists(&self, path: &str) -> bool {
        let Some(cleaned) = clean_path(path) else {
            return false;
        };
        let normalized = cleaned.to_lowercase();

        for sp in self.search_paths.iter().rev() {
            match sp {
                SearchPath::Directory(dir) => {
                    if find_in_dir(dir, &cleaned, &normalized).is_some() {
                        return true;
                    }
                }
                SearchPath::Pack(pack) => {
                    if pack.files.contains_key(&normalized) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// List all files matching a given extension (e.g., "bsp", "pcx").
    pub fn list_files(&self, extension: &str) -> Vec<String> {
        let ext = format!(".{}", extension.to_lowercase());
        let mut result = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for sp in self.search_paths.iter().rev() {
            match sp {
                SearchPath::Directory(dir) => {
                    if let Ok(walker) = walk_dir_recursive(dir) {
                        for path in walker {
                            let rel = path
                                .strip_prefix(dir)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .to_string();
                            let normalized = normalize_path(&rel);
                            if normalized.ends_with(&ext) && seen.insert(normalized.clone()) {
                                result.push(normalized);
                            }
                        }
                    }
                }
                SearchPath::Pack(pack) => {
                    for name in pack.files.keys() {
                        if name.ends_with(&ext) && seen.insert(name.clone()) {
                            result.push(name.clone());
                        }
                    }
                }
            }
        }

        result.sort();
        result
    }

    /// Get the current game directory name.
    pub fn game_dir(&self) -> &str {
        &self.game_dir
    }

    /// Number of search paths.
    pub fn search_path_count(&self) -> usize {
        self.search_paths.len()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse PAK directory entries from raw directory bytes (not the full file).
fn parse_pack_directory(pak_name: &str, dir_bytes: &[u8]) -> Q2Result<HashMap<String, PackFile>> {
    if !(dir_bytes.len() as u32).is_multiple_of(PAK_DIR_ENTRY_SIZE) {
        return Err(Q2Error::Drop(format!(
            "{pak_name}: invalid PAK directory length {}",
            dir_bytes.len()
        )));
    }

    let num_files = dir_bytes.len() / PAK_DIR_ENTRY_SIZE as usize;
    if num_files > MAX_FILES_IN_PACK {
        return Err(Q2Error::Drop(format!(
            "{pak_name}: too many files ({num_files} > {MAX_FILES_IN_PACK})"
        )));
    }

    let mut cursor = Cursor::new(dir_bytes);
    let mut files = HashMap::with_capacity(num_files);

    for _ in 0..num_files {
        let mut name_buf = [0u8; PAK_ENTRY_NAME_LEN];
        cursor
            .read_exact(&mut name_buf)
            .map_err(|e| Q2Error::Drop(format!("{pak_name}: truncated directory: {e}")))?;

        let name_end = name_buf
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(PAK_ENTRY_NAME_LEN);
        let file_name = String::from_utf8_lossy(&name_buf[..name_end]).to_string();
        let normalized = normalize_path(&file_name);

        let offset = read_u32_le(&mut cursor)?;
        let size = read_u32_le(&mut cursor)?;

        // C searches the directory linearly, so the first duplicate wins.
        files.entry(normalized.clone()).or_insert(PackFile {
            name: normalized,
            offset,
            size,
        });
    }

    Ok(files)
}

/// Normalize a path: lowercase, forward slashes, strip leading ./
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .to_lowercase()
        .trim_start_matches("./")
        .to_string()
}

/// Locate a loose file: requested case first, then lowercase (C
/// `FS_FOpenFile` tries both on case-sensitive filesystems).
fn find_in_dir(dir: &Path, cleaned: &str, lowercase: &str) -> Option<PathBuf> {
    [cleaned, lowercase]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|full_path| full_path.is_file())
}

/// Clean a requested game path the way C `FS_FOpenFile` does (drop leading
/// `/`, `./` self references and empty segments), then refuse names that
/// could leave the search directory: any `..` segment or a drive prefix.
/// Case is preserved; callers decide when to lowercase.
fn clean_path(path: &str) -> Option<String> {
    let slashed = path.replace('\\', "/");
    let mut segments = Vec::new();
    for segment in slashed.split('/') {
        match segment {
            "" | "." => {}
            ".." => return None,
            s if s.contains(':') => return None,
            s => segments.push(s),
        }
    }
    Some(segments.join("/"))
}

/// Read a little-endian u32 from any reader.
fn read_u32_le<R: Read>(reader: &mut R) -> Q2Result<u32> {
    let mut buf = [0u8; 4];
    reader
        .read_exact(&mut buf)
        .map_err(|e| Q2Error::Drop(format!("read error: {}", e)))?;
    Ok(u32::from_le_bytes(buf))
}

/// Recursively walk a directory, returning file paths.
fn walk_dir_recursive(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];

    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            // file_type() does not follow symlinks, so a symlinked directory
            // is never descended into and a link loop cannot recurse forever.
            if entry.file_type()?.is_dir() {
                stack.push(path);
            } else if path.is_file() {
                files.push(path);
            }
        }
    }

    Ok(files)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal valid PAK file in memory.
    fn create_test_pak(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut pak = Vec::new();
        pak.extend_from_slice(&[0u8; 12]);

        let mut entries = Vec::new();
        for (name, data) in files {
            let offset = pak.len() as u32;
            let size = data.len() as u32;
            pak.extend_from_slice(data);
            entries.push((name.to_string(), offset, size));
        }

        let dir_offset = pak.len() as u32;
        for (name, offset, size) in &entries {
            let mut name_buf = [0u8; PAK_ENTRY_NAME_LEN];
            let name_bytes = name.as_bytes();
            let copy_len = name_bytes.len().min(55);
            name_buf[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            pak.extend_from_slice(&name_buf);
            pak.extend_from_slice(&offset.to_le_bytes());
            pak.extend_from_slice(&size.to_le_bytes());
        }

        let dir_len = (entries.len() as u32) * PAK_DIR_ENTRY_SIZE;
        pak[0..4].copy_from_slice(&PAK_MAGIC.to_le_bytes());
        pak[4..8].copy_from_slice(&dir_offset.to_le_bytes());
        pak[8..12].copy_from_slice(&dir_len.to_le_bytes());
        pak
    }

    #[test]
    fn normalize_path_converts_backslashes() {
        assert_eq!(
            normalize_path("models\\player\\male.md2"),
            "models/player/male.md2"
        );
    }

    #[test]
    fn normalize_path_lowercases() {
        assert_eq!(
            normalize_path("Models/Player/MALE.MD2"),
            "models/player/male.md2"
        );
    }

    #[test]
    fn normalize_path_strips_dot_slash() {
        assert_eq!(normalize_path("./maps/base1.bsp"), "maps/base1.bsp");
    }

    #[test]
    fn pak_open_from_bytes_indexes_directory() {
        let pak_data = create_test_pak(&[
            ("maps/test.bsp", b"BSP_DATA_HERE"),
            ("textures/wall.wal", b"WAL_DATA"),
        ]);
        let pack = Pack::load_from_bytes("test.pak", &pak_data).unwrap();
        assert_eq!(pack.files.len(), 2);
        assert!(pack.files.contains_key("maps/test.bsp"));
        assert!(pack.files.contains_key("textures/wall.wal"));
    }

    #[test]
    fn pak_load_valid() {
        let dir = tempfile::tempdir().unwrap();
        let pak_path = dir.path().join("pak0.pak");

        let pak_data = create_test_pak(&[
            ("maps/test.bsp", b"BSP_DATA_HERE"),
            ("textures/wall.wal", b"WAL_DATA"),
        ]);
        std::fs::write(&pak_path, &pak_data).unwrap();

        let pack = Pack::load(&pak_path).unwrap();
        assert_eq!(pack.files.len(), 2);
        assert!(pack.files.contains_key("maps/test.bsp"));
        assert!(pack.files.contains_key("textures/wall.wal"));
    }

    #[test]
    fn pak_read_file() {
        let dir = tempfile::tempdir().unwrap();
        let pak_path = dir.path().join("pak0.pak");

        let content = b"THIS_IS_BSP_DATA_1234567890";
        let pak_data = create_test_pak(&[("maps/test.bsp", content)]);
        std::fs::write(&pak_path, &pak_data).unwrap();

        let pack = Pack::load(&pak_path).unwrap();
        let entry = pack.files.get("maps/test.bsp").unwrap();
        let data = pack.read_file(entry).unwrap();
        assert_eq!(data, content);
    }

    #[test]
    fn pak_read_file_from_bytes() {
        let content = b"ASSET_CONTENT_XYZ";
        let pak_data = create_test_pak(&[("sounds/boom.wav", content)]);
        let pack = Pack::load_from_bytes("demo.pak", &pak_data).unwrap();
        let entry = pack.files.get("sounds/boom.wav").unwrap();
        assert_eq!(pack.read_file(entry).unwrap(), content);
    }

    #[test]
    fn pak_invalid_magic_rejected() {
        assert!(Pack::load_from_bytes("bad.pak", b"NOT_A_PAK_FILE_AT_ALL").is_err());
    }

    #[test]
    fn pak_open_only_reads_header_and_directory() {
        // Verify that open() doesn't require full data beyond the directory.
        // Specifically: extra trailing bytes (simulating large asset payloads) are ignored.
        let pak_data = create_test_pak(&[("maps/a.bsp", b"large_asset_bytes_here")]);
        // Truncate trailing bytes would cause read_file to fail, but open() must succeed.
        let pack = Pack::load_from_bytes("lazy.pak", &pak_data).unwrap();
        assert!(pack.files.contains_key("maps/a.bsp"));
    }

    #[test]
    fn filesystem_load_from_directory() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();
        std::fs::write(game_dir.join("maps/test.bsp"), b"BSP_DATA").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        let data = fs.load_file("maps/test.bsp").unwrap();
        assert_eq!(data, b"BSP_DATA");
    }

    #[test]
    fn filesystem_load_from_pak() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(&game_dir).unwrap();

        let content = b"BSP_FROM_PAK";
        let pak_data = create_test_pak(&[("maps/demo.bsp", content)]);
        std::fs::write(game_dir.join("pak0.pak"), &pak_data).unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        let data = fs.load_file("maps/demo.bsp").unwrap();
        assert_eq!(data, content);
    }

    #[test]
    fn filesystem_pak_overrides_loose_file_in_same_game_dir() {
        // C FS_AddDirToSearchPath links the directory first and the paks
        // after it, so PAK content shadows loose files of the same name.
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();

        let pak_data = create_test_pak(&[("maps/test.bsp", b"FROM_PAK")]);
        std::fs::write(game_dir.join("pak0.pak"), &pak_data).unwrap();
        std::fs::write(game_dir.join("maps/test.bsp"), b"FROM_DIR").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        let data = fs.load_file("maps/test.bsp").unwrap();
        assert_eq!(data, b"FROM_PAK");
    }

    #[test]
    fn filesystem_higher_numbered_pak_wins_numerically_not_lexically() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(&game_dir).unwrap();
        std::fs::write(
            game_dir.join("pak2.pak"),
            create_test_pak(&[("a.txt", b"PAK2")]),
        )
        .unwrap();
        std::fs::write(
            game_dir.join("pak10.pak"),
            create_test_pak(&[("a.txt", b"PAK10")]),
        )
        .unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert_eq!(fs.load_file("a.txt").unwrap(), b"PAK10");
    }

    #[test]
    fn filesystem_unnumbered_pak_overrides_numbered_pak() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(&game_dir).unwrap();
        std::fs::write(
            game_dir.join("pak0.pak"),
            create_test_pak(&[("a.txt", b"PAK0")]),
        )
        .unwrap();
        std::fs::write(
            game_dir.join("aamod.pak"),
            create_test_pak(&[("a.txt", b"MOD")]),
        )
        .unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert_eq!(fs.load_file("a.txt").unwrap(), b"MOD");
    }

    #[test]
    fn pak_duplicate_entry_first_one_wins() {
        let pak_data = create_test_pak(&[("a.txt", b"FIRST"), ("A.TXT", b"SECOND")]);
        let pack = Pack::load_from_bytes("dup.pak", &pak_data).unwrap();
        let entry = pack.files.get("a.txt").unwrap();
        assert_eq!(pack.read_file(entry).unwrap(), b"FIRST");
    }

    /// Lays out `<tmp>/baseq2/maps/test.bsp` plus `<tmp>/secret.txt` outside
    /// the game directory.
    fn game_dir_with_sibling_secret() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();
        std::fs::write(game_dir.join("maps/test.bsp"), b"BSP").unwrap();
        let secret = dir.path().join("secret.txt");
        std::fs::write(&secret, b"SECRET").unwrap();
        (dir, game_dir, secret)
    }

    #[test]
    fn load_file_rejects_parent_dir_components() {
        let (_dir, game_dir, _secret) = game_dir_with_sibling_secret();
        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        for name in ["../secret.txt", "maps/../../secret.txt", "..\\secret.txt"] {
            assert!(fs.load_file(name).is_err(), "{name} escaped the game dir");
            assert!(!fs.file_exists(name), "{name} escaped the game dir");
        }
    }

    #[test]
    fn load_file_absolute_path_stays_inside_game_dir() {
        let (_dir, game_dir, secret) = game_dir_with_sibling_secret();
        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        let abs = secret.to_string_lossy().to_string();
        assert!(fs.load_file(&abs).is_err(), "absolute path read host file");
        assert!(!fs.file_exists(&abs));
        // C strips the leading slash, so this resolves inside the game dir.
        assert_eq!(fs.load_file("/maps/test.bsp").unwrap(), b"BSP");
        assert_eq!(fs.load_file(".//maps/./test.bsp").unwrap(), b"BSP");
    }

    #[test]
    fn clean_path_rejects_drive_prefix_and_parent_dir() {
        assert_eq!(clean_path("c:\\autoexec.bat"), None);
        assert_eq!(clean_path("a/../b"), None);
        assert_eq!(
            clean_path("/maps//base1.bsp").as_deref(),
            Some("maps/base1.bsp")
        );
        assert_eq!(clean_path("maps/...x").as_deref(), Some("maps/...x"));
    }

    #[test]
    fn load_file_finds_mixed_case_loose_file() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("Maps")).unwrap();
        std::fs::write(game_dir.join("Maps/Foo.bsp"), b"FOO").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert_eq!(fs.load_file("Maps/Foo.bsp").unwrap(), b"FOO");
    }

    #[cfg(unix)]
    #[test]
    fn list_files_does_not_follow_symlink_loops() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();
        std::fs::write(game_dir.join("maps/a.bsp"), b"").unwrap();
        std::os::unix::fs::symlink(&game_dir, game_dir.join("maps/loop")).unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert_eq!(fs.list_files("bsp"), vec!["maps/a.bsp".to_string()]);
    }

    #[test]
    fn pak_reader_accepts_single_threaded_backend() {
        // The engine has no worker threads; a backend holding a !Send handle
        // (like a JS-heap Uint8Array on wasm) must plug in without unsafe.
        struct RcReader(std::rc::Rc<Vec<u8>>);
        impl PakReader for RcReader {
            fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
                let s = offset as usize;
                let e = s
                    .checked_add(len as usize)
                    .filter(|&e| e <= self.0.len())
                    .ok_or_else(|| Q2Error::Drop("out of bounds".into()))?;
                Ok(self.0[s..e].to_vec())
            }
        }

        let pak_data = create_test_pak(&[("a.txt", b"RC")]);
        let pack = Pack::open("rc.pak", Box::new(RcReader(std::rc::Rc::new(pak_data)))).unwrap();
        let entry = pack.files.get("a.txt").unwrap();
        assert_eq!(pack.read_file(entry).unwrap(), b"RC");
    }

    /// Records the largest `read_slice` length requested.
    struct CountingReader {
        data: Vec<u8>,
        max_len: std::sync::Arc<std::sync::atomic::AtomicU32>,
    }

    impl PakReader for CountingReader {
        fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
            self.max_len
                .fetch_max(len, std::sync::atomic::Ordering::Relaxed);
            let s = offset as usize;
            let e = s
                .checked_add(len as usize)
                .filter(|&e| e <= self.data.len())
                .ok_or_else(|| Q2Error::Drop("out of bounds".into()))?;
            Ok(self.data[s..e].to_vec())
        }
    }

    fn pak_header(dir_offset: u32, dir_len: u32) -> Vec<u8> {
        let mut pak = Vec::new();
        pak.extend_from_slice(&PAK_MAGIC.to_le_bytes());
        pak.extend_from_slice(&dir_offset.to_le_bytes());
        pak.extend_from_slice(&dir_len.to_le_bytes());
        pak
    }

    #[test]
    fn pack_open_rejects_oversized_dir_len_without_reading_it() {
        let max_len = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let reader = CountingReader {
            data: pak_header(12, 0xFFFF_FFC0),
            max_len: max_len.clone(),
        };
        assert!(Pack::open("huge.pak", Box::new(reader)).is_err());
        let read = max_len.load(std::sync::atomic::Ordering::Relaxed);
        assert!(read <= 12, "read {read} bytes");
    }

    #[test]
    fn pack_open_rejects_too_many_files_before_reading_directory() {
        let max_len = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let dir_len = (MAX_FILES_IN_PACK as u32 + 1) * PAK_DIR_ENTRY_SIZE;
        let reader = CountingReader {
            data: pak_header(12, dir_len),
            max_len: max_len.clone(),
        };
        assert!(Pack::open("many.pak", Box::new(reader)).is_err());
        let read = max_len.load(std::sync::atomic::Ordering::Relaxed);
        assert!(read <= 12, "read {read} bytes");
    }

    #[test]
    fn pack_open_rejects_empty_directory() {
        // C FS_LoadPAK: numFiles == 0 -> "is too short".
        assert!(Pack::load_from_bytes("empty.pak", &pak_header(12, 0)).is_err());
    }

    #[test]
    fn pack_open_rejects_dir_len_not_multiple_of_entry_size() {
        let mut pak = create_test_pak(&[("a.txt", b"A")]);
        pak[8..12].copy_from_slice(&65u32.to_le_bytes());
        pak.push(0);
        assert!(Pack::load_from_bytes("odd.pak", &pak).is_err());
    }

    #[test]
    fn pack_open_rejects_truncated_header() {
        assert!(Pack::load_from_bytes("short.pak", &PAK_MAGIC.to_le_bytes()).is_err());
    }

    #[test]
    fn pack_read_file_past_eof_is_error() {
        let mut pak = create_test_pak(&[("a.txt", b"AAAA")]);
        let dir_offset = u32::from_le_bytes([pak[4], pak[5], pak[6], pak[7]]) as usize;
        // Entry size field sits after the 56-byte name and 4-byte offset.
        let size_at = dir_offset + PAK_ENTRY_NAME_LEN + 4;
        pak[size_at..size_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let pack = Pack::load_from_bytes("eof.pak", &pak).unwrap();
        let entry = pack.files.get("a.txt").unwrap();
        assert!(pack.read_file(entry).is_err());
    }

    #[test]
    fn disk_pak_read_past_eof_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.pak");
        std::fs::write(&path, [0u8; 16]).unwrap();
        let reader = DiskPakReader { path };
        assert!(reader.read_slice(8, u32::MAX).is_err());
        assert!(reader.read_slice(u32::MAX, 1).is_err());
        assert_eq!(reader.read_slice(8, 8).unwrap(), vec![0u8; 8]);
    }

    #[test]
    fn pak_entry_name_without_nul_uses_all_56_bytes() {
        let long_name = "x".repeat(PAK_ENTRY_NAME_LEN);
        let mut pak = create_test_pak(&[("placeholder", b"DATA")]);
        let dir_offset = u32::from_le_bytes([pak[4], pak[5], pak[6], pak[7]]) as usize;
        pak[dir_offset..dir_offset + PAK_ENTRY_NAME_LEN].copy_from_slice(long_name.as_bytes());
        let pack = Pack::load_from_bytes("long.pak", &pak).unwrap();
        assert!(pack.files.contains_key(&long_name));
    }

    #[test]
    fn pak_entry_non_utf8_name_is_indexed_lossily() {
        let pak = create_test_pak(&[("ok.txt", b"A")]);
        let mut pak = pak;
        let dir_offset = u32::from_le_bytes([pak[4], pak[5], pak[6], pak[7]]) as usize;
        pak[dir_offset] = 0xFF;
        let pack = Pack::load_from_bytes("bad_utf8.pak", &pak).unwrap();
        assert_eq!(pack.file_count(), 1);
    }

    #[test]
    fn add_game_directory_rejects_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not_a_dir");
        std::fs::write(&file, b"").unwrap();
        let mut fs = FileSystem::new("baseq2");
        assert!(fs.add_game_directory(&file).is_err());
    }

    #[test]
    fn add_game_directory_skips_corrupt_pak_and_keeps_others() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(&game_dir).unwrap();
        std::fs::write(
            game_dir.join("pak0.pak"),
            create_test_pak(&[("a.txt", b"A")]),
        )
        .unwrap();
        std::fs::write(game_dir.join("pak1.pak"), b"garbage").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();
        assert_eq!(fs.load_file("a.txt").unwrap(), b"A");
    }

    #[test]
    fn filesystem_file_not_found() {
        let fs = FileSystem::new("baseq2");
        assert!(fs.load_file("nonexistent.bsp").is_err());
    }

    #[test]
    fn filesystem_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();
        std::fs::write(game_dir.join("maps/test.bsp"), b"data").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert!(fs.file_exists("maps/test.bsp"));
        assert!(!fs.file_exists("maps/nope.bsp"));
    }

    #[test]
    fn filesystem_case_insensitive_pak_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(&game_dir).unwrap();

        let pak_data = create_test_pak(&[("Maps/Test.BSP", b"data")]);
        std::fs::write(game_dir.join("pak0.pak"), &pak_data).unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        assert!(fs.file_exists("maps/test.bsp"));
    }

    #[test]
    fn filesystem_list_files() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();
        std::fs::write(game_dir.join("maps/a.bsp"), b"").unwrap();
        std::fs::write(game_dir.join("maps/b.bsp"), b"").unwrap();
        std::fs::write(game_dir.join("maps/c.txt"), b"").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        let bsp_files = fs.list_files("bsp");
        assert_eq!(bsp_files.len(), 2);
        assert!(bsp_files.iter().any(|f| f.contains("a.bsp")));
        assert!(bsp_files.iter().any(|f| f.contains("b.bsp")));
    }

    #[test]
    fn custom_pak_reader_composes_with_filesystem() {
        // Verify the trait boundary: a custom PakReader plugs in without changes
        // to FileSystem or Pack.
        let content = b"CUSTOM_BACKEND_DATA";
        let pak_data = create_test_pak(&[("data/config.cfg", content)]);

        struct VecReader(Vec<u8>);
        impl PakReader for VecReader {
            fn read_slice(&self, offset: u32, len: u32) -> Q2Result<Vec<u8>> {
                let s = offset as usize;
                let e = s + len as usize;
                if e > self.0.len() {
                    return Err(Q2Error::Drop("out of bounds".into()));
                }
                Ok(self.0[s..e].to_vec())
            }
        }

        let pack = Pack::open("custom.pak", Box::new(VecReader(pak_data))).unwrap();
        let entry = pack.files.get("data/config.cfg").unwrap();
        assert_eq!(pack.read_file(entry).unwrap(), content);
    }
}
