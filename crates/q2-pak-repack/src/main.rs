use anyhow::{bail, Context, Result};
use clap::Parser;
use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(
    name = "q2-pak-repack",
    about = "Repack a Q2 PAK and optionally Brotli-compress it for web delivery"
)]
struct Args {
    #[arg(long, value_name = "PATH")]
    r#in: PathBuf,

    #[arg(long, value_name = "PATH")]
    out: PathBuf,

    /// Comma-separated extensions to keep (e.g. "bsp,cfg"; any case, a
    /// leading "." or "*." is ignored). Matching no file is an error.
    /// Without this flag every file is kept.
    #[arg(long)]
    allow: Option<String>,

    /// Also write <out>.br (Brotli level 11) for Content-Encoding: br serving.
    /// Without this flag an existing <out>.br is removed so it cannot go stale.
    #[arg(long)]
    brotli: bool,
}

pub const PAK_MAGIC: &[u8; 4] = b"PACK";
pub const ENTRY_SIZE: usize = 64;
pub const NAME_LEN: usize = 56;
const HEADER_LEN: usize = 12;

fn main() -> Result<()> {
    let args = Args::parse();

    let allowlist = args.allow.as_deref().map(parse_allowlist).transpose()?;

    let src = fs::read(&args.r#in).with_context(|| format!("reading {}", args.r#in.display()))?;

    let out = repack(&src, allowlist.as_ref())?;
    if allowlist.is_some() && entry_count(&out) == 0 && entry_count(&src) > 0 {
        bail!(
            "--allow {:?} matched no file in {}",
            args.allow.unwrap_or_default(),
            args.r#in.display()
        );
    }

    write_outputs(&args.out, &out, args.brotli)?;

    let src_mb = src.len() as f64 / 1_000_000.0;
    let out_mb = out.len() as f64 / 1_000_000.0;
    let retained = entry_count(&out);
    let total = entry_count(&src);

    if allowlist.is_some() {
        let pct = (1.0 - out.len() as f64 / src.len() as f64) * 100.0;
        println!(
            "{}: {:.1} MB → {:.2} MB  ({}/{} files, {:.0}% reduction)",
            args.out.display(),
            src_mb,
            out_mb,
            retained,
            total,
            pct
        );
    } else {
        println!(
            "{}: {:.1} MB  ({} files, all retained)",
            args.out.display(),
            out_mb,
            retained
        );
    }

    if args.brotli {
        let br = br_path(&args.out);
        let br_mb = fs::metadata(&br)
            .with_context(|| format!("reading size of {}", br.display()))?
            .len() as f64
            / 1_000_000.0;
        let br_pct = (1.0 - br_mb / out_mb) * 100.0;
        println!(
            "{}: {:.2} MB (brotli level 11, {:.0}% compression)",
            br.display(),
            br_mb,
            br_pct
        );
    }

    Ok(())
}

/// `<out>.br`, the name the devserver looks for next to the web pak.
/// `--allow` list: extensions with an optional leading `.` or `*.`, any case.
fn parse_allowlist(raw: &str) -> Result<HashSet<String>> {
    let extensions: HashSet<String> = raw
        .split(',')
        .map(|e| {
            e.trim()
                .trim_start_matches('*')
                .trim_start_matches('.')
                .to_lowercase()
        })
        .filter(|e| !e.is_empty())
        .collect();
    if extensions.is_empty() {
        bail!("--allow {raw:?} names no extension");
    }
    Ok(extensions)
}

fn br_path(out: &Path) -> PathBuf {
    let mut name = OsString::from(out.as_os_str());
    name.push(".br");
    PathBuf::from(name)
}

/// Write the repacked PAK and, if requested, its Brotli variant.
///
/// The old `.br` is removed before anything is written, so a run that fails
/// midway, or one without `--brotli`, never leaves a `.br` describing an older
/// PAK. Each file is written to a temporary name and renamed into place, so a
/// reader never sees a truncated file.
fn write_outputs(out: &Path, pak: &[u8], brotli: bool) -> Result<()> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
    }

    let br = br_path(out);
    match fs::remove_file(&br) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("removing stale {}", br.display())),
    }

    write_atomic(out, |file| file.write_all(pak).map_err(Into::into))?;
    if brotli {
        write_atomic(&br, |file| write_brotli(pak, file))?;
    }
    Ok(())
}

