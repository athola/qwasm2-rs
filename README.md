[![CI](https://github.com/athola/qwasm2-rs/actions/workflows/ci.yml/badge.svg)](https://github.com/athola/qwasm2-rs/actions/workflows/ci.yml)

# qwasm2-rs

Quake 2 engine rewrite in Rust, targeting WebAssembly.

Loads real Quake 2 game data (PAK files, BSP maps), renders with
WebGL2, and runs player movement in the browser.

> **Status: alpha.** The server frame loop, game logic, and Q2 wire-protocol
> delta machinery are implemented. The renderer and full WASM platform
> integration are in progress. Expect breaking changes.

## Progress

| Checkpoint | What it proves | Status |
|------------|----------------|--------|
| CP-0 Scaffold | Workspace compiles to WASM | done |
| CP-1 Types & Physics | Types serialize, collision traces pass | done |
| CP-2 Game Logic | Entity spawn, GameExport trait, AI frame | done |
| CP-3 Server | Frame loop runs, GameImport bridge wired | done |
| CP-4 Client Delta | `parse_frame`, player-state + entity delta | in progress |
| CP-5 First Frame | Textured BSP map renders in browser | next |
| CP-5b Playable | WASD + mouselook, HUD, rendered world | — |
| CP-7 Multiplayer | Two browser tabs connect via Matchbox | — |

## Features

- 13-crate Cargo workspace with strict dependency DAG
  (compiler-enforced subsystem boundaries)
- BSP map loading and rendering via WebGL2 / GLES3
- Player movement faithful to the original C `pmove` code
- `PakReader` trait with lazy WASM backend — avoids copying the full
  PAK (~100 MB) into linear memory until individual assets are requested
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta,
  packet-entity delta (30 tests)
- Server frame loop with `GameImport`/`GameExport` bridge replacing C
  function-pointer tables
- Single-file HTML bundle (`dist/qwasm2.html`) — no install needed
  for end users
- Automatic demo game data download (`make gamedata`)
- Web-optimized `pak0-web.pak` produced by `make pak-web` — Brotli-compressed
  at level 11 and served with content negotiation; browsers receive ~26 MB
  instead of ~47 MB raw
- Native and WASM build targets from the same codebase

## Quick Start

```bash
# Clone and enter the repository
git clone https://github.com/athola/qwasm2-rs.git
cd qwasm2-rs

# Check prerequisites, build everything, and launch
make play
```

This builds the WASM module, bundles it into a single HTML file,
downloads the Quake 2 demo data (~47 MB) if needed, and starts a
local dev server. Open `http://127.0.0.1:8080/qwasm2.html`.

### Prerequisites

| Tool | Purpose | Install |
|------|---------|---------|
| Rust (stable) | Compilation | [rustup.rs](https://rustup.rs) |
| `wasm32-unknown-unknown` target | WASM builds | `rustup target add wasm32-unknown-unknown` |
| wasm-pack | WASM packaging | `cargo install wasm-pack` |
| 7z | Demo data extraction | `brew install p7zip` / `apt install p7zip-full` |
| curl | Downloading demo data | Usually pre-installed |

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
| `make serve` | Serve wasm-pack output via Python (no gamedata required) |
| `make gamedata` | Download Quake 2 demo pak0.pak |
| `make pak-web` | Repack game data and Brotli-compress for web delivery |
| `make check` | Type-check native + WASM targets without full build |
| `make test` | Run all native tests |
| `make test-browser` | Run Playwright browser tests (requires npx) |
| `make lint` | Clippy lints (zero warnings) |
| `make fmt` | Format code |
| `make fmt-check` | Check formatting without modifying files |
| `make clean` | Remove build artifacts |
| `make clean-gamedata` | Remove downloaded game data |

## Architecture

13 crates organized in a strict DAG — no circular dependencies.
See [ADR-001](docs/adr/001-crate-decomposition-and-trait-boundaries.md)
for the full rationale.

```
Entry points:  q2-wasm (browser)  q2-bin (native)
                     |                |
               q2-client  q2-server  q2-game  q2-render
                     \       |       /           |
                      q2-common            q2-render-api
                           |                    |
                        q2-shared (types, constants)

Standalone:  q2-devserver  q2-bundler  q2-platform  q2-pak-repack
Networking:  q2-net (depends on q2-shared, q2-common)
```

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

| Crate | Description |
|-------|-------------|
| `q2-shared` | Foundation types, constants, protocol enums |
| `q2-common` | Engine services: collision, pmove, filesystem, cvars |
| `q2-render-api` | Renderer trait + data types (no implementation) |
| `q2-render` | GL3/GLES3 renderer backend |
| `q2-game` | Game logic, entity system (SlotMap), spawn |
| `q2-server` | Server state, frame loop, world |
| `q2-client` | Client state, parsing, input, view |
| `q2-platform` | Platform abstraction (WASM input, GL context) |
| `q2-net` | P2P networking via WebRTC (matchbox_socket) |
| `q2-wasm` | WASM cdylib entry point |
| `q2-bin` | Native binary entry point |
| `q2-devserver` | Axum dev server for local development (Brotli content negotiation) |
| `q2-bundler` | Packs WASM + HTML into a single file |
| `q2-pak-repack` | CLI tool: filter and Brotli-compress PAK files for web delivery |

## Game Data

The engine reads standard Quake 2 `.pak` files. The easiest way to
get started is with the free demo data:

```bash
make gamedata       # downloads + extracts demo pak0.pak (~47 MB)
make pak-web        # repack + Brotli-compress → pak0-web.pak{,.br}
```

`make play` runs both steps automatically. The devserver then serves
`pak0-web.pak.br` (Brotli, ~26 MB) to browsers that advertise
`Accept-Encoding: br`, and falls back to the uncompressed `pak0-web.pak`
otherwise.

To use the full game, copy your `pak0.pak` (and optionally `pak1.pak`,
`pak2.pak`) from a Quake 2 installation into `gamedata/baseq2/`, then
run `make pak-web` to produce the web-optimized variant.

## Development

```bash
make test           # run all native tests
make lint           # clippy (zero warnings required)
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
- [CLAUDE.md](CLAUDE.md) — project-level instructions for AI-assisted
  development

## License

Dual-licensed under MIT or Apache-2.0 at your option.
