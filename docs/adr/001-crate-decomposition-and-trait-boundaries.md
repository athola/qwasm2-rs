# ADR-001: Crate Decomposition and Trait Boundaries

**Status:** Accepted
**Date:** 2026-03-26

## Context

Quake 2's original C codebase is a monolithic binary with subsystems separated
by convention (file prefixes, header includes). Porting to Rust provides an
opportunity to enforce subsystem boundaries at the crate level.

Key constraints:
- The engine must compile to both WebAssembly (browser) and native targets.
- The renderer backend must be swappable (GL3/GLES3 now, Vulkan/software later).
- Game logic must be decoupled from the engine, matching Q2's original `game.dll`
  plugin model via function-pointer tables (`game_import_t`/`game_export_t`).
- Client and server can run in the same process or separately.

## Decision

### Crate Structure (14 crates in a Cargo workspace)

```
q2-shared       Foundation types, constants, protocol enums (no deps)
q2-common       Engine services: collision, pmove, filesystem, net_msg, cvars
q2-render-api   Renderer trait + data types (no implementation)
q2-render       GL3/GLES3 renderer (implements Renderer trait)
q2-game         Game logic: entity system, spawn, GameImport/GameExport traits
q2-server       Server state, frame loop, world
q2-client       Client state, parsing, input, view calculations
q2-platform     Platform abstraction (WASM input/GL context/game loop)
q2-net          Placeholder for P2P networking via WebRTC (no code or deps yet, #22)
q2-wasm         WASM cdylib entry point (orchestrator)
q2-bin          Placeholder for a native binary entry point (no deps yet)
q2-devserver    Axum dev server (standalone, no game deps)
q2-bundler      Bundles WASM + JS into a single HTML file (standalone)
q2-pak-repack   Repacks a PAK and writes a Brotli variant for web delivery (standalone)
```

### Trait Boundaries

Four trait interfaces enforce the key abstraction boundaries:

1. **`Renderer`** (q2-render-api) - 14 methods covering init, registration,
   rendering, 2D drawing. `Send`-safe, object-safe. Backends implement this
   trait without touching game logic.

2. **`GameImport`** (q2-game) - Server callbacks available to game code:
   printing, sound, collision queries, entity linking, networking. Replaces
   C `game_import_t` function-pointer table.

3. **`GameExport`** (q2-game) - Game interface exposed to server: init,
   spawn_entities, client lifecycle, run_frame. Replaces C `game_export_t`.

4. **`PakReader`** (q2-common) - Byte-range reader for PAK archive backends.
   `DiskPakReader` (native) opens the file per read. `InMemPakReader` slices
   an in-memory `Vec<u8>` (tests). `JsPakReader` (q2-platform, WASM only)
   slices a JS-heap `Uint8Array` — avoids copying the full PAK (~100 MB) into
   WASM linear memory until individual assets are requested.

### Dependency DAG

Dependencies flow strictly downward; no circular dependencies are permitted.
Layers, lowest first, with the workspace edges `cargo metadata` reports:

```
Foundation   q2-shared       -> (none)
Services     q2-common       -> q2-shared
             q2-render-api   -> q2-shared
Engine       q2-game         -> q2-shared, q2-common
             q2-client       -> q2-shared, q2-common
             q2-render       -> q2-shared, q2-common, q2-render-api
             q2-server       -> q2-shared, q2-common, q2-game
Adapter      q2-platform     -> q2-common
Entry point  q2-wasm         -> q2-shared, q2-common, q2-game, q2-client,
                                q2-render-api, q2-render, q2-platform
```

q2-common and q2-render-api are siblings: neither depends on the other.
q2-server is not yet used by an entry point. q2-bin, q2-net and the tools
(q2-devserver, q2-bundler, q2-pak-repack) have no workspace dependencies.

### Entity Storage

SlotMap with generational indices (`EntityKey`) replaces C's flat array +
free list. Prevents use-after-free bugs that were a common source of crashes
in the original engine.

### Platform Abstraction

Platform-specific code is isolated to `q2-platform` with `#[cfg(target_arch)]`
gating. WASM is the primary target; native SDL2 support is planned but not
yet implemented.

## Consequences

**Positive:**
- Subsystem boundaries are enforced by the compiler (no accidental coupling).
- Renderer and game logic are independently swappable.
- Each crate compiles and tests independently; parallel compilation.
- Unsafe code is contained to the GL FFI boundary in q2-render (8 blocks in
  `src/gl3`, each with a `// SAFETY` comment). Clippy denies undocumented
  unsafe blocks workspace-wide.

**Negative:**
- q2-common is a large crate (~8000 LOC) aggregating weakly-related modules.
  May need splitting if it exceeds ~15k LOC.
- Cross-crate refactoring requires coordinating changes across multiple
  Cargo.toml files.
- The standalone tools (devserver, bundler, pak-repack) and placeholders
  (bin, net) add workspace members that don't participate in the engine
  dependency graph.

## Wire Protocol Deviations from C Original

**Revised 2026-10-26.** The wire format targets C protocol 34, as CLAUDE.md
requires C parity for the network protocol. The earlier deviations (16-bit
entity angles instead of the C 8-bit `MSG_WriteAngle`, FRAME8|FRAME16 for
large frames, delta-encoded `event`) were removed in 4d6c420, encoder and
client decoder together (finding C-7 in
`docs/plans/2026-10-26-full-review-fixes.md`).

## Review Triggers

Re-evaluate this ADR when:
- q2-common exceeds 15,000 LOC
- A second renderer backend is added
- Native (non-WASM) platform support is implemented
- Multiplayer integration tests require new inter-crate boundaries
