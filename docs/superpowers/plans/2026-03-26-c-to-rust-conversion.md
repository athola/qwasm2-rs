# Qwasm2: C-to-Rust WASM Conversion Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## Progress (as of 2026-04-10)

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 0: Scaffolding | **DONE** | Workspace, 13 crates, CI, clippy, WASM entry point, bundler |
| Phase 1: Common Layer | **DONE** | All 11 tasks complete (276 tests) |
| Phase 2: Game Logic | **DONE** (core) | 41 files, 11,507 lines, 208 tests, CP-2 passes. Branch: `game-logic-0.1.1`. Gaps: #14-17 |
| Phase 3: Server | TODO | Skeleton exists. Issue #18 |
| Phase 4: Client | TODO | Skeleton exists. Issue #19 |
| Phase 5: Renderer | TODO | Skeleton exists. Issue #20 |
| Phase 6: Platform/WASM | TODO | Partial exists. Issue #21 |
| Phase 7: P2P Networking | TODO | q2-net stub. Issue #22 |
| Phase 8: Integration | TODO | Issue #23 |

**Additional completed work not in original plan:** `q2-wasm` (WASM entry point with self-test, WebGL2 check), `q2-bundler` (single-file HTML bundler with base64-inlined WASM), Playwright browser tests, Makefile, `q2-render-api` (Renderer trait).

**Phase 2 gaps (tracked, not blocking):**
- #14: Monster attack/pain animation expansion (all 20 types have stand/die; need full state machines)
- #15: Save/load with serde + callback registry
- #16: Player weapon state machine + DM rules
- #17: Full SV_Push with entity displacement rollback

**Next session priority — pick one branch to start:**

### Option A: Phase 5 Renderer (critical path to visual output)
```
Branch: renderer-0.1.1
Issue: #20
Work: BSP/MD2/SP2 loaders, GL3 shaders, world/mesh rendering, textures, lightmaps
Checkpoints: CP-4a → CP-4b → CP-4c → CP-5 (first frame in browser)
Est: ~20,000 Rust lines
```

### Option B: Phase 3 Server (bottom-up, enables real game loop)
```
Branch: server-0.1.1
Issue: #18
Work: Server state, GameImport impl, frame loop, client handling, entity linking
Checkpoints: CP-3 (server runs 100 frames)
Est: ~5,000 Rust lines
Unlocks: real collision traces for game logic (replaces MockGameImport)
```

### Option C: Phase 2 gaps (polish before moving on)
```
Branch: game-logic-polish-0.1.1
Issues: #14, #15, #16, #17
Work: Monster animations, save/load, weapon state, SV_Push rollback
No new checkpoints — improves existing CP-2
```

**Recommended order:** Option B (Server) → Option A (Renderer) → Phase 4 (Client) → Phase 6 (WASM). Server is smaller and provides real GameImport for testing game logic with actual collision.

