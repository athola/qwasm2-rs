# PAK Web Filter Pipeline — Implementation Plan

**Date**: 2026-05-01  
**Spec**: `docs/specification.md`  
**Target**: 47.6 MB → ~6.2 MB (~2.1 MB with Brotli) via filtered web pak

## Architecture

```
[Source]                [Build-time]              [Runtime]
pak0.pak (47.6 MB)  →  q2-pak-repack  →  pak0-web.pak (6.2 MB)
                            ↓ --brotli               ↓
                       pak0-web.pak.br (2.1 MB)  [devserver]
                                                      ↓
                                               browser fetch
                                               (Content-Encoding: br)
```

No runtime engine changes. `JsPakReader` + `FileSystem` unchanged.

## File Structure

| File | Action | Task |
|------|--------|------|
| `crates/q2-pak-repack/Cargo.toml` | Create | T001 |
| `crates/q2-pak-repack/src/main.rs` | Create | T001 |
| `Cargo.toml` | Add workspace member | T002 |
| `Makefile` | Add pak-web targets, update play/clean | T003 |
| `crates/q2-bundler/src/main.rs` | Change pak URL string | T004 |
| `crates/q2-devserver/src/main.rs` | Brotli middleware + log | T005 |

## Tasks

### T001 — Create q2-pak-repack crate

**Effort**: S (2–3 h)  
**Dependencies**: None  
**Risk**: Low — pure Rust, no external deps beyond clap + optional brotli crate

Create `crates/q2-pak-repack/Cargo.toml`:
```toml
[package]
name = "q2-pak-repack"
version.workspace = true
edition.workspace = true

[[bin]]
name = "q2-pak-repack"
path = "src/main.rs"

[dependencies]
anyhow = { workspace = true }
clap = { version = "4", features = ["derive"] }
brotli = "7"
```

Create `crates/q2-pak-repack/src/main.rs`:

```rust
use anyhow::{bail, Context, Result};
use clap::Parser;
use std::{
    collections::HashSet,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Parser)]
#[command(name = "q2-pak-repack")]
struct Args {
    #[arg(long)]
    r#in: PathBuf,

    #[arg(long)]
    out: PathBuf,

    #[arg(long, default_value = "bsp,cfg,lst,pk,map")]
    allow: String,

    #[arg(long)]
    brotli: bool,
}

const PAK_MAGIC: &[u8; 4] = b"PACK";
const ENTRY_SIZE: usize = 64;
const NAME_LEN: usize = 56;

struct Entry {
    name: String,
    offset: u32,
    size: u32,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let allow: HashSet<String> = args
        .allow
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .collect();

    let src = fs::read(&args.r#in)
        .with_context(|| format!("reading {}", args.r#in.display()))?;

    if src.len() < 12 || &src[0..4] != PAK_MAGIC {
        bail!("{}: not a valid PAK file", args.r#in.display());
    }

    let dir_offset = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
    let dir_len = u32::from_le_bytes(src[8..12].try_into().unwrap()) as usize;

    if dir_offset + dir_len > src.len() {
        bail!("{}: directory extends past end of file", args.r#in.display());
    }

    let num_files = dir_len / ENTRY_SIZE;
    let mut entries: Vec<Entry> = Vec::new();

    for i in 0..num_files {
        let base = dir_offset + i * ENTRY_SIZE;
        let name_bytes = &src[base..base + NAME_LEN];
        let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        let name = std::str::from_utf8(&name_bytes[..name_end])
            .unwrap_or("")
            .replace('\\', "/")
            .to_lowercase();
        let offset = u32::from_le_bytes(src[base + NAME_LEN..base + NAME_LEN + 4].try_into().unwrap());
        let size = u32::from_le_bytes(src[base + NAME_LEN + 4..base + NAME_LEN + 8].try_into().unwrap());

        let ext = name.rsplit('.').next().unwrap_or("").to_string();
        if allow.contains(&ext) {
            entries.push(Entry { name, offset, size });
        }
    }

    if entries.is_empty() {
        eprintln!("warning: no files matched allow list {:?}; writing empty PAK", allow);
    }

    // Build output PAK
    let mut out: Vec<u8> = Vec::with_capacity(src.len() / 2);
    out.extend_from_slice(PAK_MAGIC);
    out.extend_from_slice(&[0u8; 8]); // dir_offset + dir_len placeholder

    // Write file data, update offsets
    let mut placed: Vec<(String, u32, u32)> = Vec::with_capacity(entries.len());
    for entry in &entries {
        let file_offset = out.len() as u32;
        let s = entry.offset as usize;
        let e = s + entry.size as usize;
        if e > src.len() {
            bail!("{}: entry '{}' extends past end of file", args.r#in.display(), entry.name);
        }
        out.extend_from_slice(&src[s..e]);
        placed.push((entry.name.clone(), file_offset, entry.size));
    }

    // Write directory
    let dir_offset_out = out.len() as u32;
    for (name, offset, size) in &placed {
        let mut name_buf = [0u8; NAME_LEN];
        let nb = name.as_bytes();
        let copy = nb.len().min(NAME_LEN - 1);
        name_buf[..copy].copy_from_slice(&nb[..copy]);
        out.extend_from_slice(&name_buf);
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
    }

    let dir_len_out = (placed.len() * ENTRY_SIZE) as u32;
    out[4..8].copy_from_slice(&dir_offset_out.to_le_bytes());
    out[8..12].copy_from_slice(&dir_len_out.to_le_bytes());

    // Write output
    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.out, &out)
        .with_context(|| format!("writing {}", args.out.display()))?;

    let src_mb = src.len() as f64 / 1e6;
    let out_mb = out.len() as f64 / 1e6;
    let reduction = (1.0 - out.len() as f64 / src.len() as f64) * 100.0;
    println!(
        "{}: {:.1} MB → {:.2} MB ({} files, {:.0}% reduction)",
        args.out.display(), src_mb, out_mb, placed.len(), reduction
    );

    // Optional Brotli
    if args.brotli {
        let br_path = PathBuf::from(format!("{}.br", args.out.display()));
        let br_file = fs::File::create(&br_path)
            .with_context(|| format!("creating {}", br_path.display()))?;
        let mut writer = brotli::CompressorWriter::new(br_file, 4096, 11, 22);
        writer.write_all(&out)?;
        writer.flush()?;
        drop(writer);
        let br_mb = fs::metadata(&br_path)?.len() as f64 / 1e6;
        println!("{}: {:.2} MB (brotli level 11)", br_path.display(), br_mb);
    }

    Ok(())
}
```

