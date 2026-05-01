# PR Review: server-0.1.2 → feat/playable-prototype

**Branch:** `server-0.1.2`  
**Base:** `feat/playable-prototype`  
**Reviewer:** Claude Code  
**Date:** 2026-05-01  
**Tests:** 294 passing, 0 failing  
**Clippy:** 0 warnings  

---

## Verdict: APPROVED ✓

CP-3 acceptance criteria are met. CP-4 delta machinery is complete and correct.
Two non-blocking notes follow.

---

## Summary of changes

Four logical commits:

| Commit | Scope | Lines |
|--------|-------|-------|
| feat(q2-server): GameImport bridge + CP-3 frame loop | q2-server, q2-game | +664 |
| feat(filesystem): PakReader trait + lazy WASM backend | q2-common, q2-platform | +285 |
| feat(client): full Q2 protocol delta machinery | q2-client, q2-common | +2065 |
| chore(release): bump workspace 0.1.1 → 0.1.2 | all crates, CHANGELOG | +26 |

---

## Code Analysis

### CP-3: ServerGameImport bridge (`q2-server/src/game_iface.rs`)

Correct. `Mutex<GiInner>` gives `Send + Sync` for the WASM single-threaded target.
All write-then-flush semantics (write_byte → unicast clears buf) are verified by 11 unit tests.
The configstring bounds check (`num < 0` guard + `idx < len` guard) is correct.

### CP-3: GameLogic (`q2-game/src/game.rs`)

Correct. `ensure_slot()` + `player_slots: Vec<Option<EntityKey>>` safely handles
out-of-order client indices. `client_begin` frees any stale entity for the slot before
spawning a new one — no double-registration possible.

### CP-3: Integration tests (`q2-server/tests/cp3_integration.rs`)

Good coverage. `cp3_game_init_dispatches_configstring_through_vtable` verifies the
`&dyn GameImport` vtable path specifically — this is the right test to have.

### CP-4: MOREBITS chain fix (`q2-common/src/net_msg.rs`)

**Bug fix verified as correct.** The original code set MOREBITS3 before MOREBITS2,
so the decoder would stop reading after byte 1 (MOREBITS1 not set) and miss bytes 2-3.
Fix evaluates flags high→low so each MOREBITS flag propagates to the byte below it.
Round-trip tests exercise this: any entity delta touching bits in bytes 2-3 now encodes
and decodes correctly.

### CP-4: `read_player_state` (`q2-client/src/parse.rs:353`)

All 15 `PlayerStateFlags` decoded. `statbits` read as `u32` prevents UB from
`1i32 << 31` on stat slot 31. Verified against `sv_entities.c` — C uses the same
`PS_M_TYPE` … `PS_RDFLAGS` flag set and stat bitmask. `read_player_state` matches.

### CP-4: `read_packet_entities` merge-walk (`q2-client/src/parse.rs:446`)

Correct implementation of the merge-walk algorithm:
- Unchanged entities (old.number < newnum) are copied verbatim.
- REMOVE entities skip the old entry without decoding delta bytes.
- New entities delta from baseline; existing entities delta from old frame.
- Remaining old entities are appended after the loop.

`packet_entities_remove_roundtrip` test validates the removal path.

### CP-4: `parse_entity_bits` (`q2-client/src/parse.rs:205`)

Correct MOREBITS chain read (high-bit signals the next byte exists).
NUMBER16 flag correctly selects read_short vs read_byte for entity number.

### Filesystem: PakReader / JsPakReader (`q2-common/src/filesystem.rs`, `q2-platform/src/wasm/pak.rs`)

`JsPakReader` holds the PAK in JS heap as a `Uint8Array`. `read_slice()` calls
`Uint8Array::slice()` — only the requested file's bytes cross the JS/WASM boundary.
The `// SAFETY:` comment for `unsafe impl Send/Sync` is correct: WASM targets have no
OS threads, so the conservative default in js-sys does not apply.

---

## Non-Blocking Issues

### 1. `areabits_len` silent truncation can desync stream — `parse.rs:140`

```rust
let areabits_len = (msg.read_byte() as usize).min(MAX_MAP_AREAS / 8);
```

The C reference (`cl_parse.c:755-756`) reads exactly `len` bytes regardless of
`sizeof(areabits)`. If a server sends `len > 32`, the Rust code reads 32 bytes but
leaves `len - 32` extra bytes in the buffer — they become the next opcode byte and
corrupt the stream.