**Critical path to CP-5b (playable in browser):**
Phase 3 (#18) → Phase 5 (#20) → Phase 4 (#19) → Phase 6 (#21)

**C source reference:** `~/Qwasm2/src/` (Yamagi Quake II with Emscripten WASM port)

**Goal:** Convert the entire Qwasm2 (Yamagi Quake II WASM port) from C to idiomatic Rust, replacing Emscripten with wasm-bindgen/wasm-pack and adding P2P multiplayer via Matchbox.

**Architecture:** Clean-room Rust rewrite from foundations up (common → game → server → client → renderers → WASM backend → networking). For numerically sensitive modules (collision, pmove) only, c2rust transpilation is used as a reference to ensure bit-exact behavior, then refactored to idiomatic Rust. Each phase produces a testable, runnable build. All unsafe code is annotated with rewrite priority markers.

**Tech Stack:**
- Rust (edition 2021), `wasm-bindgen`, `wasm-pack`, `web-sys`, `js-sys`
- `glow` (OpenGL ES 3.0 bindings), `sdl2` crate (native), `winit` (future)
- `matchbox_socket` (P2P WebRTC), `matchbox_signaling` (signaling server)
- `slotmap` (entity storage), `bitflags` (flag enums), `glam` (math)
- `serde` + `bincode` (save/load serialization), `tracing` (logging)
- `cpal` (cross-platform audio incl. WASM via Web Audio), `lewton` (OGG Vorbis decoder)
- `axum` (Rust dev server for local WASM serving + signaling)
- `c2rust` (reference transpilation for collision.c and pmove.c only)

**Entire toolchain is Rust.** No Python, Node, or other runtimes required. The dev server, signaling server, and all build tooling are Rust binaries.

---

## Playability Checkpoint Strategy

Every phase ends with a **playability checkpoint** — a browser-testable build that proves the game still works. Regressions are caught at the phase boundary, not months later.

### Checkpoint Protocol

At the end of each phase (and at major task boundaries within phases):

1. **Build WASM**: `wasm-pack build --target web --release`
2. **Serve locally**: `cargo run -p q2-devserver` (Rust-based dev server using `axum`, serves `web/` with correct MIME types for WASM)
3. **Run checkpoint test suite** (manual + automated):

| Checkpoint | Test | Pass Criteria |
|------------|------|---------------|
| **CP-0** (Phase 0) | Scaffold compiles to WASM | `cargo check --target wasm32-unknown-unknown` passes |
| **CP-1** (Phase 1) | Types serialize/deserialize, collision traces work | Unit tests pass on both native and WASM (`wasm-pack test --headless --chrome`) |
| **CP-2** (Phase 2) | Game logic: spawn entities, damage applies, monsters have state | Unit tests + spawn `info_player_start` + `monster_soldier`, soldier enters `stand` state and runs 1 AI frame without panic |
| **CP-3** (Phase 3) | Server starts, accepts simulated client connection | Server frame loop runs 100 frames without panic |
| **CP-4** (Phase 4) | Client connects to local server, receives game state | Client parses `svc_serverdata` + `svc_configstring` messages correctly |
| **CP-4a** | BSP model loads, surfaces extracted | BSP loader returns valid surface/node data, no rendering needed |
| **CP-4b** | GL smoke test — single triangle renders | `glow` context created, test shader compiles, triangle visible on screen |
| **CP-4c** | World geometry renders flat-shaded | BSP world renders without textures/lighting (wireframe or flat color) |
| **CP-5** (Phase 5) | **First textured frame in browser** — render a loaded map | Open browser, see a rendered BSP level with textures and lightmaps |
| **CP-5b** | **Playable in browser** — move around, look around | WASD + mouselook works, world renders, HUD shows |
| **CP-5c** | **Sound plays in browser** | Weapon fire, ambient sounds, and OGG music play via Web Audio |
| **CP-6** (Phase 6) | Full WASM integration — saves persist, files export | Save game → refresh browser → load game succeeds |
| **CP-7** (Phase 7) | **Two browser tabs play together** | Host in tab A, join in tab B, both see each other move |
| **CP-8** (Phase 8) | Full game playable — single-player campaign loads | Load `base1` (first SP map), walk around, shoot enemies, sound works |

### Checkpoint Tagging

Each checkpoint gets a git tag:

```bash
git tag -a cp-N-description -m "Checkpoint N: <what works>"
# Examples:
git tag -a cp-0-scaffold -m "Checkpoint 0: Rust workspace compiles to WASM"
git tag -a cp-5-first-frame -m "Checkpoint 5: First rendered frame in browser"
git tag -a cp-7-multiplayer -m "Checkpoint 7: Two browser tabs connected via Matchbox"
```

### Regression Detection

If a checkpoint fails after passing previously:

1. `git bisect` between last passing checkpoint tag and current HEAD
2. The failing checkpoint test identifies which subsystem broke
3. Fix before proceeding to next phase

### Browser Test Harness

Create `qwasm2-rs/tests/browser/` with Playwright tests for visual checkpoints:

```javascript
// tests/browser/checkpoint.spec.js
test('CP-5: first frame renders', async ({ page }) => {
  await page.goto('http://localhost:8080');
  await page.waitForSelector('canvas');
  // Wait for WebGL context
  const hasGL = await page.evaluate(() => {
    const canvas = document.querySelector('canvas');
    return !!canvas.getContext('webgl2');
  });
  expect(hasGL).toBe(true);
  // Screenshot comparison for visual regression
  await expect(page).toHaveScreenshot('cp5-first-frame.png', { threshold: 0.1 });
});

test('CP-7: multiplayer connection', async ({ browser }) => {
  const host = await browser.newPage();
  const client = await browser.newPage();
  await host.goto('http://localhost:8080?host=true');
  await client.goto('http://localhost:8080?join=true');
  // Wait for connection established indicator
  await host.waitForSelector('#peer-connected');
  await client.waitForSelector('#peer-connected');
});
```

### WASM Test Runner

For unit tests that need to run in a browser environment:

```toml
# In each crate's Cargo.toml that needs browser testing
[dev-dependencies]
wasm-bindgen-test = "0.3"
```

```rust
#[cfg(target_arch = "wasm32")]
mod wasm_tests {
    use wasm_bindgen_test::*;
    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn webgl_context_creation() {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();
        let canvas = document.create_element("canvas").unwrap();
        let gl = canvas.dyn_ref::<web_sys::HtmlCanvasElement>()
            .unwrap()
            .get_context("webgl2")
            .unwrap();
        assert!(gl.is_some());
    }
}
```

Run with: `wasm-pack test --headless --chrome -p q2-platform`

---

## Unsafe Annotation Convention

Every `unsafe` block retained from c2rust or written manually MUST carry one of these markers:

```rust
// SAFETY(c2rust): Mechanical transpilation. Rewrite priority: HIGH/MEDIUM/LOW
//   Reason: <why this is unsafe — e.g., raw pointer deref, global mutable state>
//   Rewrite: <what the safe Rust replacement would look like>
unsafe { ... }

// SAFETY(ffi): Required for FFI boundary (WebGL, SDL, browser APIs).
//   Cannot be made safe without removing the FFI call.
unsafe { ... }

// SAFETY(perf): Intentional unsafe for performance. Benchmarked alternative is N% slower.
//   Safe alternative: <description>
unsafe { ... }

// SAFETY(inherent): Genuinely unsafe operation (raw memory, pointer arithmetic).
//   This cannot be expressed in safe Rust. Invariants maintained by: <description>
unsafe { ... }
```

Every file that contains `unsafe` blocks gets a header comment:

```rust
//! # Unsafe Audit Status
//! - Total unsafe blocks: N
//! - c2rust mechanical: N (rewrite targets)
//! - FFI boundary: N (permanent)
//! - Performance: N (benchmarked)
//! - Inherent: N (documented invariants)
```

---

## File Structure

### Workspace Layout

```
qwasm2-rs/                          # New Rust workspace root (sibling to src/)
├── Cargo.toml                      # Workspace manifest
├── crates/
│   ├── q2-shared/                  # Shared types, constants, math (from shared.h)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── types.rs            # qboolean, vec3_t, entity_state_t, player_state_t
│   │       ├── constants.rs        # MAX_EDICTS, MAX_CLIENTS, protocol flags
│   │       ├── math.rs             # vec3 ops, angle conversion, bytedirs
│   │       └── protocol.rs         # svc_ops, clc_ops, update flags, message encoding
│   │
│   ├── q2-common/                  # Engine common layer (from src/common/)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs            # Q2Error enum replacing setjmp/longjmp + Com_Error
│   │       ├── cvar.rs             # CVar system (HashMap + ordered iteration)
│   │       ├── cmd.rs              # Command buffer + parser
│   │       ├── filesystem.rs       # Virtual filesystem (PAK, directories)
│   │       ├── net_msg.rs          # Network message read/write (sizebuf_t replacement)
│   │       ├── netchan.rs          # Reliable/unreliable channel protocol
│   │       ├── collision.rs        # BSP collision, CM_* functions
│   │       ├── pmove.rs            # Player movement prediction
│   │       ├── zone.rs             # Tag-based allocator (Rust wrapper over Vec pools)
│   │       └── crc.rs              # CRC/MD4 checksums
│   │
│   ├── q2-game/                    # Game logic DLL equivalent (from src/game/)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── traits.rs           # GameImport/GameExport traits (replacing fn ptr tables)
│   │       ├── entity.rs           # Entity system (SlotMap-based, replacing edict_t arrays)
│   │       ├── spawn.rs            # Entity spawning from BSP entity strings
│   │       ├── ai.rs               # Monster AI state machines
│   │       ├── combat.rs           # Damage, knockback, kill tracking
│   │       ├── weapons.rs          # Weapon firing, projectiles
│   │       ├── items.rs            # Item definitions, pickup/use/drop
│   │       ├── physics.rs          # Game-level physics (g_phys.c)
│   │       ├── triggers.rs         # Trigger entities
│   │       ├── targets.rs          # Target entities
│   │       ├── func.rs             # Functional entities (doors, platforms)
│   │       ├── turret.rs           # Turret entities
│   │       ├── chase.rs            # Chase camera
│   │       ├── monster/            # Monster implementations
│   │       │   ├── mod.rs
│   │       │   ├── soldier.rs      # soldier variants
│   │       │   ├── tank.rs         # tank + supertank
│   │       │   ├── gladiator.rs
│   │       │   ├── gunner.rs
│   │       │   ├── infantry.rs
│   │       │   ├── brain.rs
│   │       │   ├── chick.rs
│   │       │   ├── flyer.rs
│   │       │   ├── hover.rs
│   │       │   ├── medic.rs
│   │       │   ├── parasite.rs
│   │       │   ├── mutant.rs
│   │       │   ├── insane.rs
│   │       │   ├── flipper.rs
│   │       │   ├── float.rs
│   │       │   ├── berserk.rs
│   │       │   ├── boss2.rs
│   │       │   ├── boss3.rs
│   │       │   └── misc.rs         # Move utilities
│   │       └── savegame/
│   │           ├── mod.rs
│   │           ├── serialize.rs    # serde-based save/load (replacing fn ptr tables)
│   │           └── fields.rs       # Field metadata for backward compat
│   │
│   ├── q2-server/                  # Dedicated server (from src/server/)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs            # Server/ServerStatic (replacing global sv/svs)
│   │       ├── init.rs             # Server initialization, map loading
│   │       ├── game_iface.rs       # GameImport impl for server → game callbacks
│   │       ├── client_handler.rs   # Per-client connection management
│   │       ├── send.rs             # Message sending, multicast, broadcast
│   │       ├── recv.rs             # Client message processing
│   │       ├── world.rs            # Spatial partitioning, entity linking
│   │       ├── frame.rs            # Server frame building
│   │       └── savegame.rs         # Server-side save/load
│   │
│   ├── q2-client/                  # Client (from src/client/)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs            # ClientState/ClientStatic (replacing global cl/cls)
│   │       ├── parse.rs            # Server message parsing
│   │       ├── entities.rs         # Client-side entity interpolation
│   │       ├── prediction.rs       # Client-side movement prediction
│   │       ├── effects.rs          # Particles, dynamic lights, temp entities
│   │       ├── view.rs             # View rendering setup
│   │       ├── input.rs            # Input handling (keyboard, mouse, gamepad)
│   │       ├── screen.rs           # HUD, loading screen, console overlay
│   │       ├── console.rs          # In-game console
│   │       ├── menu/               # Menu system
│   │       │   ├── mod.rs
│   │       │   ├── main_menu.rs
│   │       │   ├── options.rs
│   │       │   └── multiplayer.rs
│   │       ├── cin.rs              # ROQ cinematic playback
│   │       └── sound/              # Sound system
│   │           ├── mod.rs          # Sound trait + platform dispatch
│   │           ├── mixer.rs        # Software audio mixer (shared logic)
│   │           ├── ogg.rs          # OGG Vorbis decoding via lewton crate
│   │           ├── wav.rs          # WAV file loading
│   │           ├── wasm.rs         # Web Audio API backend (cpal/web-sys)
│   │           └── native.rs       # SDL2/cpal native backend
│   │
│   ├── q2-render/                  # Renderer abstraction + GL3 impl
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs              # RefExport/RefImport traits
│   │       ├── gl3/                # OpenGL ES 3.0 / WebGL 2.0 renderer
│   │       │   ├── mod.rs
│   │       │   ├── main.rs         # Renderer init, frame, shutdown
│   │       │   ├── draw.rs         # 2D drawing (HUD, console)
│   │       │   ├── mesh.rs         # Alias model (MD2) rendering
│   │       │   ├── world.rs        # BSP world rendering
│   │       │   ├── light.rs        # Lightmaps, dynamic lights
│   │       │   ├── warp.rs         # Sky, water surfaces
│   │       │   ├── image.rs        # Texture loading (PCX, WAL, TGA)
│   │       │   ├── model.rs        # Model loading (BSP, MD2, SP2)
│   │       │   └── shaders.rs      # GLSL shader sources + compilation
│   │       └── files/              # Shared image/model file parsers
│   │           ├── mod.rs
│   │           ├── pcx.rs
│   │           ├── wal.rs
│   │           ├── tga.rs
│   │           └── stb_image.rs    # stb_image wrapper or pure-Rust replacement
│   │
│   ├── q2-platform/                # Platform abstraction
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── wasm/               # WASM/browser backend
│   │       │   ├── mod.rs
│   │       │   ├── main.rs         # Entry point, requestAnimationFrame loop
│   │       │   ├── input.rs        # Pointer lock, keyboard, gamepad via web-sys
│   │       │   ├── storage.rs      # IndexedDB filesystem (replacing initfs/syncfs)
│   │       │   ├── export.rs       # File download to browser
│   │       │   └── gl_context.rs   # WebGL2 context creation
│   │       └── native/             # Desktop backend (SDL2)
│   │           ├── mod.rs
│   │           ├── main.rs
│   │           ├── input.rs
│   │           └── gl_context.rs
│   │
│   ├── q2-net/                     # Networking (Matchbox P2P)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── transport.rs        # Matchbox socket wrapper
│   │       ├── channels.rs         # Unreliable (game state) + reliable (commands)
│   │       ├── lobby.rs            # Matchmaking / room management
│   │       ├── protocol.rs         # Q2 network protocol over Matchbox
│   │       └── signaling.rs        # Embedded signaling server (host mode)
│   │
│   ├── q2-render-api/              # Renderer trait crate (no implementation)
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs              # Renderer trait, RefDef, ModelHandle, ImageHandle
│   │
│   ├── q2-bin/                     # Binary entry points
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs             # Native entry point
│   │       └── wasm.rs             # WASM entry point (#[wasm_bindgen])
│   │
│   └── q2-devserver/               # Rust dev server (replaces python http.server)
│       ├── Cargo.toml              # deps: axum, tower-http, matchbox_signaling
│       └── src/
│           └── main.rs             # Serves web/ with WASM MIME types + embedded signaling
│
├── tests/                          # Integration tests
│   ├── common_tests.rs
│   ├── game_tests.rs
│   ├── protocol_tests.rs
│   └── net_tests.rs
│
├── web/                            # Web assets (replacing wasm/shell.html)
│   ├── index.html
│   ├── style.css
│   └── bootstrap.js                # WASM loader, canvas setup
│
└── .cargo/
    └── config.toml                 # WASM target config, clippy settings
```

### C → Rust File Mapping Reference

Each Rust crate maps to C source directories. During conversion, this table tracks which C files have been fully replaced:

| C Source | Rust Crate | Status |
|----------|-----------|--------|
| `src/common/header/shared.h` | `q2-shared` | Phase 1 |
| `src/common/header/common.h` | `q2-shared` + `q2-common` | Phase 1 |
| `src/common/zone.c` | `q2-common::zone` | Phase 1 |
| `src/common/cvar.c` | `q2-common::cvar` | Phase 1 |
| `src/common/cmdparser.c` | `q2-common::cmd` | Phase 1 |
| `src/common/cbuf.c` | `q2-common::cmd` | Phase 1 |
| `src/common/filesystem.c` | `q2-common::filesystem` | Phase 1 |
| `src/common/szone.c` | `q2-common::net_msg` | Phase 1 |
| `src/common/movemsg.c` | `q2-common::net_msg` | Phase 1 |
| `src/common/netchan.c` | `q2-common::netchan` | Phase 1 |
| `src/common/collision.c` | `q2-common::collision` | Phase 1 |
| `src/common/pmove.c` | `q2-common::pmove` | Phase 1 |
| `src/common/frame.c` | `q2-common` + `q2-bin` | Phase 1 |
| `src/common/crc.c`, `md4.c` | `q2-common::crc` | Phase 1 |
| `src/common/argproc.c` | `q2-common` (merged into lib) | Phase 1 |
| `src/common/clientserver.c` | `q2-common::error` | Phase 1 |
| `src/game/header/game.h` | `q2-game::traits` | Phase 2 |
| `src/game/header/local.h` | `q2-game::entity` + types | Phase 2 |
| `src/game/g_main.c` | `q2-game::lib` | Phase 2 |
| `src/game/g_spawn.c` | `q2-game::spawn` | Phase 2 |
| `src/game/g_ai.c` | `q2-game::ai` | Phase 2 |
| `src/game/g_combat.c` | `q2-game::combat` | Phase 2 |
| `src/game/g_weapon.c` | `q2-game::weapons` | Phase 2 |
| `src/game/g_items.c` | `q2-game::items` | Phase 2 |
| `src/game/g_phys.c` | `q2-game::physics` | Phase 2 |
| `src/game/g_trigger.c` | `q2-game::triggers` | Phase 2 |
| `src/game/g_target.c` | `q2-game::targets` | Phase 2 |
| `src/game/g_func.c` | `q2-game::func` | Phase 2 |
| `src/game/g_turret.c` | `q2-game::turret` | Phase 2 |
| `src/game/g_chase.c` | `q2-game::chase` | Phase 2 |
| `src/game/g_monster.c` | `q2-game::monster` | Phase 2 |
| `src/game/g_misc.c` | `q2-game` (misc entities) | Phase 2 |
| `src/game/g_svcmds.c` | `q2-game` (server commands) | Phase 2 |
| `src/game/g_utils.c` | `q2-game` (utility functions) | Phase 2 |
| `src/game/monster/*` (22 types) | `q2-game::monster::*` | Phase 2 |
| `src/game/player/*` | `q2-game` (player logic) | Phase 2 |
| `src/game/savegame/*` | `q2-game::savegame` | Phase 2 |
| `src/server/sv_*.c` (10 files) | `q2-server::*` | Phase 3 |
| `src/client/cl_*.c` (17 files) | `q2-client::*` | Phase 4 |
| `src/client/input/*` | `q2-client::input` | Phase 4 |
| `src/client/menu/*` | `q2-client::menu` | Phase 4 |
| `src/client/sound/*` | `q2-client::sound` | Phase 4 |
| `src/client/vid/*` | `q2-client` + `q2-platform` | Phase 4 |
| `src/client/refresh/gl3/*` | `q2-render::gl3` | Phase 5 |
| `src/client/refresh/files/*` | `q2-render::files` | Phase 5 |
| `src/client/refresh/gl1/*` | Deferred (GL1 optional) | Phase 5+ |
| `src/client/refresh/soft/*` | Deferred (soft optional) | Phase 5+ |
| `src/backends/wasm/*` | `q2-platform::wasm` | Phase 6 |
| `src/backends/unix/*` | `q2-platform::native` | Phase 6 |
| `src/backends/windows/*` | `q2-platform::native` (cfg) | Phase 6 |
| N/A (new) | `q2-net` | Phase 7 |

---

## Phase 0: Project Scaffolding

### Task 0.1: Create Rust Workspace

**Files:**
- Create: `qwasm2-rs/Cargo.toml`
- Create: `qwasm2-rs/crates/q2-shared/Cargo.toml`
- Create: `qwasm2-rs/crates/q2-shared/src/lib.rs`
- Create: `qwasm2-rs/rust-toolchain.toml`
- Create: `qwasm2-rs/.cargo/config.toml`

- [x] **Step 1: Write a failing test for the shared types**

```rust
// qwasm2-rs/crates/q2-shared/src/lib.rs
#[cfg(test)]
mod tests {
    #[test]
    fn vec3_add() {
        // This will fail until we define Vec3
        let a = super::Vec3::new(1.0, 2.0, 3.0);
        let b = super::Vec3::new(4.0, 5.0, 6.0);
        let c = a + b;
        assert_eq!(c, super::Vec3::new(5.0, 7.0, 9.0));
    }
}
```

- [x] **Step 2: Create the workspace Cargo.toml**

```toml
# qwasm2-rs/Cargo.toml
[workspace]
resolver = "2"
members = [
    "crates/q2-shared",
    "crates/q2-common",
    "crates/q2-game",
    "crates/q2-server",
    "crates/q2-client",
    "crates/q2-render-api",
    "crates/q2-render",
    "crates/q2-platform",
    "crates/q2-net",
    "crates/q2-bin",
    "crates/q2-devserver",
]

[workspace.dependencies]
glam = "0.29"
bitflags = "2"
slotmap = "1"
serde = { version = "1", features = ["derive"] }
bincode = "1"
tracing = "0.1"
tracing-subscriber = "0.3"
thiserror = "2"
anyhow = "1"
glow = "0.16"
matchbox_socket = "0.14"
wasm-bindgen = "0.2"
web-sys = "0.3"
js-sys = "0.3"
cpal = "0.15"          # Cross-platform audio (native + WASM via Web Audio)
lewton = "0.10"        # Pure-Rust OGG Vorbis decoder
axum = "0.8"           # Dev server + signaling
tower-http = { version = "0.6", features = ["fs", "cors"] }  # Static file serving

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "warn"

[workspace.lints.clippy]
undocumented_unsafe_blocks = "deny"
```

```toml
# qwasm2-rs/rust-toolchain.toml
[toolchain]
channel = "stable"
targets = ["wasm32-unknown-unknown"]
```

```toml
# qwasm2-rs/.cargo/config.toml
[target.wasm32-unknown-unknown]
runner = "wasm-server-runner"

[build]
rustflags = ["-D", "clippy::undocumented_unsafe_blocks"]
```

- [x] **Step 3: Create q2-shared crate skeleton**

```toml
# qwasm2-rs/crates/q2-shared/Cargo.toml
[package]
name = "q2-shared"
version = "0.1.0"
edition = "2021"

[dependencies]
glam = { workspace = true }
bitflags = { workspace = true }
serde = { workspace = true }

[lints]
workspace = true
```

- [x] **Step 4: Run test to verify it fails**

Run: `cd qwasm2-rs && cargo test -p q2-shared`
Expected: FAIL — `Vec3` not defined

- [x] **Step 5: Commit scaffold**

```bash
git add qwasm2-rs/
git commit -m "feat: initialize Rust workspace with q2-shared crate skeleton"
```

---

### Task 0.2: Create Remaining Crate Skeletons

**Files:**
- Create: `qwasm2-rs/crates/q2-common/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-game/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-server/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-client/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-render/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-platform/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-net/Cargo.toml` + `src/lib.rs`
- Create: `qwasm2-rs/crates/q2-bin/Cargo.toml` + `src/main.rs`

- [x] **Step 1: Create all crate Cargo.toml files with correct inter-crate dependencies**

Key dependency graph (each crate depends on those above it):
```
q2-shared          (no deps)
q2-common          → q2-shared
q2-game            → q2-shared, q2-common
q2-server          → q2-shared, q2-common, q2-game
q2-client          → q2-shared, q2-common          (NOT q2-game — communicates only via server)
q2-render-api      → q2-shared                      (trait-only crate for Renderer trait)
q2-render          → q2-shared, q2-common, q2-render-api
q2-platform        → q2-shared, q2-common, q2-render-api  (depends on trait, not impl)
q2-net             → q2-shared, q2-common
q2-devserver       → axum, tower-http               (dev WASM file server + signaling)
q2-bin             → all of the above
```

Note: `q2-client` does NOT depend on `q2-game`. The client communicates with the game
exclusively through the server via network messages. Shared types live in `q2-shared`.
`q2-render-api` is a thin crate containing only the `Renderer` trait and related types,
preventing a circular dependency between `q2-platform` and `q2-render`.

Example for q2-common:
```toml
[package]
name = "q2-common"
version = "0.1.0"
edition = "2021"

[dependencies]
q2-shared = { path = "../q2-shared" }
thiserror = { workspace = true }
tracing = { workspace = true }

[lints]
workspace = true
```

- [x] **Step 2: Add placeholder lib.rs for each crate**

Each `lib.rs` starts with:
```rust
//! Qwasm2-rs: [crate description]
//!
//! # Unsafe Audit Status
//! - Total unsafe blocks: 0
//! - c2rust mechanical: 0
//! - FFI boundary: 0
//! - Performance: 0
//! - Inherent: 0
```

- [x] **Step 3: Verify full workspace compiles**

Run: `cd qwasm2-rs && cargo check --workspace`
Expected: PASS — all crates compile (empty)

- [x] **Step 4: Verify WASM target compiles**

Run: `cd qwasm2-rs && cargo check --workspace --target wasm32-unknown-unknown`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add qwasm2-rs/
git commit -m "feat: add all workspace crate skeletons with dependency graph"
```

---

### Task 0.3: Set Up CI and Linting

**Files:**
- Create: `qwasm2-rs/.github/workflows/ci.yml`
- Create: `qwasm2-rs/clippy.toml`

- [x] **Step 1: Create CI workflow**

```yaml
name: CI
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: wasm32-unknown-unknown
          components: clippy
      - run: cargo check --workspace
      - run: cargo check --workspace --target wasm32-unknown-unknown
      - run: cargo clippy --workspace -- -D warnings
      - run: cargo test --workspace
```

- [x] **Step 2: Create clippy.toml**

```toml
# Enforce unsafe documentation
undocumented-unsafe-blocks = true
```

- [x] **Step 3: Verify locally**

Run: `cd qwasm2-rs && cargo clippy --workspace -- -D warnings`
Expected: PASS — no warnings on empty crates

- [x] **Step 4: Commit**

```bash
git add qwasm2-rs/.github/ qwasm2-rs/clippy.toml
git commit -m "ci: add GitHub Actions workflow with clippy and WASM target checks"
```

---

## Phase 1: Shared Types and Common Layer

This is the foundation. Everything else depends on it. We rewrite this from scratch (not c2rust) because these types define the Rust-idiomatic API surface that all other crates consume.

### Task 1.1: Core Shared Types (`q2-shared`)

**Files:**
- Create: `qwasm2-rs/crates/q2-shared/src/types.rs`
- Create: `qwasm2-rs/crates/q2-shared/src/constants.rs`
- Modify: `qwasm2-rs/crates/q2-shared/src/lib.rs`
- Reference: `src/common/header/shared.h:49-1000`

- [x] **Step 1: Write failing tests for core types**

```rust
// types.rs tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_state_default() {
        let es = EntityState::default();
        assert_eq!(es.number, 0);
        assert_eq!(es.origin, Vec3::ZERO);
    }

    #[test]
    fn player_state_has_pmove() {
        let ps = PlayerState::default();
        assert_eq!(ps.pmove.pm_type, PmType::Normal);
    }

    #[test]
    fn trace_result() {
        let t = Trace::default();
        assert!(!t.allsolid);
        assert_eq!(t.fraction, 1.0);
    }

    #[test]
    fn usercmd_serialization() {
        let cmd = UserCmd { msec: 16, buttons: 1, angles: [0, 90, 0], ..Default::default() };
        assert_eq!(cmd.msec, 16);
    }
}
```

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p q2-shared`
Expected: FAIL — types not defined