/// Write `path` via `<path>.tmp` + fsync + rename.
fn write_atomic(path: &Path, fill: impl FnOnce(&mut fs::File) -> Result<()>) -> Result<()> {
    let mut tmp_name = OsString::from(path.as_os_str());
    tmp_name.push(".tmp");
    let tmp = PathBuf::from(tmp_name);

    let written = (|| {
        let mut file =
            fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        fill(&mut file).with_context(|| format!("writing {}", tmp.display()))?;
        file.sync_all()
            .with_context(|| format!("syncing {}", tmp.display()))?;
        fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))
    })();
    if written.is_err() {
        // Cleanup only; the write error above is the one the caller needs.
        let _ = fs::remove_file(&tmp);
    }
    written
}

/// Brotli-compress `data` (quality 11, window 22) into `w`.
///
/// Uses the one-shot encoder because `CompressorWriter` finishes the stream in
/// `Drop` and discards any error from the final write.
fn write_brotli<W: Write>(data: &[u8], w: &mut W) -> Result<()> {
    let params = brotli::enc::BrotliEncoderParams {
        quality: 11,
        lgwin: 22,
        size_hint: data.len(),
        ..Default::default()
    };
    brotli::BrotliCompress(&mut &data[..], w, &params).context("brotli compression failed")?;
    w.flush().context("flushing brotli output")?;
    Ok(())
}

/// Repack a Q2 PAK, optionally filtering to an extension allowlist.
///
/// `allowlist = None` means keep every file.
/// `allowlist = Some(set)` keeps only files whose lowercase extension is in `set`.
///
/// Output is a valid PAK with all offsets recalculated from scratch (no gaps).
pub fn repack(src: &[u8], allowlist: Option<&HashSet<String>>) -> Result<Vec<u8>> {
    repack_with_limit(src, allowlist, u32::MAX as usize)
}

