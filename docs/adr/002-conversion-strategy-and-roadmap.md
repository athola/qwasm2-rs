# ADR-002: C-to-Rust Conversion Strategy and Roadmap

**Status:** Accepted
**Date:** 2026-03-26 (consolidated 2026-10-26)

## Context

qwasm2-rs rewrites Qwasm2, the Emscripten build of Yamagi Quake II
(`~/Qwasm2/src`), in Rust. The goals are a browser build that uses
wasm-bindgen instead of Emscripten, peer-to-peer multiplayer over WebRTC, and
gameplay that matches the C original closely enough for client prediction to
agree with the server. This ADR replaces the conversion plan that lived in
`docs/superpowers/plans/2026-03-26-c-to-rust-conversion.md`.

## Decision

### Clean-room rewrite, bottom up

The port is written by hand from the lowest layer upward: shared types, then
common services, game logic, server, client, renderer, WASM platform, and
networking (ADR-001 gives the crate layout). Each phase must leave a build that
compiles and passes its tests. The numerically sensitive modules, collision
and pmove, are faithful ports checked line by line against the C source;
c2rust output was allowed only as a reference for those two modules, never as
shipped code.

The whole toolchain is Rust: the dev server, bundler and PAK repacker are
workspace binaries, so contributors need no Python or Node beyond the
optional Playwright browser tests.

### Playability checkpoints

Every phase ends at a checkpoint that a browser or test run can prove, and a
regression is bisected between the last passing checkpoint and HEAD before
work continues.

| Checkpoint | Proves |
|------------|--------|
| CP-0 | Workspace compiles to `wasm32-unknown-unknown` |
| CP-1 | Shared types and collision traces pass on native and WASM |
| CP-2 | Game logic spawns `info_player_start` and `monster_soldier` and runs an AI frame |
| CP-3 | Server frame loop runs 100 frames |
| CP-4 | Client parses serverdata, configstrings and frame deltas |
| CP-5 | First textured BSP frame in the browser |
| CP-5b | Playable in the browser: WASD, mouse look, HUD |
| CP-5c | Sound through Web Audio |
| CP-6 | Saves persist across a browser refresh |
| CP-7 | Two browser tabs play together over Matchbox |
| CP-8 | Single-player campaign map loads and plays |

### Unsafe policy

Unsafe code is allowed only at FFI boundaries, today the GL calls in
q2-render. Each block carries a `// SAFETY:` comment stating the invariant it
relies on, each file with unsafe code carries an audit header counting its
blocks, and the workspace denies `clippy::undocumented_unsafe_blocks`.

### Technology choices

wasm-bindgen, web-sys and js-sys for the browser boundary; `glow` for
OpenGL ES 3.0; `slotmap` for entities; `bitflags` and `glam` for flags and
math; `serde` for save games; `tracing` for logs; `axum` for the dev server;
`matchbox_socket` for WebRTC once Phase 7 starts. Audio (`cpal`, `lewton`)
and a native SDL2 backend are planned but have no code or dependencies yet.

## Status of the phases (2026-10-26)

| Phase | State |
|-------|-------|
| 0 Scaffolding | Done |
| 1 Common layer | Done: types, cvars, commands, net_msg, filesystem, collision, pmove, netchan |
| 2 Game logic | Core done, see ADR-003; gaps #14-#17 |
| 3 Server | Map loading, SV_Trace, entity links and the 10 Hz game frame done; client messages and baselines open (#10) |
| 4 Client | Protocol 34 delta parsing done; prediction, interpolation, view open (#19) |
| 5 Renderer | Flat-shaded BSP world renders; textures and lightmaps open (#20) |
| 6 WASM platform | Game loop, pointer lock, input and resize done; saves open (#21) |
| 7 P2P networking | Not started (#22) |
| 8 Integration | Server and game run in-process in the browser (`LocalGame`); networking open (#23) |

The browser build runs the server and the full game in one process and
moves the player with the pmove port through the server's traces. Still
missing in play: entity rendering, brush-entity collision (the game never
calls `set_model`), and trigger touches (#12).

## Consequences

Each phase is reviewable and testable on its own, and checkpoints catch
regressions at phase boundaries. The cost is that subsystems can exist
before anything drives them; the client delta parser and the networking
layers are tested but not yet part of the browser loop, so tests rather
than play carry their verification until Phase 7 and 8.