- [x] **Step 3: Implement core types**

```rust
// qwasm2-rs/crates/q2-shared/src/types.rs
use glam::Vec3;
use serde::{Deserialize, Serialize};

// Re-export glam::Vec3 as our vec3_t replacement
pub type Vec3f = Vec3;

/// Replaces qboolean. Use native bool throughout; this alias exists only
/// for documentation when reading the C source alongside.
pub type QBool = bool;

/// Entity state — communicated across the network.
/// Replaces entity_state_t from shared.h:107-131
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EntityState {
    pub number: i32,
    pub origin: Vec3f,
    pub angles: Vec3f,
    pub old_origin: Vec3f,  // for lerping
    pub modelindex: i32,
    pub modelindex2: i32,
    pub modelindex3: i32,
    pub modelindex4: i32,
    pub frame: i32,
    pub skinnum: i32,
    pub effects: u32,
    pub renderfx: i32,
    pub solid: i32,
    pub sound: i32,
    pub event: i32,
}

/// Player movement type
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PmType {
    #[default]
    Normal,
    Spectator,
    Dead,
    Gib,
    Freeze,
}

/// Player movement state — shared between client prediction and server.
/// Replaces pmove_state_t from shared.h:133-149
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PmoveState {
    pub pm_type: PmType,
    pub origin: [i16; 3],     // 12.3 fixed point
    pub velocity: [i16; 3],   // 12.3 fixed point
    pub pm_flags: u8,
    pub pm_time: u8,
    pub gravity: i16,
    pub delta_angles: [i16; 3],
}

/// Player state — communicated from server to clients.
/// Replaces player_state_t from shared.h:152-170
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerState {
    pub pmove: PmoveState,
    pub viewangles: Vec3f,
    pub viewoffset: Vec3f,
    pub kick_angles: Vec3f,
    pub gunangles: Vec3f,
    pub gunoffset: Vec3f,
    pub gunindex: i32,
    pub gunframe: i32,
    pub blend: [f32; 4],
    pub fov: f32,
    pub rdflags: i32,
    pub stats: [i16; 32],  // MAX_STATS
}

/// User input command. Replaces usercmd_t from shared.h:173-183
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserCmd {
    pub msec: u8,
    pub buttons: u8,
    pub angles: [i16; 3],
    pub forwardmove: i16,
    pub sidemove: i16,
    pub upmove: i16,
    pub impulse: u8,
    pub lightlevel: u8,
}

/// Trace result from collision detection. Replaces trace_t.
#[derive(Debug, Clone)]
pub struct Trace {
    pub allsolid: bool,
    pub startsolid: bool,
    pub fraction: f32,
    pub endpos: Vec3f,
    pub plane: Plane,
    pub surface: Option<Surface>,
    pub contents: i32,
    pub ent_index: Option<usize>,  // replaces edict_t* (index into entity storage)
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            allsolid: false,
            startsolid: false,
            fraction: 1.0,
            endpos: Vec3f::ZERO,
            plane: Plane::default(),
            surface: None,
            contents: 0,
            ent_index: None,
        }
    }
}

/// BSP plane. Replaces cplane_t.
#[derive(Debug, Clone, Default)]
pub struct Plane {
    pub normal: Vec3f,
    pub dist: f32,
    pub plane_type: u8,
    pub signbits: u8,
}

/// Surface info. Replaces csurface_t.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub name: String,
    pub flags: i32,
    pub value: i32,
}

/// Solid type for entities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Solid {
    #[default]
    Not,
    Trigger,
    Bbox,
    Bsp,
}
```

