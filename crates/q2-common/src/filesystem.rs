//! Virtual filesystem with PAK archive support.
//!
//! Replaces C `filesystem.c`. Provides:
//! - PAK file loading (Quake 2 `.pak` format)
//! - Directory-based search paths
//! - Unified `load_file` / `open_file` interface
//! - Case-insensitive filename matching
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0

use std::collections::HashMap;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::error::{Q2Error, Q2Result};

// PAK file magic: "PACK" as little-endian u32
const PAK_MAGIC: u32 = (b'P' as u32)
    | ((b'A' as u32) << 8)
    | ((b'C' as u32) << 16)
    | ((b'K' as u32) << 24);

/// PAK filename field length — fixed at 56 bytes (null-padded) per the Quake
/// PAK format spec. Total directory entry = 56 name + 4 offset + 4 size = 64.
const PAK_ENTRY_NAME_LEN: usize = 56;
const PAK_DIR_ENTRY_SIZE: u32 = 64;

/// Maximum files per PAK
const MAX_FILES_IN_PACK: usize = 4096;

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

/// A loaded PAK archive.
#[derive(Debug)]
pub struct Pack {
    /// Path to the .pak file on disk (empty for in-memory packs)
    pub path: PathBuf,
    /// Files in this pack, keyed by normalized name
    pub files: HashMap<String, PackFile>,
    /// In-memory PAK data (for WASM where we fetch the whole file)
    data: Option<Vec<u8>>,
}

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

/// Parse PAK directory entries from raw bytes. Shared by `Pack::load` and
/// `Pack::load_from_bytes` to avoid duplicating the parsing logic.
fn parse_pack_data(name: &str, data: &[u8]) -> Q2Result<HashMap<String, PackFile>> {
    if data.len() < 12 {
        return Err(Q2Error::Drop(format!(
            "{}: data too small for PAK header",
            name
        )));
    }

    let mut cursor = Cursor::new(data);

    let magic = read_u32_le(&mut cursor)?;
    if magic != PAK_MAGIC {
        return Err(Q2Error::Drop(format!(
            "{}: not a PAK file (magic: {:#010x})",
            name, magic
        )));
    }

    let dir_offset = read_u32_le(&mut cursor)?;
    let dir_len = read_u32_le(&mut cursor)?;

    if dir_len % PAK_DIR_ENTRY_SIZE != 0 {
        return Err(Q2Error::Drop(format!(
            "{}: invalid PAK directory length {}",
            name, dir_len
        )));
    }

    let num_files = (dir_len / PAK_DIR_ENTRY_SIZE) as usize;
    if num_files > MAX_FILES_IN_PACK {
        return Err(Q2Error::Drop(format!(
            "{}: too many files ({} > {})",
            name, num_files, MAX_FILES_IN_PACK
        )));
    }

    cursor.seek(SeekFrom::Start(dir_offset as u64)).map_err(|e| {
        Q2Error::Drop(format!("{}: can't seek to directory: {}", name, e))
    })?;

    let mut files = HashMap::with_capacity(num_files);

    for _ in 0..num_files {
        let mut name_buf = [0u8; PAK_ENTRY_NAME_LEN];
        cursor.read_exact(&mut name_buf).map_err(|e| {
            Q2Error::Drop(format!("{}: truncated directory: {}", name, e))
        })?;

        let name_end = name_buf.iter().position(|&b| b == 0).unwrap_or(PAK_ENTRY_NAME_LEN);
        let file_name = String::from_utf8_lossy(&name_buf[..name_end]).to_string();
        let normalized = normalize_path(&file_name);

        let offset = read_u32_le(&mut cursor)?;
        let size = read_u32_le(&mut cursor)?;

        files.insert(
            normalized.clone(),
            PackFile {
                name: normalized,
                offset,
                size,
            },
        );
    }

    Ok(files)
}

impl Pack {
    /// Load a PAK file from disk, parsing its directory.
    pub fn load(path: &Path) -> Q2Result<Self> {
        let data = std::fs::read(path)
            .map_err(|e| Q2Error::Drop(format!("can't open {}: {}", path.display(), e)))?;

        let display_name = path.display().to_string();
        let files = parse_pack_data(&display_name, &data)?;

        tracing::info!(
            "Loaded PAK: {} ({} files)",
            path.display(),
            files.len()
        );

        Ok(Pack {
            path: path.to_path_buf(),
            files,
            data: None,
        })
    }

    /// Load a PAK from an in-memory byte buffer (for WASM where files are fetched).
    pub fn load_from_bytes(name: &str, data: &[u8]) -> Q2Result<Self> {
        let files = parse_pack_data(name, data)?;

        tracing::info!("Loaded PAK from memory: {} ({} files)", name, files.len());

        Ok(Pack {
            path: PathBuf::from(name),
            files,
            data: Some(data.to_vec()),
        })
    }

    /// Read a file from this PAK archive.
    pub fn read_file(&self, entry: &PackFile) -> Q2Result<Vec<u8>> {
        let start = entry.offset as usize;
        let end = start + entry.size as usize;

        // In-memory mode (WASM)
        if let Some(ref data) = self.data {
            if end > data.len() {
                return Err(Q2Error::Drop(format!(
                    "{}: file '{}' extends past end of PAK",
                    self.path.display(),
                    entry.name
                )));
            }
            return Ok(data[start..end].to_vec());
        }

        // Disk mode (native)
        let file = std::fs::File::open(&self.path)
            .map_err(|e| Q2Error::Drop(format!("can't open {}: {}", self.path.display(), e)))?;
        let mut reader = io::BufReader::new(file);
        reader
            .seek(SeekFrom::Start(entry.offset as u64))
            .map_err(|e| Q2Error::Drop(format!("seek error: {}", e)))?;
        let mut buf = vec![0u8; entry.size as usize];
        reader
            .read_exact(&mut buf)
            .map_err(|e| Q2Error::Drop(format!("read error: {}", e)))?;
        Ok(buf)
    }
}