A conforming Q2 server always sends exactly 32 bytes, so this cannot occur in practice.
But the `.min()` is silent. Replace with an assertion:

```rust
let areabits_len = msg.read_byte() as usize;
debug_assert!(
    areabits_len <= MAX_MAP_AREAS / 8,
    "server sent oversized areabits: {} bytes",
    areabits_len
);
let areabits_len = areabits_len.min(MAX_MAP_AREAS / 8);
```

This surfaces a malformed server immediately in dev builds without changing release behavior.

### 2. `drain_configstring_updates` name vs semantics — `game_iface.rs:67`

The method is named "drain" but it returns all non-empty configstrings on every call
without clearing them. The in-code comment acknowledges this is a placeholder.

This is fine for CP-3 where configstrings are written once at init. For Phase 4 delta
compression, calling this method twice between frames will produce duplicate updates.
Before Phase 4 delta work lands, either:
- rename to `iter_configstring_updates`, or
- add a dirty-flag set per slot and clear after draining.

No action needed on this PR — just flag it before Phase 4 networking work starts.

---

## Protocol Deviation (Architecture Note)

Entity angles (`read_delta_entity`, line 300) use `read_angle16()` (16-bit, 2 bytes).
The C protocol uses `MSG_ReadAngle` (8-bit, 1 byte) for entity angles — 16-bit is only
used for player `viewangles`.

Encoder and decoder are consistent within the Rust codebase (write_delta_entity also
uses angle16), so there is no internal bug. However, this blocks wire interoperability
with a real Q2 server. Given the plan targets Matchbox P2P (not original Q2 server
compatibility), this is acceptable — but should be documented before Phase 7 networking
is designed, as it will affect the P2P protocol contract.

---

## Out-of-Scope (Deferred, per commit message)

The following stubs are explicitly deferred to Phase 4 per the commit message and are
not findings:
- `write_dir` (byte 0 placeholder, needs BYTEDIRS quantization)
- `link_entity` / `box_edicts` (requires shared EntityStorage access)
- `Arc<dyn GameImport>` storage decision for `run_frame` callbacks

These should be tracked in issues before Phase 4 networking work begins.

---

## Test Plan

Verify these pass before merging:

- [ ] `cargo test --workspace` → 294 passing, 0 failed
- [ ] `cargo clippy --workspace` → 0 warnings
- [ ] `cp3_server_runs_100_frames` passes
- [ ] `cp3_game_init_dispatches_configstring_through_vtable` passes
- [ ] `player_state_delta_stat_bit31_no_overflow` passes (guards u32 statbits)
- [ ] `packet_entities_remove_roundtrip` passes (guards REMOVE path)
- [ ] `delta_frame_uses_old_playerstate` passes (guards fov carry-forward)
- [ ] MOREBITS revert test: in net_msg.rs tests, `entity_delta_morebits_chain` encodes
  an entity with RENDERFX bits (byte 3) and verifies round-trip.

---

## PR Description (for when PR is opened)

**Title:** `feat: server frame loop, GameImport bridge, Q2 delta protocol, lazy WASM PAK (0.1.2)`

**Body:**
Reaches CP-3 acceptance (server runs 100 frames with real GameLogic) and lands the full
CP-4 delta protocol machinery (parse_frame, player-state delta, packet-entity delta).

Changes:
- `q2-server/src/game_iface.rs`: `ServerGameImport` — full `GameImport` trait impl backed by `Mutex<GiInner>`; 11 unit tests
- `q2-game/src/game.rs`: `GameLogic` — first concrete `GameExport` impl with entity storage and player slot management; 5 unit tests
- `q2-server/tests/cp3_integration.rs`: 4 CP-3 integration tests including vtable-dispatch verification
- `q2-client/src/parse.rs`: replace 3 stream-corrupting stubs with full delta decode (parse_frame, read_player_state, read_packet_entities, parse_entity_bits); 17 new parse tests
- `q2-common/src/net_msg.rs`: `write_player_state`, `write_packet_entities_list`; fix pre-existing MOREBITS chain bug; 7 encoder tests
- `q2-common/src/filesystem.rs`: `PakReader` trait; `DiskPakReader` / `InMemPakReader` backends
- `q2-platform/src/wasm/pak.rs`: `JsPakReader` — JS-heap `Uint8Array` backed reader; avoids 100 MB WASM copy on startup
- Version bump: 0.1.1 → 0.1.2 across all 13 crates; CHANGELOG.md added

Test plan: 294 workspace tests pass; 0 clippy warnings.