- [x] **Step 4: Run tests**

Run: `cargo test -p q2-shared`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add qwasm2-rs/crates/q2-shared/
git commit -m "feat(q2-shared): implement core types — EntityState, PlayerState, UserCmd, Trace"
```

---

### Task 1.2: Constants and Protocol Flags (`q2-shared`)

**Files:**
- Create: `qwasm2-rs/crates/q2-shared/src/constants.rs`
- Create: `qwasm2-rs/crates/q2-shared/src/protocol.rs`
- Reference: `src/common/header/shared.h:134-200`, `src/common/header/common.h:178-317`

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn max_edicts_matches_protocol() {
    assert_eq!(constants::MAX_EDICTS, 1024);
    assert_eq!(constants::PROTOCOL_VERSION, 34);
}

#[test]
fn update_flags_bitfield() {
    use protocol::UpdateFlags;
    let flags = UpdateFlags::ORIGIN1 | UpdateFlags::ORIGIN2;
    assert!(flags.contains(UpdateFlags::ORIGIN1));
    assert!(!flags.contains(UpdateFlags::ANGLE1));
}

#[test]
fn svc_ops_values() {
    assert_eq!(protocol::SvcOp::Bad as u8, 0);
    assert_eq!(protocol::SvcOp::Frame as u8, 20);
}
```

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p q2-shared`
Expected: FAIL

- [x] **Step 3: Implement constants and protocol types**

```rust
// constants.rs
pub const PROTOCOL_VERSION: i32 = 34;
pub const MAX_CLIENTS: usize = 256;
pub const MAX_EDICTS: usize = 1024;
pub const MAX_MODELS: usize = 256;
pub const MAX_SOUNDS: usize = 256;
pub const MAX_IMAGES: usize = 256;
pub const MAX_ITEMS: usize = 256;
pub const MAX_LIGHTSTYLES: usize = 256;
pub const MAX_CONFIGSTRINGS: usize = 2080; // calculated from Q2 source
pub const MAX_QPATH: usize = 64;
pub const MAX_MSGLEN: usize = 1400;
pub const MAX_STRING_CHARS: usize = 2048;
// ... all constants from shared.h and common.h
```

```rust
// protocol.rs
use bitflags::bitflags;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvcOp {
    Bad = 0,
    MuzzleFlash, MuzzleFlash2, TempEntity, Layout, Inventory,
    Nop, Disconnect, Reconnect, Sound, Print, StuffText,
    ServerData, ConfigString, SpawnBaseline, CenterPrint,
    Download, PlayerInfo, PacketEntities, DeltaPacketEntities, Frame,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClcOp {
    Bad = 0, Nop, Move, UserInfo, StringCmd,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UpdateFlags: u32 {
        const ORIGIN1    = 1 << 0;
        const ORIGIN2    = 1 << 1;
        const ANGLE2     = 1 << 2;
        const ANGLE3     = 1 << 3;
        // ... all U_* flags from common.h:284-316
        const FRAME8     = 1 << 4;
        const EVENT      = 1 << 5;
        const REMOVE     = 1 << 6;
        const MOREBITS1  = 1 << 7;
        const NUMBER16   = 1 << 8;
        const ORIGIN3    = 1 << 9;
        const ANGLE1     = 1 << 10;
        const MODEL      = 1 << 11;
        const RENDERFX8  = 1 << 12;
        const EFFECTS8   = 1 << 14;
        const MOREBITS2  = 1 << 15;
        const SKIN8      = 1 << 16;
        const FRAME16    = 1 << 17;
        const RENDERFX16 = 1 << 18;
        const EFFECTS16  = 1 << 19;
        const MODEL2     = 1 << 20;
        const MODEL3     = 1 << 21;
        const MODEL4     = 1 << 22;
        const MOREBITS3  = 1 << 23;
        const OLDORIGIN  = 1 << 24;
        const SKIN16     = 1 << 25;
        const SOUND      = 1 << 26;
        const SOLID      = 1 << 27;
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PlayerStateFlags: u16 {
        const M_TYPE       = 1 << 0;
        const M_ORIGIN     = 1 << 1;
        const M_VELOCITY   = 1 << 2;
        const M_TIME       = 1 << 3;
        const M_FLAGS      = 1 << 4;
        const M_GRAVITY    = 1 << 5;
        const M_DELTA_ANGLES = 1 << 6;
        const VIEWOFFSET   = 1 << 7;
        const VIEWANGLES   = 1 << 8;
        const KICKANGLES   = 1 << 9;
        const BLEND        = 1 << 10;
        const FOV          = 1 << 11;
        const WEAPONINDEX  = 1 << 12;
        const WEAPONFRAME  = 1 << 13;
        const RDFLAGS      = 1 << 14;
    }
}
```

- [x] **Step 4: Run tests**

Run: `cargo test -p q2-shared`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add qwasm2-rs/crates/q2-shared/
git commit -m "feat(q2-shared): add constants, protocol ops, and bitflag types"
```

---

### Task 1.3: Error Handling System (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/error.rs`
- Modify: `qwasm2-rs/crates/q2-common/src/lib.rs`
- Reference: `src/common/header/common.h:731-734`, `src/common/frame.c:46`, `src/common/clientserver.c:268-279`

This replaces the `setjmp`/`longjmp` + `Com_Error()` pattern that is the #1 conversion blocker.

- [x] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_drop_is_recoverable() {
        let err = Q2Error::Drop("connection lost".into());
        assert!(err.is_recoverable());
    }

    #[test]
    fn error_fatal_is_not_recoverable() {
        let err = Q2Error::Fatal("out of memory".into());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn frame_error_recovery() {
        // Simulates the main loop catching ERR_DROP
        let result: Result<(), Q2Error> = Err(Q2Error::Drop("test".into()));
        match result {
            Err(Q2Error::Drop(msg)) => assert_eq!(msg, "test"),
            _ => panic!("expected Drop error"),
        }
    }
}
```

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p q2-common`
Expected: FAIL

- [x] **Step 3: Implement error system**

```rust
// qwasm2-rs/crates/q2-common/src/error.rs
use thiserror::Error;

/// Replaces Com_Error() + ERR_FATAL/ERR_DROP/ERR_QUIT.
/// The frame loop catches Drop errors and continues.
/// Fatal errors terminate the process.
#[derive(Error, Debug)]
pub enum Q2Error {
    /// ERR_DROP: Print to console, disconnect, continue main loop.
    #[error("drop: {0}")]
    Drop(String),

    /// ERR_FATAL: Exit the entire game.
    #[error("fatal: {0}")]
    Fatal(String),

    /// ERR_QUIT: Clean shutdown requested.
    #[error("quit")]
    Quit,

    /// Generic I/O error wrapper.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Network error.
    #[error("net: {0}")]
    Net(String),
}

impl Q2Error {
    /// ERR_DROP is recoverable (skip frame, keep running).
    /// ERR_FATAL and ERR_QUIT are not.
    pub fn is_recoverable(&self) -> bool {
        matches!(self, Q2Error::Drop(_))
    }
}

pub type Q2Result<T> = Result<T, Q2Error>;

/// Replacement for Com_Error(ERR_DROP, ...) — returns Err(Drop).
/// Use: `return q2_drop!("connection lost: {}", reason);`
#[macro_export]
macro_rules! q2_drop {
    ($($arg:tt)*) => {
        return Err($crate::error::Q2Error::Drop(format!($($arg)*)))
    };
}

/// Replacement for Com_Error(ERR_FATAL, ...) — returns Err(Fatal).
#[macro_export]
macro_rules! q2_fatal {
    ($($arg:tt)*) => {
        return Err($crate::error::Q2Error::Fatal(format!($($arg)*)))
    };
}
```

- [x] **Step 4: Run tests**

Run: `cargo test -p q2-common`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add qwasm2-rs/crates/q2-common/
git commit -m "feat(q2-common): implement Q2Error replacing setjmp/longjmp error handling"
```

---

### Task 1.4: CVar System (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/cvar.rs`
- Reference: `src/common/cvar.c`, `src/common/header/common.h:434-513`

Replaces the linked-list `cvar_t` chain with a `HashMap`-based system.

- [x] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_creates_cvar() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("test_var", "42", CVarFlags::empty());
        assert_eq!(cvars.string(handle), "42");
        assert_eq!(cvars.value(handle), 42.0);
    }

    #[test]
    fn set_changes_value() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("test_var", "1", CVarFlags::empty());
        cvars.set("test_var", "2");
        assert_eq!(cvars.value(handle), 2.0);
    }

    #[test]
    fn noset_prevents_change() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("locked", "1", CVarFlags::NOSET);
        cvars.set("locked", "2");
        assert_eq!(cvars.value(handle), 1.0); // unchanged
    }

    #[test]
    fn latch_defers_change() {
        let mut cvars = CVarSystem::new();
        let handle = cvars.get("latched", "1", CVarFlags::LATCH);
        cvars.set("latched", "2");
        assert_eq!(cvars.value(handle), 1.0); // still old
        cvars.apply_latched();
        assert_eq!(cvars.value(handle), 2.0); // now updated
    }

    #[test]
    fn archive_flag_exports() {
        let mut cvars = CVarSystem::new();
        cvars.get("saved_var", "hello", CVarFlags::ARCHIVE);
        let output = cvars.write_archive();
        assert!(output.contains("set saved_var \"hello\""));
    }

    #[test]
    fn userinfo_modified_flag() {
        let mut cvars = CVarSystem::new();
        cvars.get("name", "player", CVarFlags::USERINFO);
        assert!(!cvars.userinfo_modified());
        cvars.set("name", "newname");
        assert!(cvars.userinfo_modified());
        cvars.clear_userinfo_modified();
        assert!(!cvars.userinfo_modified());
    }
}
```

- [x] **Step 2: Run tests to verify failure**

Run: `cargo test -p q2-common -- cvar`
Expected: FAIL

- [x] **Step 3: Implement CVar system**

Key design decisions vs. C original:
- `HashMap<String, CVar>` instead of linked list (O(1) lookup vs O(n))
- `CVarHandle` (newtype around index) replaces `cvar_t*` pointers
- `bitflags!` for CVAR_ARCHIVE, CVAR_USERINFO, etc.
- No global state — `CVarSystem` is an owned struct passed through the engine

```rust
// cvar.rs — abbreviated, full implementation in actual code
use bitflags::bitflags;
use std::collections::HashMap;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CVarFlags: u32 {
        const ARCHIVE    = 1 << 0;
        const USERINFO   = 1 << 1;
        const SERVERINFO = 1 << 2;
        const NOSET      = 1 << 3;
        const LATCH      = 1 << 4;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CVarHandle(usize);

#[derive(Debug)]
struct CVar {
    name: String,
    string: String,
    latched_string: Option<String>,
    value: f32,
    flags: CVarFlags,
}

#[derive(Debug)]
pub struct CVarSystem {
    vars: Vec<CVar>,
    name_to_index: HashMap<String, usize>,
    userinfo_modified: bool,
}
```

- [x] **Step 4: Run tests**

Run: `cargo test -p q2-common -- cvar`
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add qwasm2-rs/crates/q2-common/src/cvar.rs
git commit -m "feat(q2-common): implement CVar system with HashMap storage, flags, latching"
```

---

### Task 1.5: Command Buffer and Parser (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/cmd.rs`
- Reference: `src/common/cmdparser.c`, `src/common/cbuf.c`

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn tokenize_simple() {
    let tokens = tokenize("map q2dm1");
    assert_eq!(tokens, vec!["map", "q2dm1"]);
}

#[test]
fn tokenize_quoted() {
    let tokens = tokenize("say \"hello world\"");
    assert_eq!(tokens, vec!["say", "hello world"]);
}

#[test]
fn command_registration_and_execution() {
    let mut cmd = CmdSystem::new();
    let called = std::cell::Cell::new(false);
    cmd.add_command("test", |_args| { called.set(true); });
    cmd.execute_string("test");
    assert!(called.get());
}

#[test]
fn cbuf_execute_order() {
    let mut cmd = CmdSystem::new();
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec![]));
    let log2 = log.clone();
    cmd.add_command("echo", move |args| {
        log2.borrow_mut().push(args.get(1).unwrap_or(&"").to_string());
    });
    cmd.cbuf_add_text("echo first\necho second\n");
    cmd.cbuf_execute();
    assert_eq!(*log.borrow(), vec!["first", "second"]);
}
```

- [x] **Step 2: Run to verify failure, implement, run to verify pass**

- [x] **Step 3: Commit**

```bash
git commit -m "feat(q2-common): implement command buffer and tokenizer"
```

---

### Task 1.6: Network Message Read/Write (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/net_msg.rs`
- Reference: `src/common/szone.c`, `src/common/movemsg.c`, `src/common/header/common.h:88-146`