/// `repack` with an output size cap. PAK offsets are u32, and directory
/// entries may alias the same source bytes, so the output can be far larger
/// than the input; the cap is checked before anything is copied.
fn repack_with_limit(
    src: &[u8],
    allowlist: Option<&HashSet<String>>,
    max_out: usize,
) -> Result<Vec<u8>> {
    if src.len() < HEADER_LEN || &src[0..4] != PAK_MAGIC {
        bail!("not a valid PAK file (bad magic)");
    }

    let dir_offset = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
    let dir_len = u32::from_le_bytes(src[8..12].try_into().unwrap()) as usize;

    if !dir_len.is_multiple_of(ENTRY_SIZE) {
        bail!(
            "PAK directory length {} is not a multiple of {}",
            dir_len,
            ENTRY_SIZE
        );
    }
    if dir_offset.saturating_add(dir_len) > src.len() {
        bail!("PAK directory extends past end of file");
    }

    let num_files = dir_len / ENTRY_SIZE;
    let mut kept: Vec<([u8; NAME_LEN], &[u8])> = Vec::with_capacity(num_files);
    let mut out_len = HEADER_LEN;

    for i in 0..num_files {
        let base = dir_offset + i * ENTRY_SIZE;
        let raw_name: [u8; NAME_LEN] = src[base..base + NAME_LEN].try_into().unwrap();
        let offset = u32::from_le_bytes(
            src[base + NAME_LEN..base + NAME_LEN + 4]
                .try_into()
                .unwrap(),
        );
        let size = u32::from_le_bytes(
            src[base + NAME_LEN + 4..base + ENTRY_SIZE]
                .try_into()
                .unwrap(),
        );

        let name_end = raw_name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        let name = String::from_utf8_lossy(&raw_name[..name_end]);

        if let Some(allow) = allowlist {
            let ext = Path::new(name.as_ref())
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_lowercase);
            if !ext.is_some_and(|ext| allow.contains(&ext)) {
                continue;
            }
        }

        let start = offset as usize;
        let end = start.saturating_add(size as usize);
        if end > src.len() {
            bail!("entry '{}' data extends past end of file", name);
        }

        out_len = out_len.saturating_add(size as usize + ENTRY_SIZE);
        if out_len > max_out {
            bail!("output PAK exceeds {} bytes (PAK offsets are u32)", max_out);
        }

        kept.push((raw_name, &src[start..end]));
    }

    // Build output PAK: header | file data... | directory
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    out.extend_from_slice(PAK_MAGIC);
    out.extend_from_slice(&[0u8; 8]); // placeholder for dir_offset + dir_len

    // Every value below is <= out_len <= max_out <= u32::MAX, checked above.
    let to_u32 = |n: usize| u32::try_from(n).expect("bounded by max_out check");

    let mut placed: Vec<([u8; NAME_LEN], u32, u32)> = Vec::with_capacity(kept.len());
    for (name, data) in &kept {
        let file_offset = to_u32(out.len());
        out.extend_from_slice(data);
        placed.push((*name, file_offset, to_u32(data.len())));
    }

    let dir_offset_out = to_u32(out.len());
    for (name, file_offset, size) in &placed {
        out.extend_from_slice(name);
        out.extend_from_slice(&file_offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }

    let dir_len_out = to_u32(placed.len() * ENTRY_SIZE);
    out[4..8].copy_from_slice(&dir_offset_out.to_le_bytes());
    out[8..12].copy_from_slice(&dir_len_out.to_le_bytes());

    Ok(out)
}