**Acceptance**: `cargo build -p q2-pak-repack` compiles clean with zero warnings.

---

### T002 — Add q2-pak-repack to workspace

**Effort**: XS (5 min)  
**Dependencies**: T001 (directory must exist)  
**Risk**: None

In `Cargo.toml`, add `"crates/q2-pak-repack"` to the `members` list.

**Acceptance**: `cargo build --workspace` compiles without errors.

---

### T003 — Makefile: pak-web target + updates

**Effort**: XS (15 min)  
**Dependencies**: T001, T002  
**Risk**: None

Add to Makefile:

```make
PAK_WEB_ALLOW := bsp,cfg,lst,pk,map

prereq-pak-repack: ## Check q2-pak-repack builds
    @cargo build -p q2-pak-repack --quiet 2>&1 | grep -E "^error" && exit 1 || true
    @echo "$(OK) q2-pak-repack"

pak-web: prereq-pak-repack gamedata-check ## Build filtered web pak (~6 MB from ~48 MB)
    cargo run -p q2-pak-repack --release -- \
        --in "$(GAMEDATA)/pak0.pak" \
        --out "$(GAMEDATA)/pak0-web.pak" \
        --allow "$(PAK_WEB_ALLOW)" \
        --brotli
    @echo "$(OK) $(GAMEDATA)/pak0-web.pak"
```

Update existing targets:
- `prereqs`: add `prereq-pak-repack`
- `play`: add `pak-web` dependency
- `play-release`: add `pak-web` dependency
- `clean`: add `rm -f "$(GAMEDATA)/pak0-web.pak" "$(GAMEDATA)/pak0-web.pak.br"`

**Acceptance**: `make pak-web` runs, produces `gamedata/baseq2/pak0-web.pak` ≤ 8 MB
and `pak0-web.pak.br` ≤ 3 MB.

---

### T004 — Update q2-bundler pak URL

**Effort**: XS (5 min)  
**Dependencies**: None  
**Risk**: None

In `crates/q2-bundler/src/main.rs`, in the `generate_html` function, change:

```js
const pakUrl = '/gamedata/baseq2/pak0.pak';
```

to:

```js
const pakUrl = '/gamedata/baseq2/pak0-web.pak';
```

**Acceptance**: `cargo build -p q2-bundler` passes. `grep pak0-web crates/q2-bundler/src/main.rs`
returns a match.

---

### T005 — Update q2-devserver: log + Brotli serving