Replaces `sizebuf_t` + all `MSG_Write*`/`MSG_Read*` functions with a cursor-based byte buffer.

- [x] **Step 1: Write failing tests**

```rust
#[test]
fn write_read_roundtrip() {
    let mut buf = NetMsg::with_capacity(1400);
    buf.write_byte(42);
    buf.write_short(1234);
    buf.write_long(-1);
    buf.write_float(3.14);
    buf.write_string("hello");

    buf.begin_reading();
    assert_eq!(buf.read_byte(), 42);
    assert_eq!(buf.read_short(), 1234);
    assert_eq!(buf.read_long(), -1);
    assert!((buf.read_float() - 3.14).abs() < 0.01);
    assert_eq!(buf.read_string(), "hello");
}

#[test]
fn write_delta_entity() {
    let from = EntityState::default();
    let mut to = EntityState::default();
    to.origin = Vec3f::new(100.0, 200.0, 0.0);

    let mut buf = NetMsg::with_capacity(1400);
    buf.write_delta_entity(&from, &to, true, true);

    buf.begin_reading();
    // Should only encode changed fields (origin1, origin2)
    assert!(buf.len() < 100); // compact encoding
}
```

- [x] **Step 2: Run, implement (cursor-based `Vec<u8>` with read position), run, commit**

```bash
git commit -m "feat(q2-common): implement NetMsg buffer replacing sizebuf_t and MSG_* functions"
```

---

### Task 1.7: Zone Allocator (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/zone.rs`
- Reference: `src/common/zone.c`

The C zone allocator uses tagged `malloc` for bulk cleanup. In Rust, this becomes tag-based `Vec` pools.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn tag_alloc_and_free() {
    let mut zone = Zone::new();
    let _a = zone.alloc::<[u8; 64]>(Tag::Game);
    let _b = zone.alloc::<[u8; 128]>(Tag::Level);
    assert_eq!(zone.stats().count, 2);

    zone.free_tags(Tag::Level);
    assert_eq!(zone.stats().count, 1);
}
```

- [ ] **Step 2: Implement, test, commit**

```bash
git commit -m "feat(q2-common): implement tagged zone allocator with bulk free"
```

---

### Task 1.8: Virtual Filesystem (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/filesystem.rs`
- Reference: `src/common/filesystem.c`

Reads from directories and `.pak` files, with search path ordering.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn pak_file_loading() {
    // Create a minimal PAK file in memory
    let pak = create_test_pak(&[("test.txt", b"hello")]);
    let mut fs = FileSystem::new();
    fs.add_pak_from_bytes(&pak).unwrap();
    let data = fs.load_file("test.txt").unwrap();
    assert_eq!(data, b"hello");
}

#[test]
fn search_path_priority() {
    let mut fs = FileSystem::new();
    fs.add_directory("/game/baseq2");
    fs.add_directory("/game/mymod"); // mymod overrides baseq2
    // Directory added last has highest priority
}
```

- [ ] **Step 2: Implement, test, commit**

PAK format is trivial: 12-byte header + flat file entries. Use `std::io::Cursor` for in-memory reads. For WASM, the filesystem backend will be swapped to IndexedDB in Phase 6.

```bash
git commit -m "feat(q2-common): implement virtual filesystem with PAK support"
```

---

### Task 1.9: Collision Model (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/collision.rs`
- Reference: `src/common/collision.c`

BSP collision detection. This is the most math-heavy common module.

- [ ] **Step 1: Write tests for known collision scenarios**

```rust
#[test]
fn point_contents_empty() {
    let cm = CollisionModel::empty();
    assert_eq!(cm.point_contents(Vec3f::ZERO), 0);
}

#[test]
fn box_trace_open_space() {
    let cm = CollisionModel::empty();
    let trace = cm.box_trace(
        Vec3f::ZERO, Vec3f::new(100.0, 0.0, 0.0),
        Vec3f::splat(-16.0), Vec3f::splat(16.0),
        0, 0xFFFF,
    );
    assert_eq!(trace.fraction, 1.0);
    assert!(!trace.allsolid);
}
```

- [ ] **Step 2: Implement BSP traversal, trace, point contents**

This is a mostly mechanical translation — the math is identical, just with `glam::Vec3` ops instead of manual float arrays. Start with c2rust output of collision.c, then refactor to safe Rust.

- [ ] **Step 3: Run, commit**

```bash
git commit -m "feat(q2-common): implement BSP collision model (CM_BoxTrace, CM_PointContents)"
```

---

### Task 1.10: Player Movement Prediction (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/pmove.rs`
- Reference: `src/common/pmove.c`

Shared between client (prediction) and server (authoritative). Must produce identical results.

- [ ] **Step 1: Write tests**

```rust
#[test]
fn pmove_gravity() {
    let mut pm = PmoveContext::new(/* ... */);
    pm.state.velocity = [0, 0, 0];
    pm.state.gravity = 800;
    pm.cmd.msec = 100; // 100ms
    pm.execute();
    // After 100ms of freefall at 800 gravity, velocity.z should be negative
    assert!(pm.state.velocity[2] < 0);
}
```

- [ ] **Step 2: Implement, test, commit**

```bash
git commit -m "feat(q2-common): implement player movement prediction (Pmove)"
```

---

### Task 1.11: Network Channel (`q2-common`)

**Files:**
- Create: `qwasm2-rs/crates/q2-common/src/netchan.rs`
- Reference: `src/common/netchan.c`, `src/common/header/common.h:565-612`

This is the Quake 2 reliable/unreliable protocol layer. In Phase 7, we'll bridge this to Matchbox channels, but the protocol logic stays.

- [ ] **Step 1: Write tests for reliable message delivery**

