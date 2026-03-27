# qwasm2-rs

Quake 2 engine rewrite in Rust, targeting WebAssembly.

Loads real Quake 2 game data (PAK files, BSP maps), renders with
WebGL2, and runs player movement in the browser.

> **Status: alpha.** The engine loads maps, renders BSP geometry, and
> supports first-person movement. Networking, sound, and game logic are
> stubbed or in progress. Expect breaking changes.

## Features

- 13-crate Cargo workspace with strict dependency DAG
  (compiler-enforced subsystem boundaries)
- BSP map loading and rendering via WebGL2 / GLES3
- Player movement faithful to the original C `pmove` code
- Single-file HTML bundle (`dist/qwasm2.html`) — no install needed
  for end users
- Automatic demo game data download (`make gamedata`)
- Native and WASM build targets from the same codebase

## Quick Start

```bash
# Clone and enter the repository
git clone https://github.com/<owner>/qwasm2-rs.git
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
| `make gamedata` | Download Quake 2 demo pak0.pak |
| `make test` | Run all native tests |
| `make lint` | Clippy lints (zero warnings) |
| `make fmt` | Format code |
| `make clean` | Remove build artifacts |

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

Standalone:  q2-devserver  q2-bundler  q2-platform  q2-net
```

### Trait Boundaries

Three traits enforce the key abstraction seams, replacing the C
function-pointer tables:

| Trait | Crate | Replaces |
|-------|-------|----------|
| `Renderer` | q2-render-api | Renderer backend interface |
| `GameImport` | q2-game | `game_import_t` (server → game callbacks) |
| `GameExport` | q2-game | `game_export_t` (game → server interface) |

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
| `q2-devserver` | Axum dev server for local development |
| `q2-bundler` | Packs WASM + HTML into a single file |

## Game Data

The engine reads standard Quake 2 `.pak` files. The easiest way to
get started is with the free demo data:

```bash
make gamedata       # downloads + extracts demo pak0.pak (~47 MB)
```

To use the full game, copy your `pak0.pak` (and optionally `pak1.pak`,
`pak2.pak`) from a Quake 2 installation into `gamedata/baseq2/`.

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

License not yet specified. See individual crate `Cargo.toml` files
for any per-crate license declarations.