fn entry_count(pak: &[u8]) -> usize {
    if pak.len() < HEADER_LEN {
        return 0;
    }
    let dir_len = u32::from_le_bytes(pak[8..12].try_into().unwrap()) as usize;
    dir_len / ENTRY_SIZE
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid PAK in memory with the given files.
    fn make_pak(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut pak = Vec::new();
        pak.extend_from_slice(&[0u8; 12]); // header placeholder

        let mut dir: Vec<(String, u32, u32)> = Vec::new();
        for (name, data) in files {
            let offset = pak.len() as u32;
            pak.extend_from_slice(data);
            dir.push((name.to_string(), offset, data.len() as u32));
        }

        let dir_offset = pak.len() as u32;
        for (name, offset, size) in &dir {
            let mut name_buf = [0u8; NAME_LEN];
            let bytes = name.as_bytes();
            let len = bytes.len().min(NAME_LEN - 1);
            name_buf[..len].copy_from_slice(&bytes[..len]);
            pak.extend_from_slice(&name_buf);
            pak.extend_from_slice(&offset.to_le_bytes());
            pak.extend_from_slice(&size.to_le_bytes());
        }

        let dir_len = (dir.len() * ENTRY_SIZE) as u32;
        pak[0..4].copy_from_slice(PAK_MAGIC);
        pak[4..8].copy_from_slice(&dir_offset.to_le_bytes());
        pak[8..12].copy_from_slice(&dir_len.to_le_bytes());
        pak
    }

    /// Parse a PAK's directory and return (name, offset, size) tuples.
    fn parse_dir(pak: &[u8]) -> Vec<(String, u32, u32)> {
        assert!(pak.len() >= 12 && &pak[0..4] == PAK_MAGIC);
        let dir_offset = u32::from_le_bytes(pak[4..8].try_into().unwrap()) as usize;
        let dir_len = u32::from_le_bytes(pak[8..12].try_into().unwrap()) as usize;
        let num = dir_len / ENTRY_SIZE;
        let mut entries = Vec::new();
        for i in 0..num {
            let base = dir_offset + i * ENTRY_SIZE;
            let name_bytes = &pak[base..base + NAME_LEN];
            let end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name = std::str::from_utf8(&name_bytes[..end]).unwrap().to_string();
            let offset = u32::from_le_bytes(
                pak[base + NAME_LEN..base + NAME_LEN + 4]
                    .try_into()
                    .unwrap(),
            );
            let size = u32::from_le_bytes(
                pak[base + NAME_LEN + 4..base + NAME_LEN + 8]
                    .try_into()
                    .unwrap(),
            );
            entries.push((name, offset, size));
        }
        entries
    }

    #[test]
    fn repack_no_filter_keeps_all_files() {
        let src = make_pak(&[
            ("maps/base1.bsp", b"BSP_DATA"),
            ("sound/boom.wav", b"WAV_DATA"),
            ("textures/wall.wal", b"WAL_DATA"),
        ]);

        let out = repack(&src, None).unwrap();

        let entries = parse_dir(&out);
        assert_eq!(entries.len(), 3);
        let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"maps/base1.bsp"));
        assert!(names.contains(&"sound/boom.wav"));
        assert!(names.contains(&"textures/wall.wal"));
    }

    #[test]
    fn repack_allowlist_filters_to_matching_extensions() {
        let src = make_pak(&[
            ("maps/base1.bsp", b"BSP_DATA"),
            ("sound/boom.wav", b"WAV_DATA"),
            ("textures/wall.wal", b"WAL_DATA"),
        ]);

        let allow: HashSet<String> = ["bsp"].iter().map(|s| s.to_string()).collect();
        let out = repack(&src, Some(&allow)).unwrap();

        let entries = parse_dir(&out);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "maps/base1.bsp");
    }

    #[test]
    fn repack_file_bytes_are_byte_identical_to_source() {
        let bsp_bytes = b"ACTUAL_BSP_CONTENT_XYZ_1234";
        let wav_bytes = b"ACTUAL_WAV_CONTENT_ABCDEFGH";
        let src = make_pak(&[("maps/base1.bsp", bsp_bytes), ("sound/boom.wav", wav_bytes)]);

        let out = repack(&src, None).unwrap();

        let entries = parse_dir(&out);
        for (name, offset, size) in entries {
            let data = &out[offset as usize..offset as usize + size as usize];
            if name == "maps/base1.bsp" {
                assert_eq!(data, bsp_bytes);
            } else if name == "sound/boom.wav" {
                assert_eq!(data, wav_bytes);
            }
        }
    }

    #[test]
    fn repack_output_offsets_are_recalculated_contiguously() {
        // Source PAK has files at arbitrary offsets; output should pack them tightly
        let src = make_pak(&[("a.bsp", b"AAAA"), ("b.bsp", b"BBBBBBBB")]);

        let out = repack(&src, None).unwrap();

        let entries = parse_dir(&out);
        assert_eq!(entries.len(), 2);

        // First file must start at offset 12 (right after header)
        assert_eq!(entries[0].1, 12, "first file should start at byte 12");
        // Second file must immediately follow the first (12 + 4 bytes = 16)
        assert_eq!(
            entries[1].1,
            12 + entries[0].2,
            "second file should follow first with no gap"
        );
    }

    #[test]
    fn repack_empty_allowlist_match_produces_valid_empty_pak() {
        let src = make_pak(&[("maps/base1.bsp", b"BSP_DATA")]);

        let allow: HashSet<String> = ["wav"].iter().map(|s| s.to_string()).collect();
        let out = repack(&src, Some(&allow)).unwrap();

        // Must still be a valid PAK with 0 entries
        assert_eq!(&out[0..4], PAK_MAGIC);
        let entries = parse_dir(&out);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn repack_invalid_magic_returns_error() {
        let not_a_pak = b"NOT_A_PAK_FILE_AT_ALL_LONG_ENOUGH";
        let result = repack(not_a_pak, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("bad magic"));
    }

    #[test]
    fn repack_too_short_input_returns_error() {
        let result = repack(b"PACK", None); // valid magic but no header
        assert!(result.is_err());
    }

    #[test]
    fn repack_output_is_itself_a_valid_pak() {
        // Round-trip: repacking a repacked pak must succeed and preserve content
        let src = make_pak(&[("maps/base1.bsp", b"BSP"), ("sound/boom.wav", b"WAV")]);

        let first_pass = repack(&src, None).unwrap();
        let second_pass = repack(&first_pass, None).unwrap();

        let entries = parse_dir(&second_pass);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn repack_extension_filter_is_case_insensitive() {
        // PAK entry names from original Q2 often have mixed case
        let src = make_pak(&[
            ("maps/BASE1.BSP", b"BSP_DATA"),
            ("sound/boom.WAV", b"WAV_DATA"),
        ]);

        let allow: HashSet<String> = ["bsp"].iter().map(|s| s.to_string()).collect();
        let out = repack(&src, Some(&allow)).unwrap();

        let entries = parse_dir(&out);
        assert_eq!(
            entries.len(),
            1,
            "BSP (uppercase) should match lowercase filter"
        );
    }

    #[test]
    fn repack_output_magic_is_preserved() {
        let src = make_pak(&[("a.bsp", b"data")]);
        let out = repack(&src, None).unwrap();
        assert_eq!(&out[0..4], PAK_MAGIC);
    }

    #[test]
    fn repack_multi_extension_allowlist_keeps_all_matching() {
        let src = make_pak(&[
            ("maps/base1.bsp", b"BSP"),
            ("sound/boom.wav", b"WAV"),
            ("textures/wall.wal", b"WAL"),
            ("models/player.md2", b"MD2"),
        ]);

        let allow: HashSet<String> = ["bsp", "wav"].iter().map(|s| s.to_string()).collect();
        let out = repack(&src, Some(&allow)).unwrap();

        let entries = parse_dir(&out);
        assert_eq!(entries.len(), 2);
        let names: Vec<&str> = entries.iter().map(|(n, _, _)| n.as_str()).collect();
        assert!(names.contains(&"maps/base1.bsp"));
        assert!(names.contains(&"sound/boom.wav"));
        assert!(!names.contains(&"textures/wall.wal"));
        assert!(!names.contains(&"models/player.md2"));
    }

    fn allow(exts: &[&str]) -> HashSet<String> {
        exts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn repack_allowlist_ignores_extensionless_names() {
        // A root file literally named "bsp" has no extension and must not match "bsp".
        let src = make_pak(&[("bsp", b"X"), ("maps/readme", b"Y")]);
        let out = repack(&src, Some(&allow(&["bsp", "maps/readme"]))).unwrap();
        assert_eq!(parse_dir(&out).len(), 0);
    }

    #[test]
    fn repack_allowlist_uses_extension_of_file_name_not_directory() {
        let src = make_pak(&[("dir.v2/file", b"X")]);
        let out = repack(&src, Some(&allow(&["v2", "v2/file"]))).unwrap();
        assert_eq!(parse_dir(&out).len(), 0);
    }

    #[test]
    fn repack_rejects_directory_length_not_multiple_of_entry_size() {
        let mut src = make_pak(&[("a.bsp", b"data")]);
        src[8..12].copy_from_slice(&63u32.to_le_bytes());
        let err = repack(&src, None).unwrap_err().to_string();
        assert!(err.contains("not a multiple of 64"), "{err}");
    }

    #[test]
    fn repack_rejects_directory_past_end_of_file() {
        let mut src = make_pak(&[("a.bsp", b"data")]);
        src[8..12].copy_from_slice(&128u32.to_le_bytes());
        let err = repack(&src, None).unwrap_err().to_string();
        assert!(err.contains("directory extends past end"), "{err}");
    }

    #[test]
    fn repack_rejects_entry_data_past_end_of_file() {
        let mut src = make_pak(&[("a.bsp", b"data")]);
        let dir_offset = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
        let size_at = dir_offset + NAME_LEN + 4;
        src[size_at..size_at + 4].copy_from_slice(&1_000u32.to_le_bytes());
        let err = repack(&src, None).unwrap_err().to_string();
        assert!(err.contains("'a.bsp' data extends past end"), "{err}");
    }

    #[test]
    fn entry_count_reads_directory_length() {
        let src = make_pak(&[("a.bsp", b"1"), ("b.bsp", b"2")]);
        assert_eq!(entry_count(&src), 2);
        assert_eq!(entry_count(b"PACK"), 0);
    }

    #[test]
    fn repack_rejects_output_larger_than_limit() {
        // Two directory entries aliasing the same 8 source bytes produce 16 bytes of output data.
        let mut src = make_pak(&[("a.bsp", b"ABCDEFGH"), ("b.bsp", b"ABCDEFGH")]);
        let dir_offset = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
        let second_offset_at = dir_offset + ENTRY_SIZE + NAME_LEN;
        src[second_offset_at..second_offset_at + 4].copy_from_slice(&12u32.to_le_bytes());

        let needed = 12 + 16 + 2 * ENTRY_SIZE;
        assert!(repack_with_limit(&src, None, needed).is_ok());
        let err = repack_with_limit(&src, None, needed - 1)
            .unwrap_err()
            .to_string();
        assert!(err.contains("exceeds"), "{err}");
    }

    #[test]
    fn write_brotli_round_trips() {
        let pak = make_pak(&[("maps/base1.bsp", b"BSP_DATA_BSP_DATA_BSP_DATA")]);
        let mut compressed = Vec::new();
        write_brotli(&pak, &mut compressed).unwrap();
        let mut decompressed = Vec::new();
        brotli::BrotliDecompress(&mut &compressed[..], &mut decompressed).unwrap();
        assert_eq!(decompressed, pak);
    }

    struct FailingWriter;
    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_brotli_surfaces_write_errors() {
        let err = write_brotli(b"some pak bytes", &mut FailingWriter).unwrap_err();
        assert!(format!("{err:#}").contains("disk full"), "{err:#}");
    }

    #[test]
    fn write_outputs_with_brotli_writes_pak_and_matching_br() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("sub/pak0-web.pak");
        let pak = make_pak(&[("a.bsp", b"data")]);

        write_outputs(&out, &pak, true).unwrap();

        assert_eq!(fs::read(&out).unwrap(), pak);
        let compressed = fs::read(br_path(&out)).unwrap();
        let mut decompressed = Vec::new();
        brotli::BrotliDecompress(&mut &compressed[..], &mut decompressed).unwrap();
        assert_eq!(decompressed, pak);
        let leftovers: Vec<_> = fs::read_dir(out.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
    }

    #[test]
    fn write_outputs_without_brotli_removes_stale_br() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("pak0-web.pak");
        fs::write(br_path(&out), b"stale").unwrap();

        write_outputs(&out, &make_pak(&[("a.bsp", b"data")]), false).unwrap();

        assert!(out.exists());
        assert!(!br_path(&out).exists(), "stale .br must be removed");
    }

    #[test]
    fn write_outputs_without_brotli_and_no_br_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("pak0-web.pak");
        write_outputs(&out, &make_pak(&[]), false).unwrap();
        assert!(out.exists());
    }

    #[test]
    fn allowlist_accepts_dotted_and_glob_spellings() {
        assert_eq!(
            parse_allowlist("bsp, .CFG,*.pcx").unwrap(),
            allow(&["bsp", "cfg", "pcx"])
        );
    }

    #[test]
    fn allowlist_without_any_extension_is_an_error() {
        for raw in ["", " , ", ".", "*."] {
            assert!(parse_allowlist(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn br_path_appends_suffix() {
        assert_eq!(
            br_path(std::path::Path::new("g/pak0-web.pak")),
            PathBuf::from("g/pak0-web.pak.br")
        );
    }
}