```rust
#[test]
fn netchan_reliable_delivery() {
    let mut chan = Netchan::new(NetsrcServer, /* addr */);
    chan.message.write_byte(42);

    let packet = chan.transmit(&[]);
    assert!(packet.len() > 0);

    // Simulate receiving and processing
    let mut recv = Netchan::new(NetsrcClient, /* addr */);
    let msg = recv.process(&packet).unwrap();
    assert!(msg.is_some());
}
```

- [ ] **Step 2: Implement, test, commit**

```bash
git commit -m "feat(q2-common): implement Netchan reliable/unreliable protocol"
```

---

## Phase 2: Game Logic

The largest phase by LOC. Game logic is relatively self-contained — it communicates with the engine through the `GameImport`/`GameExport` trait boundary.

### Task 2.1: Game Traits (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/traits.rs`
- Reference: `src/game/header/game.h:105-237`

Replaces the `game_import_t`/`game_export_t` function pointer structs with Rust traits.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn mock_game_import() {
    struct MockImport;
    impl GameImport for MockImport {
        fn bprintf(&self, _level: i32, msg: &str) { assert_eq!(msg, "test"); }
        fn sound_index(&self, name: &str) -> i32 { 0 }
        // ... all required methods
    }
    let gi = MockImport;
    gi.bprintf(0, "test");
}
```

- [ ] **Step 2: Define traits**

```rust
// traits.rs
//
// BORROW STRATEGY: The game holds &GameImport while running. The server
// implements GameImport using interior mutability (RefCell) for state that
// the game needs to mutate (multicast buffer, entity linking). Read-only
// operations (trace, point_contents, index lookups) use &self naturally.
// Network message writes go to a command queue that the server drains
// after the game frame returns. This avoids the &mut game + &mut server
// simultaneous borrow problem.

pub trait GameImport: Send + Sync {
    // --- Printing (writes to internal buffer, no &mut needed) ---
    fn bprintf(&self, printlevel: i32, msg: &str);
    fn dprintf(&self, msg: &str);
    fn cprintf(&self, ent_idx: Option<usize>, printlevel: i32, msg: &str);
    fn centerprintf(&self, ent_idx: Option<usize>, msg: &str);

    // --- Sound (enqueued as command, processed after game frame) ---
    fn sound(&self, ent_idx: Option<usize>, channel: i32, sound_index: i32,
             volume: f32, attenuation: f32, time_ofs: f32);

    // --- Resource indexing (read-only lookups) ---
    fn model_index(&self, name: &str) -> i32;
    fn sound_index(&self, name: &str) -> i32;
    fn image_index(&self, name: &str) -> i32;
    fn set_model(&self, ent_idx: usize, name: &str);

    // --- Collision (read-only BSP queries) ---
    fn trace(&self, start: Vec3f, mins: Vec3f, maxs: Vec3f, end: Vec3f,
             pass_ent: Option<usize>, content_mask: i32) -> Trace;
    fn point_contents(&self, point: Vec3f) -> i32;
    fn in_pvs(&self, p1: Vec3f, p2: Vec3f) -> bool;
    fn in_phs(&self, p1: Vec3f, p2: Vec3f) -> bool;
    fn areas_connected(&self, area1: i32, area2: i32) -> bool;
    fn set_area_portal_state(&self, portalnum: i32, open: bool);

    // --- Entity linking (uses interior mutability — RefCell<WorldState>) ---
    fn link_entity(&self, ent_idx: usize);
    fn unlink_entity(&self, ent_idx: usize);
    fn box_edicts(&self, mins: Vec3f, maxs: Vec3f, max_count: usize, area_type: i32) -> Vec<usize>;
    fn pmove(&self, pmove: &mut PmoveContext);

    // --- Network message writing (command queue, drained after game frame) ---
    fn configstring(&self, num: i32, string: &str);
    fn write_byte(&self, c: i32);
    fn write_short(&self, c: i32);
    fn write_long(&self, c: i32);
    fn write_float(&self, f: f32);
    fn write_string(&self, s: &str);
    fn write_position(&self, pos: Vec3f);
    fn write_dir(&self, dir: Vec3f);
    fn write_angle(&self, f: f32);
    fn multicast(&self, origin: Vec3f, to: Multicast);
    fn unicast(&self, ent_idx: usize, reliable: bool);

    // --- CVar access ---
    fn cvar_get(&self, name: &str, default: &str, flags: u32) -> CVarHandle;
    fn cvar_set(&self, name: &str, value: &str);
    fn cvar_forceset(&self, name: &str, value: &str);
    fn cvar_value(&self, handle: CVarHandle) -> f32;
    fn cvar_string(&self, handle: CVarHandle) -> String;

    // --- Command execution ---
    fn add_command_string(&self, text: &str);
    fn argc(&self) -> i32;
    fn argv(&self, n: i32) -> String;
    fn args(&self) -> String;

    // --- Memory (tag-based allocation) ---
    fn tag_malloc(&self, size: usize, tag: i32) -> *mut u8;
    fn tag_free(&self, ptr: *mut u8);
    fn free_tags(&self, tag: i32);
}

pub trait GameExport: Send + Sync {
    fn api_version(&self) -> i32 { 3 }
    fn init(&mut self);
    fn shutdown(&mut self);
    fn spawn_entities(&mut self, mapname: &str, entstring: &str, spawnpoint: &str);
    fn write_game(&self, filename: &str, autosave: bool);
    fn read_game(&mut self, filename: &str);
    fn write_level(&self, filename: &str);
    fn read_level(&mut self, filename: &str);
    fn client_connect(&mut self, ent_idx: usize, userinfo: &str) -> bool;
    fn client_begin(&mut self, ent_idx: usize);
    fn client_disconnect(&mut self, ent_idx: usize);
    fn client_command(&mut self, ent_idx: usize);
    fn client_think(&mut self, ent_idx: usize, cmd: &UserCmd);
    fn run_frame(&mut self);
    fn server_command(&mut self);
}
```

- [ ] **Step 3: Test, commit**

```bash
git commit -m "feat(q2-game): define GameImport/GameExport traits replacing C function pointer tables"
```

---

### Task 2.2: Entity System (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/entity.rs`
- Reference: `src/game/header/game.h:56-98`, `src/game/header/local.h`

Replaces `edict_t` array with `SlotMap`-based entity storage. This is the most important architectural decision in the game layer.

- [ ] **Step 1: Design and test entity storage**

```rust
use slotmap::{SlotMap, new_key_type};

new_key_type! { pub struct EntityKey; }

#[derive(Debug)]
pub struct Entity {
    pub state: EntityState,
    pub in_use: bool,
    pub solid: Solid,
    pub svflags: u32,
    pub mins: Vec3f,
    pub maxs: Vec3f,
    pub absmin: Vec3f,
    pub absmax: Vec3f,
    pub size: Vec3f,
    pub clipmask: i32,
    pub owner: Option<EntityKey>, // replaces edict_t *owner
    pub client: Option<ClientData>,
    pub game: GameEntityData, // all the game-specific fields from local.h
}

pub struct EntityStorage {
    pub entities: SlotMap<EntityKey, Entity>,
    pub max_entities: usize,
}

#[test]
fn entity_create_and_lookup() {
    let mut storage = EntityStorage::new(1024);
    let key = storage.spawn();
    assert!(storage.get(key).unwrap().in_use);
    storage.free(key);
    assert!(storage.get(key).is_none());
}

#[test]
fn entity_owner_reference() {
    let mut storage = EntityStorage::new(1024);
    let owner = storage.spawn();
    let child = storage.spawn();
    storage.get_mut(child).unwrap().owner = Some(owner);
    let child_owner = storage.get(child).unwrap().owner.unwrap();
    assert!(storage.get(child_owner).is_some());
}
```

- [ ] **Step 2: Implement entity storage**

- [ ] **Step 3: Test, commit**

```bash
git commit -m "feat(q2-game): implement SlotMap-based entity system replacing edict_t arrays"
```

---

### Task 2.3: Game Spawn System (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/spawn.rs`
- Reference: `src/game/g_spawn.c`

Parses BSP entity strings and creates game entities.

- [ ] **Step 1: Write tests for entity string parsing**

```rust
#[test]
fn parse_entity_string() {
    let entstring = r#"
    {
    "classname" "info_player_start"
    "origin" "0 0 0"
    "angle" "90"
    }
    "#;
    let ents = parse_entity_string(entstring);
    assert_eq!(ents.len(), 1);
    assert_eq!(ents[0].classname, "info_player_start");
    assert_eq!(ents[0].origin, Vec3f::ZERO);
}
```

- [ ] **Step 2: Implement spawn table (classname → spawn function mapping)**

In C this is a null-terminated `spawn_t` array. In Rust: `HashMap<&str, fn(&mut EntityStorage, &SpawnFields)>`.

- [ ] **Step 3: Test, commit**

```bash
git commit -m "feat(q2-game): implement entity spawn system from BSP entity strings"
```

---

### Task 2.4: Combat, Weapons, Items, Physics (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/combat.rs`
- Create: `qwasm2-rs/crates/q2-game/src/weapons.rs`
- Create: `qwasm2-rs/crates/q2-game/src/items.rs`
- Create: `qwasm2-rs/crates/q2-game/src/physics.rs`
- Reference: `src/game/g_combat.c`, `src/game/g_weapon.c`, `src/game/g_items.c`, `src/game/g_phys.c`

These are mostly mechanical translations. The logic is straightforward math and state updates.

- [ ] **Step 1: Write tests for damage calculation**

```rust
#[test]
fn damage_reduces_health() {
    let mut world = GameWorld::new_test();
    let attacker = world.spawn_player();
    let target = world.spawn_player();
    world.entity_mut(target).health = 100;
    world.apply_damage(target, attacker, 25, /* ... */);
    assert_eq!(world.entity(target).health, 75);
}
```

- [ ] **Step 2: Implement each subsystem with tests**

- [ ] **Step 3: Commit each module**

```bash
git commit -m "feat(q2-game): implement combat, weapons, items, and physics systems"
```

---

### Task 2.5: Monster AI (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/ai.rs`
- Create: `qwasm2-rs/crates/q2-game/src/monster/` (22 files)
- Reference: `src/game/g_ai.c`, `src/game/monster/`

This is ~8000 LOC of state machines. Each monster has: stand, walk, run, attack, pain, die states with frame-by-frame animation tables.

Design: Monster state machines become enum-based state patterns:

```rust
#[derive(Debug)]
pub enum MonsterState {
    Stand { frames: &'static [AnimFrame] },
    Walk { frames: &'static [AnimFrame], next: Box<MonsterState> },
    Run { frames: &'static [AnimFrame] },
    Attack { frames: &'static [AnimFrame], attack_fn: fn(&mut Entity, &GameImport) },
    Pain { frames: &'static [AnimFrame], level: PainLevel },
    Dead { frames: &'static [AnimFrame] },
}
```