impl FileSystem {
    /// Create a new empty filesystem.
    pub fn new(game_dir: &str) -> Self {
        FileSystem {
            search_paths: Vec::new(),
            game_dir: game_dir.to_string(),
        }
    }

    /// Add a pre-loaded PAK directly (for WASM where data is fetched into memory).
    pub fn add_pack(&mut self, pack: Pack) {
        self.search_paths.push(SearchPath::Pack(pack));
    }

    /// Add a directory to the search path. Also loads any .pak files found in it.
    /// PAK files are loaded in alphabetical order (pak0.pak, pak1.pak, ...).
    pub fn add_game_directory(&mut self, dir: &Path) -> Q2Result<()> {
        if !dir.is_dir() {
            return Err(Q2Error::Drop(format!(
                "not a directory: {}",
                dir.display()
            )));
        }

        // Find and load PAK files in sorted order
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
        pak_files.sort();

        // Add PAK files first (lower priority than loose files)
        for pak_path in &pak_files {
            match Pack::load(pak_path) {
                Ok(pack) => {
                    self.search_paths.push(SearchPath::Pack(pack));
                }
                Err(e) => {
                    tracing::warn!("Skipping {}: {}", pak_path.display(), e);
                }
            }
        }

        // Add directory itself (higher priority — searched before PAKs)
        self.search_paths
            .push(SearchPath::Directory(dir.to_path_buf()));

        tracing::info!(
            "Added game directory: {} ({} paks)",
            dir.display(),
            pak_files.len()
        );

        Ok(())
    }

    /// Load an entire file into memory, searching all paths.
    /// Returns the file contents or an error if not found.
    pub fn load_file(&self, path: &str) -> Q2Result<Vec<u8>> {
        let normalized = normalize_path(path);

        // Search in reverse order (last added = highest priority)
        for sp in self.search_paths.iter().rev() {
            match sp {
                SearchPath::Directory(dir) => {
                    let full_path = dir.join(&normalized);
                    if full_path.is_file() {
                        let data = std::fs::read(&full_path).map_err(|e| {
                            Q2Error::Drop(format!(
                                "can't read {}: {}",
                                full_path.display(),
                                e
                            ))
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
        let normalized = normalize_path(path);

        for sp in self.search_paths.iter().rev() {
            match sp {
                SearchPath::Directory(dir) => {
                    if dir.join(&normalized).is_file() {
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

/// Normalize a path: lowercase, forward slashes, strip leading ./
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
        .to_lowercase()
        .trim_start_matches("./")
        .to_string()
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
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push(path);
            }
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a minimal valid PAK file in memory.
    fn create_test_pak(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut pak = Vec::new();

        // Reserve space for header (12 bytes)
        pak.extend_from_slice(&[0u8; 12]);

        // Write file data and record entries
        let mut entries = Vec::new();
        for (name, data) in files {
            let offset = pak.len() as u32;
            let size = data.len() as u32;
            pak.extend_from_slice(data);
            entries.push((name.to_string(), offset, size));
        }

        // Write directory
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

        // Patch header
        pak[0..4].copy_from_slice(&PAK_MAGIC.to_le_bytes());
        pak[4..8].copy_from_slice(&dir_offset.to_le_bytes());
        pak[8..12].copy_from_slice(&dir_len.to_le_bytes());

        pak
    }

    #[test]
    fn normalize_path_converts_backslashes() {
        assert_eq!(normalize_path("models\\player\\male.md2"), "models/player/male.md2");
    }

    #[test]
    fn normalize_path_lowercases() {
        assert_eq!(normalize_path("Models/Player/MALE.MD2"), "models/player/male.md2");
    }

    #[test]
    fn normalize_path_strips_dot_slash() {
        assert_eq!(normalize_path("./maps/base1.bsp"), "maps/base1.bsp");
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
    fn pak_invalid_magic_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let pak_path = dir.path().join("bad.pak");
        std::fs::write(&pak_path, b"NOT_A_PAK_FILE_AT_ALL").unwrap();

        assert!(Pack::load(&pak_path).is_err());
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
    fn filesystem_directory_overrides_pak() {
        let dir = tempfile::tempdir().unwrap();
        let game_dir = dir.path().join("baseq2");
        std::fs::create_dir_all(game_dir.join("maps")).unwrap();

        // Put one version in PAK
        let pak_data = create_test_pak(&[("maps/test.bsp", b"FROM_PAK")]);
        std::fs::write(game_dir.join("pak0.pak"), &pak_data).unwrap();

        // Put another version as loose file
        std::fs::write(game_dir.join("maps/test.bsp"), b"FROM_DIR").unwrap();

        let mut fs = FileSystem::new("baseq2");
        fs.add_game_directory(&game_dir).unwrap();

        // Directory should win (higher priority)
        let data = fs.load_file("maps/test.bsp").unwrap();
        assert_eq!(data, b"FROM_DIR");
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

        // Lookup with different case should still work
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
}
