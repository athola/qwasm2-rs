# PAK Web Filter Pipeline — Specification

**Date**: 2026-05-01  
**Status**: Draft  
**Derived from**: `docs/project-brief.md`

## Overview

Produce `gamedata/baseq2/pak0-web.pak` — a filtered copy of `pak0.pak` containing
only the file extensions the engine currently reads — reducing the browser download
from 47.6 MB to ~6.2 MB (or ~2.1 MB with Brotli). Zero engine changes.

## Scope

**In scope**:
- `crates/q2-pak-repack/` — new native Rust CLI crate
- `Makefile` — `pak-web` and `prereq-pak-repack` targets
- `crates/q2-bundler/src/main.rs` — update hardcoded pak URL constant
- `crates/q2-devserver/src/main.rs` — serve `pak0-web.pak`; optional Brotli support
- `crates/q2-wasm/src/lib.rs` — remove hardcoded `/gamedata/baseq2/pak0.pak` assumption
  (pak URL already passed as argument; this is just a comment/log update)

**Out of scope**:
- Audio transcoding (WAV→Opus) — deferred until audio engine lands
- Texture transcoding (WAL→WebP) — deferred until texture loading lands
- HTTP Range request streaming
- Service Worker caching
- Changes to `q2-common::filesystem` or `q2-platform::wasm::pak`

## q2-pak-repack CLI

### Crate location
`crates/q2-pak-repack/` — binary crate (`[[bin]]`), not a library.

### Command-line interface

```
q2-pak-repack --in <path> --out <path> [--allow <ext1,ext2,...>] [--brotli]
```

| Flag | Required | Description |
|------|----------|-------------|
| `--in <path>` | Yes | Source PAK file path |
| `--out <path>` | Yes | Output PAK file path (created or overwritten) |
| `--allow <exts>` | No | Comma-separated lowercase extensions without dot. Default: `bsp,cfg,lst,pk,map` |
| `--brotli` | No | Also write `<out>.br` (Brotli, level 11). Requires `brotli` crate. |

### Behavior

1. Parse source PAK header (magic, dir_offset, dir_len). Exit non-zero on invalid magic.
2. Parse directory entries into a list of `(name, offset, size)` tuples.
3. Filter entries: keep only those whose filename extension (lowercased, after last `.`)
   is in the allowlist.
4. Write output PAK:
   - Write PACK header placeholder (12 bytes).
   - Sequentially write each retained file's raw bytes (read via `read_slice`).
   - Write directory at end.
   - Patch header with `dir_offset` and `dir_len`.
5. Print summary to stdout: source size, output size, file count, reduction %.
6. If `--brotli`: write `<out>.br` at Brotli level 11. Print compressed size.
7. Exit 0 on success.

### Output format

The output file is a valid Quake 2 PAK file. Offsets within the output file are
recalculated (not copied from source). File content bytes are byte-for-byte identical
to source.

### Dependencies (Cargo.toml)

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
brotli = { version = "7", optional = true }

[features]
default = ["brotli-compress"]
brotli-compress = ["dep:brotli"]
```

### Error handling

- Invalid source PAK → stderr + exit 1.
- Output directory does not exist → create parent dirs; fail with error if impossible.
- Zero files after filtering → warning on stderr; still write valid empty PAK; exit 0.
- `--brotli` on a target without the feature → compile-time error (feature-gated).

## Makefile targets

### `prereq-pak-repack`

```make
prereq-pak-repack: ## Check that q2-pak-repack binary can be built
    @cargo build -p q2-pak-repack --quiet 2>&1 | head -5 || \
        (echo "$(FAIL) q2-pak-repack failed to build"; exit 1)
    @echo "$(OK) q2-pak-repack"
```

### `pak-web`

```make
PAK_WEB_ALLOW := bsp,cfg,lst,pk,map

pak-web: prereq-pak-repack gamedata-check ## Build filtered web pak (pak0-web.pak)
    @cargo run -p q2-pak-repack --release -- \
        --in "$(GAMEDATA)/pak0.pak" \
        --out "$(GAMEDATA)/pak0-web.pak" \
        --allow "$(PAK_WEB_ALLOW)" \
        --brotli
    @echo "$(OK) $(GAMEDATA)/pak0-web.pak ready"
```

### `play` and `play-release` updates

Both targets gain a `pak-web` prerequisite:
```make
play: prereqs bundle gamedata-check pak-web ## Build everything + launch devserver
play-release: prereqs bundle-release gamedata-check pak-web ## Release build + devserver
```

### `clean` update

```make
clean: ## Remove build artifacts
    cargo clean
    rm -rf "$(WASM_PKG)" "$(DIST)"
    rm -f "$(GAMEDATA)/pak0-web.pak" "$(GAMEDATA)/pak0-web.pak.br"