- [ ] **Step 1: Write tests for AI state transitions**
- [ ] **Step 2: Implement base AI (`ai.rs`)**
- [ ] **Step 3: Port monster implementations (start with soldier as template)**
- [ ] **Step 4: Port remaining monsters using soldier as pattern**
- [ ] **Step 5: Commit**

```bash
git commit -m "feat(q2-game): implement monster AI with 22 enemy type state machines"
```

---

### Task 2.6: Save/Load System (`q2-game`)

**Files:**
- Create: `qwasm2-rs/crates/q2-game/src/savegame/`
- Reference: `src/game/savegame/`

Replaces the C function-pointer serialization tables with `serde`.

The C system maps string names to function addresses for deserialization. In Rust, we use `serde` with a registry.

**SlotMap key stability:** `EntityKey` values contain version counters that are NOT stable across save/load. The save system serializes entity cross-references (`owner`, `enemy`, `goalentity`, `movetarget`) as **integer indices** (position in the entity array), then rebuilds `EntityKey` handles on load by re-inserting entities in order and building an index→key mapping table.

```rust
// Instead of functionList_t mapping strings → fn ptrs,
// use serde with #[serde(tag = "type")] for polymorphic serialization.
#[derive(Serialize, Deserialize)]
pub struct SaveGame {
    pub game_state: GameLocals,
    pub level_state: LevelLocals,
    pub entities: Vec<SavedEntity>,
}

/// Entity cross-references use indices, not EntityKeys, for save stability.
#[derive(Serialize, Deserialize)]
pub struct SavedEntity {
    pub index: usize,
    pub owner_index: Option<usize>,      // rebuilt to EntityKey on load
    pub enemy_index: Option<usize>,
    pub goalentity_index: Option<usize>,
    pub movetarget_index: Option<usize>,
    // ... all other serializable fields
}
```

- [ ] **Step 1: Write roundtrip serialization tests**
- [ ] **Step 2: Implement serde-based save/load**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(q2-game): implement serde-based save/load replacing function pointer tables"
```

---

## Phase 3: Server

### Task 3.1: Server State (`q2-server`)

**Files:**
- Create: `qwasm2-rs/crates/q2-server/src/state.rs`
- Reference: `src/server/header/server.h`

Replaces global `sv`/`svs` with owned structs.

```rust
pub struct Server {
    pub state: ServerState,
    pub time: u32,
    pub framenum: i32,
    pub name: String,
    pub configstrings: Vec<String>,
    pub baselines: Vec<EntityState>,
    pub multicast: NetMsg,
}

pub struct ServerStatic {
    pub initialized: bool,
    pub realtime: i32,
    pub clients: Vec<ClientSlot>,
    pub client_entities: Vec<EntityState>,
    pub challenges: Vec<Challenge>,
}
```

- [ ] **Step 1: Write tests for server initialization**
- [ ] **Step 2: Implement Server/ServerStatic structs**
- [ ] **Step 3: Implement GameImport trait for server (game_iface.rs)**
- [ ] **Step 4: Implement client connection handling**
- [ ] **Step 5: Implement frame building and message sending**
- [ ] **Step 6: Commit**

```bash
git commit -m "feat(q2-server): implement server state, game interface, and frame building"
```

---

### Task 3.2: Server World (`q2-server`)

**Files:**
- Create: `qwasm2-rs/crates/q2-server/src/world.rs`
- Reference: `src/server/sv_world.c`

Spatial partitioning for entity linking. Replaces the `link_t` doubly-linked list tree.

- [ ] **Step 1: Write tests for entity linking and area queries**
- [ ] **Step 2: Implement with Vec-based spatial partitioning (replacing link_t chains)**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(q2-server): implement spatial partitioning replacing link_t chains"
```

---

## Phase 4: Client

### Task 4.1: Client State and Connection (`q2-client`)

**Files:**
- Create: `qwasm2-rs/crates/q2-client/src/state.rs`
- Create: `qwasm2-rs/crates/q2-client/src/parse.rs`
- Reference: `src/client/header/client.h`, `src/client/cl_parse.c`

Replaces global `cl`/`cls` with owned structs.

- [ ] **Step 1-6: Implement client state, server message parsing, entity interpolation, prediction, effects, view setup**

```bash
git commit -m "feat(q2-client): implement client state, parsing, prediction, and rendering setup"
```

---

### Task 4.2: Input System (`q2-client`)

**Files:**
- Create: `qwasm2-rs/crates/q2-client/src/input.rs`
- Reference: `src/client/input/sdl.c`

- [ ] **Steps: Implement keyboard/mouse/gamepad abstraction**

```bash
git commit -m "feat(q2-client): implement input system abstraction"
```

---

### Task 4.3: Console, Menu, Sound (`q2-client`)

**Files:**
- Create: `qwasm2-rs/crates/q2-client/src/console.rs`
- Create: `qwasm2-rs/crates/q2-client/src/menu/`
- Create: `qwasm2-rs/crates/q2-client/src/sound/`
- Reference: `src/client/cl_console.c`, `src/client/menu/`, `src/client/sound/`

- [ ] **Steps: Implement console, menu system, sound mixer**

```bash
git commit -m "feat(q2-client): implement console, menu system, and sound subsystem"
```

---

## Phase 5: Renderer

### Task 5.1: Renderer Trait and GL3 Implementation (`q2-render`)

**Files:**
- Create: `qwasm2-rs/crates/q2-render/src/lib.rs` (RefExport/RefImport traits)
- Create: `qwasm2-rs/crates/q2-render/src/gl3/`
- Reference: `src/client/refresh/gl3/`, `src/client/refresh/ref_shared.h`

Use `glow` crate for OpenGL ES 3.0 / WebGL 2.0 abstraction (works on both native and WASM).

```rust
// lib.rs — renderer abstraction
pub trait Renderer {
    fn init(&mut self, width: i32, height: i32) -> Q2Result<()>;
    fn shutdown(&mut self);
    fn begin_frame(&mut self, camera_separation: f32);
    fn render_frame(&mut self, refdef: &RefDef);
    fn end_frame(&mut self);
    fn draw_char(&mut self, x: i32, y: i32, ch: i32);
    fn draw_pic(&mut self, x: i32, y: i32, name: &str);
    fn register_model(&mut self, name: &str) -> Option<ModelHandle>;
    fn register_skin(&mut self, name: &str) -> Option<ImageHandle>;
    fn register_pic(&mut self, name: &str) -> Option<ImageHandle>;
    fn set_sky(&mut self, name: &str, rotate: f32, axis: Vec3f);
    // ... remaining API
}
```

- [ ] **Step 1: Define renderer trait matching C refexport_t**
- [ ] **Step 2: Implement GL3 renderer using `glow`**
  - Port shaders from `gl3_shaders.c` (GLSL source strings)
  - Port BSP world rendering
  - Port MD2 mesh rendering
  - Port lightmap system
  - Port 2D drawing (HUD, console chars)
- [ ] **Step 3: Port image loaders (PCX, WAL, TGA) to pure Rust**
- [ ] **Step 4: Port BSP/MD2/SP2 model loaders**
- [ ] **Step 5: Commit**

```bash
git commit -m "feat(q2-render): implement GL3/GLES3 renderer with glow, image and model loaders"
```

---

## Phase 6: Platform Layer and WASM Backend

### Task 6.1: WASM Platform Backend (`q2-platform`)

**Files:**
- Create: `qwasm2-rs/crates/q2-platform/src/wasm/`
- Create: `qwasm2-rs/web/index.html`
- Reference: `src/backends/wasm/`, `wasm/shell.html`

Replaces Emscripten integration with `wasm-bindgen` + `web-sys`.

```rust
// wasm/main.rs
use wasm_bindgen::prelude::*;
use web_sys::{window, HtmlCanvasElement, WebGl2RenderingContext};

#[wasm_bindgen(start)]
pub fn main() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    tracing_wasm::set_as_global_default();

    let window = window().unwrap();
    let document = window.document().unwrap();
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("canvas")
        .unwrap()
        .dyn_into()?;

    // Create WebGL2 context
    let gl = canvas
        .get_context("webgl2")?
        .unwrap()
        .dyn_into::<WebGl2RenderingContext>()?;

    // Initialize engine
    let engine = Engine::new(gl);

    // requestAnimationFrame loop
    request_animation_frame(engine);
    Ok(())
}
```

- [ ] **Step 1: Implement WebGL2 context creation**
- [ ] **Step 2: Implement requestAnimationFrame game loop**
- [ ] **Step 3: Implement pointer lock for mouse capture**
- [ ] **Step 4: Implement keyboard/gamepad input via web-sys**
- [ ] **Step 5: Implement IndexedDB storage for saves (replacing initfs/syncfs)**
- [ ] **Step 6: Implement file export (download saves/demos)**
- [ ] **Step 7: Create `web/index.html` with canvas, loading UI, console**
- [ ] **Step 8: Commit**

```bash
git commit -m "feat(q2-platform): implement WASM backend with wasm-bindgen, WebGL2, IndexedDB"
```

---

### Task 6.2: Native Platform Backend (`q2-platform`)

**Files:**
- Create: `qwasm2-rs/crates/q2-platform/src/native/`

- [ ] **Steps: Implement SDL2-based native backend (window, input, GL context)**

```bash
git commit -m "feat(q2-platform): implement native SDL2 backend for desktop"
```

---

## Phase 7: P2P Networking with Matchbox

### Task 7.1: Matchbox Transport Layer (`q2-net`)

**Files:**
- Create: `qwasm2-rs/crates/q2-net/src/transport.rs`
- Create: `qwasm2-rs/crates/q2-net/src/channels.rs`

```rust
// transport.rs
use matchbox_socket::{WebRtcSocket, ChannelConfig, PeerId};

pub struct GameTransport {
    socket: WebRtcSocket,
    /// Channel 0: unreliable, unordered — game state snapshots at 20Hz
    state_channel: usize,
    /// Channel 1: reliable, ordered — chat, commands, player join/leave
    reliable_channel: usize,
}

impl GameTransport {
    pub async fn connect(room_url: &str) -> Self {
        let (socket, loop_fut) = WebRtcSocket::builder(room_url)
            .add_channel(ChannelConfig::unreliable())   // channel 0: game state
            .add_channel(ChannelConfig::reliable())      // channel 1: commands
            .build();

        // Spawn the socket loop
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(loop_fut);
        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(loop_fut);

        Self { socket, state_channel: 0, reliable_channel: 1 }
    }

    pub fn send_game_state(&mut self, peer: PeerId, data: &[u8]) {
        self.socket.channel(self.state_channel).send(data.into(), peer);
    }

    pub fn send_reliable(&mut self, peer: PeerId, data: &[u8]) {
        self.socket.channel(self.reliable_channel).send(data.into(), peer);
    }

    pub fn receive_game_state(&mut self) -> Vec<(PeerId, Vec<u8>)> {
        self.socket.channel(self.state_channel).receive()
    }

    pub fn receive_reliable(&mut self) -> Vec<(PeerId, Vec<u8>)> {
        self.socket.channel(self.reliable_channel).receive()
    }
}
```

