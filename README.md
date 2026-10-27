[![CI](https://github.com/athola/qwasm2-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/athola/qwasm2-rs/actions/workflows/ci.yml)

# qwasm2-rs

Quake 2 engine rewrite in Rust, targeting WebAssembly.

Loads real Quake 2 game data (PAK files, BSP maps), renders with
WebGL2, and runs player movement in the browser.

> **Status: alpha.** The browser build loads a demo map into an
> in-process server running the full game logic (`q2-game`: physics,
> combat, items, weapons, AI, 20 monster types) and moves the player with
> the C pmove port through the server's collision. Doors, platforms and
> buttons collide and open, triggers fire on the player, and items can be
> picked up. The renderer draws the world textured and lightmapped with sky
> and warp surfaces, brush entities and MD2 models, and the status bar.
> Weapons, animated light styles and sound are not in yet (#16, #20).
> Expect breaking changes.

## Progress

| Checkpoint | What it proves | Status |
|------------|----------------|--------|
| CP-0 Scaffold | Workspace compiles to WASM | done |
| CP-1 Types & Physics | Types serialize, collision traces pass | done |
| CP-2 Game Logic | Entity spawn, GameExport trait, AI frame | done |
| CP-3 Server | Frame loop runs the game; map loading, SV_Trace and entity links; the browser runs it in-process | done |
| CP-4 Client Delta | `parse_frame`, player-state + entity delta | in progress |
| CP-5 First Frame | Textured BSP map renders in browser | done |
| CP-5b Playable | WASD + mouselook, HUD, rendered world | in progress (no weapons yet) |
| CP-7 Multiplayer | Two browser tabs connect via Matchbox | — |

## Features

- 14-crate Cargo workspace with strict dependency DAG
  (compiler-enforced subsystem boundaries)
- BSP map loading and rendering via WebGL2 / GLES3
- Player movement: C `pmove` in `ClientThink`, tracing through the
  server's map and linked entities
- `PakReader` trait with lazy WASM backend — avoids copying the full
  PAK (~100 MB) into linear memory until individual assets are requested
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta,
  packet-entity delta (30 tests)
- Server frame loop with `GameImport`/`GameExport` traits replacing C
  function-pointer tables; the browser runs server and game in one process
  (`q2_server::local::LocalGame`)
- Single-file HTML bundle (`dist/qwasm2.html`) — no install needed
  for end users
- Automatic demo game data download (`make gamedata`)
- Web-optimized `pak0-web.pak` produced by `make pak-web`, Brotli-compressed
  at level 11 and served with content negotiation; browsers receive ~28 MB
  instead of ~50 MB raw
- WASM is the only target that runs the engine; the native binary
  (`q2-bin`) is a placeholder

## Quick Start

```bash
# Clone and enter the repository
git clone https://github.com/athola/qwasm2-rs.git
cd qwasm2-rs

# Check prerequisites, build everything, and launch
make play
```

This builds the WASM module, bundles it into a single HTML file,
downloads the Quake 2 demo installer (~39 MB) if pak0.pak is missing, and starts a
local dev server. Open `http://127.0.0.1:8080/qwasm2.html`.

### Prerequisites

| Tool | Purpose | Install |
|------|---------|---------|
| Rust 1.97.1 | Compilation (pinned in `rust-toolchain.toml`) | [rustup.rs](https://rustup.rs); rustup installs the pinned toolchain |
| `wasm32-unknown-unknown` target | WASM builds | Installed by rustup from `rust-toolchain.toml` |
| wasm-pack | WASM packaging | `cargo install wasm-pack` |
| 7z | Demo data extraction (only when pak0.pak is missing) | `brew install p7zip` / `apt install p7zip-full` |
| curl | Demo data download (only when pak0.pak is missing) | Usually pre-installed |
| npm | Browser tests (`make test-browser`) | [nodejs.org](https://nodejs.org) |

Run `make prereqs` to verify all tools are present.

## Build Targets

| Command | Description |
|---------|-------------|
| `make play` | Full build + dev server (recommended) |
| `make play-release` | Release build + dev server |
| `make wasm` | Build WASM module only |
| `make bundle` | WASM + single-file HTML bundle |
| `make build` | Native build (all crates) |
| `make devserver` | Run dev server without rebuilding |
| `make gamedata` | Download Quake 2 demo pak0.pak |
| `make pak-web` | Repack game data and Brotli-compress for web delivery |
| `make check` | Type-check native + WASM targets without full build |
| `make test` | Run all workspace tests (unit, integration, doc) |
| `make test-browser` | Run Playwright browser tests (requires npm) |
| `make lint` | Clippy on all native targets + wasm32 libraries (zero warnings) |
| `make ci` | fmt-check, lint, check and test, as CI runs them |
| `make fmt` | Format code |
| `make fmt-check` | Check formatting without modifying files |
| `make clean` | Remove build artifacts |
| `make clean-gamedata` | Delete `gamedata/baseq2/pak0.pak` and the web paks (including a copied retail pak0.pak) |

## Architecture

14 crates organized in a strict DAG with no circular dependencies.
See [ADR-001](docs/adr/001-crate-decomposition-and-trait-boundaries.md)
for the rationale. Layers, lowest first (workspace edges per crate are in
the table below):

```
Foundation:    q2-shared
Services:      q2-common, q2-render-api      (each depends only on q2-shared)
Engine:        q2-game, q2-client, q2-render, q2-server
Adapter:       q2-platform
Entry point:   q2-wasm (browser)
Placeholders:  q2-bin (native), q2-net (networking, #22)    (no dependencies)
Tools:         q2-devserver, q2-bundler, q2-pak-repack      (no workspace dependencies)
```

q2-server is not yet a dependency of any entry point.

### Trait Boundaries

Four traits enforce the key abstraction seams, replacing the C
function-pointer tables and backend coupling:

| Trait | Crate | Replaces |
|-------|-------|----------|
| `Renderer` | q2-render-api | Renderer backend interface |
| `GameImport` | q2-game | `game_import_t` (server → game callbacks) |
| `GameExport` | q2-game | `game_export_t` (game → server interface) |
| `PakReader` | q2-common | PAK archive byte-range reader (disk / in-mem / JS heap) |

### Project Structure

| Crate | Description | Workspace dependencies |
|-------|-------------|------------------------|
| `q2-shared` | Foundation types, constants, protocol enums | none |
| `q2-common` | Engine services: collision, pmove, filesystem, cvars | q2-shared |
| `q2-render-api` | Renderer trait + data types (no implementation) | q2-shared |
| `q2-render` | GL3/GLES3 renderer backend | q2-shared, q2-common, q2-render-api |
| `q2-game` | Game logic: entities (SlotMap), spawn, physics, combat, items, weapons, AI, 20 monsters, player | q2-shared, q2-common |
| `q2-server` | Server state, frame loop, world | q2-shared, q2-common, q2-game |
| `q2-client` | Client state, parsing, input, view | q2-shared, q2-common |
| `q2-platform` | Platform abstraction (WASM input, GL context, PAK reader) | q2-common |
| `q2-wasm` | WASM cdylib entry point | q2-shared, q2-common, q2-game, q2-server, q2-client, q2-render-api, q2-render, q2-platform |
| `q2-net` | Placeholder for P2P networking (WebRTC via Matchbox, #22) | none |
| `q2-bin` | Placeholder for a native binary | none |
| `q2-devserver` | Axum dev server for local development (Brotli content negotiation) | none |
| `q2-bundler` | Packs WASM + HTML into a single file | none |
| `q2-pak-repack` | CLI tool: repack (optionally filter) and Brotli-compress PAK files | none |

## Game Data

The engine reads standard Quake 2 `.pak` files. The easiest way to
get started is with the free demo data:

```bash
make gamedata       # downloads + extracts demo pak0.pak (~50 MB)
make pak-web        # repack + Brotli-compress → pak0-web.pak{,.br}
```

`make play` runs both steps automatically. The devserver then serves
`pak0-web.pak.br` (Brotli, ~28 MB) to browsers that advertise
`Accept-Encoding: br`, and falls back to the uncompressed `pak0-web.pak`
otherwise or when the `.br` is older than the pak. `make pak-web` only
reruns when `pak0.pak` or the repacker changes.

To use the full game, copy your `pak0.pak` (and optionally `pak1.pak`,
`pak2.pak`) from a Quake 2 installation into `gamedata/baseq2/`, then
run `make pak-web` to produce the web-optimized variant.

## Development

```bash
make ci             # everything below except fmt, as CI runs it
make test           # run all workspace tests
make lint           # clippy, native + wasm32 (zero warnings required)
make fmt            # format code
make fmt-check      # check formatting without modifying
make check          # type-check native + WASM targets
```

### Conventions

- **Commits:** conventional commits (`feat:`, `fix:`, `refactor:`,
  `test:`, `docs:`)
- **No global mutable state.** All state is instance-owned or passed
  by reference.
- **Unsafe code** only at FFI boundaries (q2-render). Every `unsafe`
  block requires a `// SAFETY:` comment.
- **Entity storage** uses SlotMap with generational indices — never
  raw pointers or array indices.
- **Physics constants** must match the C original exactly (numerical
  precision matters for prediction sync).

### Reference Implementation

The original C Quake 2 source lives at `~/Qwasm2`. Algorithmic
modules (collision, pmove) are faithful ports — cross-reference
against the C source before modifying.

## Documentation

- [ADR-001: Crate Decomposition](docs/adr/001-crate-decomposition-and-trait-boundaries.md)
  — workspace structure and trait boundary rationale
- [ADR-002: Conversion Strategy and Roadmap](docs/adr/002-conversion-strategy-and-roadmap.md)
  — rewrite approach, playability checkpoints, phase status
- [ADR-003: Game Logic Design](docs/adr/003-game-logic-design.md) —
  `GameWorld`, callbacks, scope of the q2-game port
- [ADR-004: Web Asset Delivery](docs/adr/004-web-asset-delivery.md) — PAK
  repacking, Brotli serving, lazy reads
- [ADR-005: C Parity and Quality Gates](docs/adr/005-c-parity-and-quality-gates.md)
  — protocol and physics parity rules, CI gates
- [CLAUDE.md](CLAUDE.md) — project-level instructions for AI-assisted
  development

## License

Dual-licensed under MIT or Apache-2.0 at your option.
