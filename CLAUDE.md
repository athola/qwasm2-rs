# qwasm2-rs — Quake 2 Engine Rewrite in Rust/WASM

## Reference Implementation

The original C Quake 2 codebase is at [GMH-Code/Qwasm2](https://github.com/GMH-Code/Qwasm2)
(local clone: `~/Qwasm2`). **Always cross-reference game design decisions**
(physics constants, collision behavior, network protocol, entity system, player
movement) against the C source before implementing.

Key C source directories:
- `~/Qwasm2/src/common/` — collision, pmove, cvar, cmd, filesystem, net_msg
- `~/Qwasm2/src/game/` — entity system, spawning, AI, items, weapons
- `~/Qwasm2/src/server/` — server frame loop, world, client management
- `~/Qwasm2/src/client/` — client prediction, view, input, parsing
- `~/Qwasm2/src/backends/` — renderer, sound, platform

## Architecture

13-crate Cargo workspace with strict DAG dependency graph. See `docs/adr/001-crate-decomposition-and-trait-boundaries.md`.
See `docs/superpowers/plans/2026-03-26-c-to-rust-conversion.md` for phase progress and next-session roadmap.

```
q2-shared (types) → q2-common (services) → q2-game/server/client/render → q2-wasm/bin
```

### Key boundaries
- **Renderer**: `q2-render-api::Renderer` trait. Backends implement it.
- **Game logic**: `q2-game::traits::GameImport/GameExport` traits replace C function-pointer tables.
- **Player movement**: `q2-common::player_ctrl::PlayerController` — platform-agnostic physics.
- **Platform**: `q2-platform` — WASM-specific code gated on `cfg(target_arch = "wasm32")`.

### q2-game patterns
- **Entity callbacks are `fn` pointers** (not closures) — keeps entities Send+Sync and serializable for save/load.
- **Callback borrow pattern**: Read `Option<fn>` into a local variable before calling, since callbacks take `&mut GameWorld`.
- **GameWorld is the central state** — all game code receives `&mut GameWorld`. Replaces C globals (`g_edicts[]`, `level`, `game`).
- **MockGameImport** (`world::test_world()`) enables unit testing game logic without a real server.
- **Spawn functions** use `SpawnFn = fn(&mut EntityStorage, EntityKey, &HashMap<String, String>)` signature.
- Monster/func callback functions should be `pub` to avoid dead_code warnings (they're used as fn-pointer values).

### Rules
- No circular dependencies between crates.
- Unsafe code only at FFI boundaries (q2-render). All unsafe blocks must have `// SAFETY:` comments.
- Errors: use `Q2Error` (q2-common) internally. At WASM boundary, use `q2err_to_js()`.
- Entity storage uses SlotMap with generational indices — never raw pointers/indices.
- Physics constants must match the C original exactly (numerical precision matters for client/server prediction sync).

## Build & Test

```bash
cargo test --workspace          # Run all tests
cargo clippy --workspace        # Lint (zero errors required)
cargo check -p q2-game --target wasm32-unknown-unknown  # Verify WASM compat
make wasm                       # Build WASM target
make serve                      # Serve wasm-pack output via python3 (simple, no gamedata)
```

## Conventions
- Commit messages: conventional commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`)
- No global mutable state. All state is instance-owned or passed by reference.
- Large algorithmic modules (collision, pmove) are faithful C ports — modify with care and tests.
- `#[allow(clippy::too_many_arguments)]` on weapon fire functions matching C signatures.
- `#[allow(clippy::field_reassign_with_default)]` on spawn functions building MoveInfo incrementally.
- Use `.to_string()` not `.into()` when passing string literals to `SpawnTable::insert()`.