**Effort**: S (1 h)  
**Dependencies**: T003 (pak0-web.pak must exist to test)  
**Risk**: Low — new handler layer is additive, fallback is transparent

Two changes to `crates/q2-devserver/src/main.rs`:

**1. Startup log** — add after existing pak0.pak log:
```rust
let web_pak = std::path::Path::new("gamedata/baseq2/pak0-web.pak");
if web_pak.exists() {
    let size = std::fs::metadata(web_pak)
        .map(|m| m.len() as f64 / 1e6)
        .unwrap_or(0.0);
    tracing::info!("gamedata/baseq2/pak0-web.pak ({:.1} MB) — web-optimized", size);
    let br_pak = std::path::Path::new("gamedata/baseq2/pak0-web.pak.br");
    if br_pak.exists() {
        let br_size = std::fs::metadata(br_pak)
            .map(|m| m.len() as f64 / 1e6)
            .unwrap_or(0.0);
        tracing::info!("gamedata/baseq2/pak0-web.pak.br ({:.1} MB) — brotli variant", br_size);
    }
} else {
    tracing::warn!("pak0-web.pak not found — run: make pak-web");
}
```

**2. Brotli route** — add a custom handler that intercepts `GET /gamedata/baseq2/pak0-web.pak`
and serves `.br` variant if it exists and client sends `Accept-Encoding: br`:

Add a new axum handler:
```rust
async fn serve_pak(
    axum::extract::State(gamedata_dir): axum::extract::State<std::path::PathBuf>,
    headers: axum::http::HeaderMap,
) -> impl axum::response::IntoResponse {
    let pak_path = gamedata_dir.join("baseq2/pak0-web.pak");
    let br_path = gamedata_dir.join("baseq2/pak0-web.pak.br");

    let accept_br = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("br"))
        .unwrap_or(false);

    if accept_br && br_path.exists() {
        match tokio::fs::read(&br_path).await {
            Ok(bytes) => axum::response::Response::builder()
                .header("Content-Type", "application/octet-stream")
                .header("Content-Encoding", "br")
                .header("Cache-Control", "public, max-age=3600")
                .body(axum::body::Body::from(bytes))
                .unwrap(),
            Err(_) => serve_plain(&pak_path).await,
        }
    } else {
        serve_plain(&pak_path).await
    }
}

async fn serve_plain(path: &std::path::Path) -> axum::response::Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => axum::response::Response::builder()
            .header("Content-Type", "application/octet-stream")
            .body(axum::body::Body::from(bytes))
            .unwrap(),
        Err(_) => axum::response::Response::builder()
            .status(404)
            .body(axum::body::Body::empty())
            .unwrap(),
    }
}
```

Wire into the router before the fallback ServeDir:
```rust
let app = Router::new()
    .route(
        "/gamedata/baseq2/pak0-web.pak",
        axum::routing::get(serve_pak),
    )
    .with_state(std::path::PathBuf::from("gamedata"))
    .nest_service("/gamedata", ServeDir::new("gamedata"))
    .fallback_service(ServeDir::new("dist"))
    .layer(CorsLayer::permissive());
```

**Acceptance**: 
- `cargo build -p q2-devserver` passes.
- `curl -s -I -H "Accept-Encoding: br" http://localhost:8080/gamedata/baseq2/pak0-web.pak`
  returns `content-encoding: br` when `.br` file exists.

---

## Sprint Plan

| Sprint | Tasks | Effort | Goal |
|--------|-------|--------|------|
| 1 (today) | T001, T002, T003 | S + XS + XS | Repacker works, `make pak-web` produces filtered pak |
| 2 (today) | T004, T005 | XS + S | Bundler + devserver use web pak; Brotli served |

All tasks are independent enough to execute sequentially in one session.

## Critical Path

T001 → T002 → T003 (repacker must compile before Makefile target runs)

T004 and T005 are independent of T001–T003 and can be done in any order.

## Risks

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `brotli` crate API differs from expected | Low | Check docs before writing; crate is stable |
| Devserver Brotli handler breaks for other routes | Low | Route is specific path; fallback to ServeDir |
| pak0-web.pak missing when `make play` runs | Low | `play` depends on `pak-web`; explicit dep chain |
| Future texture landing silently misses assets | Low | Spec notes: expand `PAK_WEB_ALLOW` when adding wal/pcx |

## Definition of Done

- [ ] `make pak-web` → `pak0-web.pak` ≤ 8 MB + `pak0-web.pak.br` ≤ 3 MB
- [ ] `make play` serves the web pak, game renders first map
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace` zero errors
