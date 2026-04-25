# Bloat Detection Report — qwasm2-rs

**Scan Date:** 2026-03-26
**Level:** 3 (Deep Audit)
**Files Scanned:** 43 (.rs)
**Total Lines:** 12,148
**Compiler Warnings:** 0

## Summary

| Metric | Value |
|--------|-------|
| Total findings | 8 |
| High priority | 2 |
| Medium priority | 4 |
| Low priority | 2 |
| Estimated token savings | ~2,400 tokens |
| Context reduction | ~3% |

**Overall assessment: CLEAN.** This is a young codebase (single session) with minimal bloat. The findings are mostly structural — duplicate helpers and leftover artifacts — not dead code accumulation.

## High Priority (2)

### [H1] Duplicate binary read helpers — collision.rs + bsp.rs

**Score:** 82/100 | **Confidence:** HIGH
**Type:** CONSOLIDATE

Both `q2-common/src/collision.rs` and `q2-render/src/bsp.rs` implement identical `read_u16`, `read_i16`, `read_i32`, `read_f32` functions for parsing binary BSP data.

| File | Functions | Lines |
|------|-----------|-------|
| `collision.rs:1628-1658` | `read_u16`, `read_i16`, `read_u32`, `read_i32`, `read_f32` | 30 |
| `bsp.rs:182-207` | `read_u16`, `read_i16`, `read_u32`, `read_i32`, `read_f32` | 25 |

**Action:** Extract into `q2-common::binary` module shared by both crates.
**Token savings:** ~400
**Risk:** LOW — purely mechanical, no behavior change.

### [H2] Temp artifact: `rust_out` at project root

**Score:** 95/100 | **Confidence:** HIGH
**Type:** DELETE

Compiler artifact left by a background agent. Not gitignored.

**Action:** Delete file, add `rust_out` to `.gitignore`.
**Token savings:** 0 (not source)
**Risk:** NONE

## Medium Priority (4)

### [M1] Stub crate: `q2-bin` (3 lines)

**Score:** 65/100 | **Confidence:** MEDIUM
**Type:** DEFER

```rust
fn main() {
    println!("qwasm2-rs");
}
```

Declares 10 dependencies but uses none. Increases `cargo check` time.

**Action:** No action now — this is the future native entry point. But consider removing unused deps from its `Cargo.toml` until they're needed.
**Risk:** LOW

### [M2] Stub crate: `q2-net` (8 lines)

**Score:** 60/100 | **Confidence:** MEDIUM
**Type:** DEFER

Empty lib.rs, declares `matchbox_socket` + `q2-shared` + `q2-common` + `tracing` as dependencies. Not used by anything yet.

**Action:** No action — planned for Phase 7 (P2P networking). Deps are correct for future use.
**Risk:** LOW

### [M3] TODOs in server/client (20 total)

**Score:** 55/100 | **Confidence:** MEDIUM
**Type:** DEFER

20 TODO comments across `q2-server/src/frame.rs`, `q2-server/src/init.rs`, and `q2-client/src/parse.rs`. These are legitimate placeholders for unimplemented server/client functionality.

**Action:** No action — these mark real work items from the plan. They'll be replaced with implementation in future phases.

### [M4] Test blocks are 20-50% of some files

**Score:** 50/100 | **Confidence:** LOW
**Type:** MONITOR

| File | Total | Tests | Test % |
|------|-------|-------|--------|
| netchan.rs | 497 | 244 | 49% |
| filesystem.rs | 670 | 204 | 30% |
| cmd.rs | 360 | 159 | 44% |
| world.rs | 461 | 145 | 31% |

**Action:** No action — inline tests are idiomatic Rust. Only consider extracting to separate test files if individual files exceed ~1500 lines total.

## Low Priority (2)

### [L1] `q2-devserver` missing from `Makefile` test target

**Score:** 35/100
**Type:** MONITOR

The `make test` target excludes `q2-wasm` (correct, WASM-only) but doesn't explicitly test `q2-devserver` which has no tests.

**Action:** No action until devserver has test-worthy logic.

### [L2] `filesystem.rs` has two PAK loading paths (disk + memory)

**Score:** 30/100
**Type:** MONITOR

`Pack::load` (disk) and `Pack::load_from_bytes` (memory) share ~60 lines of duplicate directory-parsing logic.

**Action:** Consider extracting shared `parse_pak_directory(data: &[u8])` if a third path is ever added. Two paths is fine.

## Not Bloat (excluded from findings)

- **Collision.rs (1,823 lines):** Largest file, but it's a faithful port of Q2's collision.c. The C original is ~1,500 lines. Size is justified by the BSP traversal algorithm.
- **Pmove.rs (1,371 lines):** Second largest. The C original is ~1,200 lines. Size is justified by the movement physics.
- **183 tests:** Healthy test-to-code ratio (~1:5). Tests are not bloat.
- **Plan file (2,471 lines):** In `docs/`, not compiled. Useful reference.

## Recommended Actions

| Priority | Finding | Action | Risk |
|----------|---------|--------|------|
| 1 | H2 | Delete `rust_out`, add to `.gitignore` | NONE |
| 2 | H1 | Extract shared binary read helpers | LOW |
| 3 | M1 | Trim unused deps from q2-bin Cargo.toml | LOW |
| — | M2-M4, L1-L2 | No action needed | — |

## Next Scan

Schedule next scan after Phase 2-4 implementation when codebase exceeds 20K lines.
