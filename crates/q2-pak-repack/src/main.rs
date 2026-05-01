use anyhow::{bail, Context, Result};
use clap::Parser;
use std::{collections::HashSet, fs, io::Write, path::PathBuf};

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

    /// Keep all files regardless of extension (default; mutually exclusive with --allow)
    #[arg(long, conflicts_with = "allow")]
    all: bool,

    /// Comma-separated lowercase extensions to keep (e.g. "bsp,cfg").
    /// When audio/texture transcoding lands, add those extensions here.
    #[arg(long)]
    allow: Option<String>,

    /// Also write <out>.br (Brotli level 11) for Content-Encoding: br serving
    #[arg(long)]
    brotli: bool,
}

pub const PAK_MAGIC: &[u8; 4] = b"PACK";
pub const ENTRY_SIZE: usize = 64;
pub const NAME_LEN: usize = 56;

fn main() -> Result<()> {
    let args = Args::parse();

    let allowlist: Option<HashSet<String>> = args
        .allow
        .as_deref()
        .map(|s| s.split(',').map(|e| e.trim().to_lowercase()).collect());

    let src = fs::read(&args.r#in).with_context(|| format!("reading {}", args.r#in.display()))?;

    let out = repack(&src, allowlist.as_ref())?;

    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(&args.out, &out).with_context(|| format!("writing {}", args.out.display()))?;

    let src_mb = src.len() as f64 / 1_000_000.0;
    let out_mb = out.len() as f64 / 1_000_000.0;
    let retained = retained_count(&out);
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
        let br_path = PathBuf::from(format!("{}.br", args.out.display()));
        let br_file = fs::File::create(&br_path)
            .with_context(|| format!("creating {}", br_path.display()))?;
        let mut writer = brotli::CompressorWriter::new(br_file, 65536, 11, 22);
        writer
            .write_all(&out)
            .context("brotli compression failed")?;
        writer.flush()?;
        drop(writer);
        let br_mb = fs::metadata(&br_path)
            .map(|m| m.len() as f64 / 1_000_000.0)
            .unwrap_or(0.0);
        let br_pct = (1.0 - br_mb / out_mb) * 100.0;
        println!(
            "{}: {:.2} MB (brotli level 11, {:.0}% compression)",
            br_path.display(),
            br_mb,
            br_pct
        );
    }

    Ok(())
}

/// Repack a Q2 PAK, optionally filtering to an extension allowlist.
///
/// `allowlist = None` means keep every file.
/// `allowlist = Some(set)` keeps only files whose lowercase extension is in `set`.
///
/// Output is a valid PAK with all offsets recalculated from scratch (no gaps).
pub fn repack(src: &[u8], allowlist: Option<&HashSet<String>>) -> Result<Vec<u8>> {
    if src.len() < 12 || &src[0..4] != PAK_MAGIC {
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
    let mut kept: Vec<([u8; NAME_LEN], u32, &[u8])> = Vec::with_capacity(num_files);

    for i in 0..num_files {
        let base = dir_offset + i * ENTRY_SIZE;
        let raw_name = &src[base..base + NAME_LEN];
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

        if let Some(allow) = allowlist {
            let name_end = raw_name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name_str = std::str::from_utf8(&raw_name[..name_end])
                .unwrap_or("")
                .to_lowercase();
            let ext = name_str.rsplit('.').next().unwrap_or("").to_string();
            if !allow.contains(&ext) {
                continue;
            }
        }

        let start = offset as usize;
        let end = start.saturating_add(size as usize);
        if end > src.len() {
            let name_end = raw_name.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
            let name_str = std::str::from_utf8(&raw_name[..name_end]).unwrap_or("?");
            bail!("entry '{}' data extends past end of file", name_str);
        }

        let mut name_fixed = [0u8; NAME_LEN];
        let copy_len = raw_name.len().min(NAME_LEN);
        name_fixed[..copy_len].copy_from_slice(&raw_name[..copy_len]);

        kept.push((name_fixed, size, &src[start..end]));
    }

    // Build output PAK: header | file data... | directory
    let mut out: Vec<u8> = Vec::with_capacity(src.len() + 12);
    out.extend_from_slice(PAK_MAGIC);
    out.extend_from_slice(&[0u8; 8]); // placeholder for dir_offset + dir_len

    let mut placed: Vec<([u8; NAME_LEN], u32, u32)> = Vec::with_capacity(kept.len());
    for (name, size, data) in &kept {
        let file_offset = out.len() as u32;
        out.extend_from_slice(data);
        placed.push((*name, file_offset, *size));
    }

    let dir_offset_out = out.len() as u32;
    for (name, file_offset, size) in &placed {
        out.extend_from_slice(name);
        out.extend_from_slice(&file_offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }

    let dir_len_out = (placed.len() * ENTRY_SIZE) as u32;
    out[4..8].copy_from_slice(&dir_offset_out.to_le_bytes());
    out[8..12].copy_from_slice(&dir_len_out.to_le_bytes());

    Ok(out)
}

fn entry_count(pak: &[u8]) -> usize {
    if pak.len() < 12 {
        return 0;
    }
    let dir_len = u32::from_le_bytes(pak[8..12].try_into().unwrap()) as usize;
    dir_len / ENTRY_SIZE
}

fn retained_count(pak: &[u8]) -> usize {
    entry_count(pak)
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
}