- [ ] **Step 1: Write tests with mock transport**
- [ ] **Step 2: Implement Matchbox transport wrapper**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(q2-net): implement Matchbox WebRTC transport with dual channels"
```

---

### Task 7.2: Q2 Protocol over Matchbox (`q2-net`)

**Files:**
- Create: `qwasm2-rs/crates/q2-net/src/protocol.rs`
- Create: `qwasm2-rs/crates/q2-net/src/lobby.rs`

Bridge the Q2 `netchan` protocol to Matchbox channels.

```rust
// protocol.rs — adapts Q2 network protocol to run over Matchbox
pub struct Q2NetAdapter {
    transport: GameTransport,
    /// Maps Matchbox PeerIds to Q2 client slots
    peer_to_client: HashMap<PeerId, usize>,
}

impl Q2NetAdapter {
    /// Replaces NET_GetPacket — reads from Matchbox channels
    pub fn get_packet(&mut self) -> Option<(usize, NetMsg)> {
        // Check unreliable channel first (game state)
        for (peer, data) in self.transport.receive_game_state() {
            if let Some(&client_idx) = self.peer_to_client.get(&peer) {
                return Some((client_idx, NetMsg::from_bytes(&data)));
            }
        }
        // Then reliable channel
        for (peer, data) in self.transport.receive_reliable() {
            if let Some(&client_idx) = self.peer_to_client.get(&peer) {
                return Some((client_idx, NetMsg::from_bytes(&data)));
            }
        }
        None
    }

    /// Replaces NET_SendPacket
    pub fn send_packet(&mut self, client_idx: usize, data: &[u8], reliable: bool) {
        // Find peer for this client
        if let Some(peer) = self.client_to_peer(client_idx) {
            if reliable {
                self.transport.send_reliable(peer, data);
            } else {
                self.transport.send_game_state(peer, data);
            }
        }
    }
}
```

- [ ] **Step 1: Write tests for protocol adapter**
- [ ] **Step 2: Implement lobby/matchmaking**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(q2-net): bridge Q2 network protocol to Matchbox P2P channels"
```

---

### Task 7.3: Dev Server with Embedded Signaling (`q2-devserver`)

**Files:**
- Create: `qwasm2-rs/crates/q2-devserver/Cargo.toml`
- Create: `qwasm2-rs/crates/q2-devserver/src/main.rs`

All-Rust dev server: serves WASM files with correct MIME types AND embeds a Matchbox signaling endpoint. Single `cargo run` gives you everything needed for local development and multiplayer testing.

```rust
use axum::{Router, routing::get};
use tower_http::services::ServeDir;
use matchbox_signaling::SignalingServer;

#[tokio::main]
async fn main() {
    tracing_subscriber::init();

    // Matchbox signaling at /ws
    let signaling = SignalingServer::client_server_builder("0.0.0.0:3536")
        .on_connection_request(|info| {
            tracing::info!("peer connecting: {:?}", info);
            Ok(true)
        })
        .build();

    // Static file server for web/ directory with WASM MIME types
    let app = Router::new()
        .nest_service("/", ServeDir::new("web"));

    tracing::info!("Dev server: http://localhost:8080");
    tracing::info!("Signaling:  ws://localhost:3536");

    // Run both concurrently
    tokio::join!(
        async { axum::serve(tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap(), app).await.unwrap() },
        signaling.serve(),
    );
}
```

- [ ] **Step 1: Implement devserver with static file serving + signaling**
- [ ] **Step 2: Test with `cargo run -p q2-devserver` + two browser tabs**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(q2-devserver): all-Rust dev server with WASM serving and Matchbox signaling"
```

---

## Phase 8: Integration and Polish

### Task 8.1: Binary Entry Points (`q2-bin`)

**Files:**
- Create: `qwasm2-rs/crates/q2-bin/src/main.rs`
- Create: `qwasm2-rs/crates/q2-bin/src/wasm.rs`

Wire everything together.

```rust
// main.rs — native entry point
fn main() -> anyhow::Result<()> {
    tracing_subscriber::init();

    let args: Vec<String> = std::env::args().collect();
    let mut engine = Engine::new(args)?;

    loop {
        match engine.frame() {
            Ok(()) => {},
            Err(Q2Error::Drop(msg)) => {
                tracing::warn!("ERR_DROP: {msg}");
                engine.disconnect();
            }
            Err(Q2Error::Quit) => break,
            Err(e) => {
                tracing::error!("Fatal: {e}");
                return Err(e.into());
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 1: Implement native main loop with error recovery**
- [ ] **Step 2: Implement WASM entry point with requestAnimationFrame**
- [ ] **Step 3: Integration test — load a map and render one frame**
- [ ] **Step 4: Commit**

```bash
git commit -m "feat(q2-bin): wire engine together with native and WASM entry points"
```

---

### Task 8.2: Unsafe Audit Pass

**Files:**
- All crates

- [ ] **Step 1: Run `cargo clippy --workspace -- -D clippy::undocumented_unsafe_blocks`**
- [ ] **Step 2: Generate unsafe audit report**

```bash
# Count unsafe blocks by category across workspace
rg 'SAFETY\(c2rust\)' --count qwasm2-rs/
rg 'SAFETY\(ffi\)' --count qwasm2-rs/
rg 'SAFETY\(perf\)' --count qwasm2-rs/
rg 'SAFETY\(inherent\)' --count qwasm2-rs/
```

- [ ] **Step 3: Update all file-level audit headers**
- [ ] **Step 4: Create `docs/unsafe-audit.md` with totals and rewrite priority list**
- [ ] **Step 5: Commit**

```bash
git commit -m "docs: comprehensive unsafe audit with rewrite priorities"
```

---

### Task 8.3: Performance Profiling and Optimization

- [ ] **Step 1: Profile WASM build with Chrome DevTools**
- [ ] **Step 2: Profile native build with `perf` / `flamegraph`**
- [ ] **Step 3: Identify hot paths where unsafe might be justified**
- [ ] **Step 4: Document in `docs/performance.md`**
- [ ] **Step 5: Commit**

```bash
git commit -m "docs: performance profiling results and optimization notes"
```

---

## Dependency Graph (Execution Order)

```
Phase 0 ──→ Phase 1 ──→ Phase 2 ──→ Phase 3 ──→ Phase 4 ──→ Phase 5 ──→ Phase 6 ──→ Phase 7 ──→ Phase 8
scaffold     shared+      game         server       client       renderer     platform     networking   polish
             common       logic                                                WASM         matchbox
```

Phases are strictly sequential — each depends on the previous. Within a phase, tasks can sometimes be parallelized (e.g., monster AI types in Phase 2 are independent of each other).

---

## Estimated Scope Per Phase

| Phase | New Rust LOC (est.) | C LOC Replaced | Key Risk |
|-------|--------------------:|---------------:|----------|
| 0 | ~500 | 0 | None — scaffold only |
| 1 | ~8,000 | ~11,000 | Collision model fidelity, pmove determinism |
| 2 | ~18,000 | ~22,000 | Entity system design, monster state machines |
| 3 | ~5,000 | ~6,000 | Server-game interface correctness |
| 4 | ~12,000 | ~17,000 | Client prediction matching server |
| 5 | ~20,000 | ~36,000 | GL3 shader porting, texture pipeline |
| 6 | ~4,000 | ~5,000 | IndexedDB async, WebGL2 context |
| 7 | ~3,000 | 0 (new) | WebRTC NAT traversal, latency |
| 8 | ~1,000 | 0 | Audit only |
| **Total** | **~71,500** | **~97,000** | |

Note: Rust LOC is lower than C LOC replaced because Rust is more expressive (no header files, no manual memory management boilerplate, enum pattern matching vs. switch/if chains).

---

## Key Design Decisions

### 1. SlotMap over raw pointers for entities
The C code uses `edict_t*` pointers everywhere. SlotMap gives O(1) access with generation-checked keys — dangling references become `None` instead of UB.

### 2. Traits over function pointer tables
`game_import_t` (25+ fn ptrs) and `game_export_t` (10+ fn ptrs) become traits. The server implements `GameImport`, the game module implements `GameExport`. This is the natural Rust equivalent.

### 3. `Result<T, Q2Error>` over setjmp/longjmp
Every function that can fail returns `Q2Result<T>`. The main loop catches `Q2Error::Drop` and continues. This is the single biggest safety improvement.

### 4. Owned structs over global mutable state
`sv`/`svs`/`cl`/`cls` become owned fields in `Engine`. Subsystems receive `&mut` references. No `static mut` anywhere.

### 5. `glow` for GL abstraction
Works on native OpenGL and WebGL2. Single codebase for both targets. No Emscripten needed.

### 6. Matchbox for P2P networking
Unreliable channel for game state (20Hz, ~1KB, drop stale), reliable channel for commands. WebRTC with STUN/TURN for NAT traversal. Signaling server only handles connection setup.

### 7. serde for save/load and config (NOT network messages)
Save games and config files use `serde` + `bincode`. Replaces the C function-pointer-to-string serialization tables. **Network messages use the custom `NetMsg` binary format** (Task 1.6) with delta compression — this is a hand-tuned wire protocol that must stay compact for 20Hz real-time gameplay. serde would add unacceptable overhead and break compatibility with the Q2 protocol.

### 8. cpal + lewton for cross-platform audio
`cpal` provides cross-platform audio output — on native it uses ALSA/CoreAudio/WASAPI, on WASM it uses the Web Audio API. `lewton` provides pure-Rust OGG Vorbis decoding (replacing `stb_vorbis.h`). This eliminates the SDL audio and OpenAL dependencies.

### 9. Interior mutability for GameImport (command queue pattern)
The game holds `&GameImport` (immutable) while running its frame. The server implements `GameImport` using `RefCell` for mutable state (multicast buffer, entity linking). Network message writes (`write_byte`, `multicast`, etc.) enqueue commands into an internal buffer. The server drains this buffer after the game frame returns. This avoids the `&mut game` + `&mut server` simultaneous borrow problem that is otherwise unsolvable with the callback-based architecture.

### 10. axum for all-Rust tooling
The dev server and signaling server are both Rust binaries using `axum`. `q2-devserver` serves the `web/` directory with correct WASM MIME types and embeds a Matchbox signaling endpoint at `/ws`. No Python, Node, or external tooling required — `cargo run -p q2-devserver` gives you everything.