```

## q2-bundler changes

In `crates/q2-bundler/src/main.rs`, `generate_html()` currently hardcodes:
```js
const pakUrl = '/gamedata/baseq2/pak0.pak';
```

Change to:
```js
const pakUrl = '/gamedata/baseq2/pak0-web.pak';
```

This is a one-line string change in the `generate_html` function (inside the JS template literal).

## q2-devserver changes

### Brotli serving

If `pak0-web.pak.br` exists alongside `pak0-web.pak`, serve it with:
```
Content-Encoding: br
Content-Type: application/octet-stream
```
when the client sends `Accept-Encoding: br` (all modern browsers do).

Implementation: add a custom handler layer before `ServeDir` that checks for `.br`
variants of requested files. If `.br` file exists and client accepts Brotli, serve it
with the appropriate headers.

Fallback: if `.br` file is absent or client doesn't send `Accept-Encoding: br`, serve
the uncompressed file via existing `ServeDir`. No error.

### Startup log

Add logging for `pak0-web.pak` alongside existing `pak0.pak` log:
```rust
let web_pak = Path::new("gamedata/baseq2/pak0-web.pak");
if web_pak.exists() {
    let size = fs::metadata(web_pak).map(|m| m.len()).unwrap_or(0);
    tracing::info!("gamedata/baseq2/pak0-web.pak ({:.1} MB)", size as f64 / 1e6);
} else {
    tracing::warn!("pak0-web.pak not found — run: make pak-web");
}
```

## Acceptance Criteria

### AC-1: Repacker output is valid and byte-faithful

Given `pak0.pak` (any valid Q2 PAK):
- `pak0-web.pak` opens successfully via `Pack::open()` in `q2-common::filesystem`
- Every retained file's bytes are byte-identical to the source (MD5 match)
- No file from an excluded extension appears in the output directory

### AC-2: Size reduction

For the demo `pak0.pak` (47.6 MB):
- `pak0-web.pak` ≤ 8 MB uncompressed
- `pak0-web.pak.br` ≤ 3 MB

### AC-3: Game runs against web pak

`make play` (using `pak0-web.pak`) results in:
- Engine finds at least one map via `fs.list_files("bsp")`
- First map loads and renders (WebGL canvas non-black)
- Playwright smoke test passes

### AC-4: Makefile is idempotent

Running `make pak-web` twice produces the same output files. Second run completes
without error.

### AC-5: `cargo test --workspace` passes

All existing tests pass. No new test failures introduced.

### AC-6: CLI error handling

- `q2-pak-repack --in /nonexistent.pak --out /tmp/out.pak` exits non-zero with error on stderr.
- `q2-pak-repack --in pak0.pak --out /tmp/out.pak --allow xyz` (no matching files) exits 0,
  produces a valid empty PAK, prints a warning.

## File Tree After Implementation

```
crates/
  q2-pak-repack/
    Cargo.toml
    src/
      main.rs          (~150 LOC)
gamedata/
  baseq2/
    pak0.pak           (unchanged, 47.6 MB — source of truth)
    pak0-web.pak       (generated, ~6.2 MB)
    pak0-web.pak.br    (generated, ~2.1 MB)
```

## Extension Allowlist Rationale

| Extension | Included | Reason |
|-----------|----------|--------|
| `bsp` | ✅ | Engine reads maps via `fs.list_files("bsp")` + `fs.load_file()` |
| `cfg` | ✅ | `default.cfg` — may be read in future config phase; tiny (2.8 KB) |
| `lst` | ✅ | `maps.lst` — map listing; tiny (0.1 KB) |
| `pk` | ✅ | `sound/world/lava1.pk` — tiny (10.9 KB); `.pk` may be read as config |
| `map` | ✅ | `sound/misc/bigtele.map` — tiny (41.8 KB) |
| `wav` | ❌ | 28.4 MB; audio is stubbed (`sound()` is no-op) |
| `pcx` | ❌ | 6.3 MB; renderer does not load PCX from pak |
| `wal` | ❌ | 2.2 MB; renderer does not load WAL from pak |
| `md2` | ❌ | 3.2 MB; model rendering not implemented |
| `tga` | ❌ | 1.1 MB; not loaded |

The allowlist is a Makefile variable (`PAK_WEB_ALLOW`). As the engine gains
functionality, the variable expands: `PAK_WEB_ALLOW := bsp,cfg,lst,pk,map,wal,pcx`.
