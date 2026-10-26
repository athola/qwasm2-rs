# Full-codebase review: server-0.1.2 (2026-10-26)

Scope: every crate, the Makefile, CI and the browser tests, reviewed by six
independent passes (q2-common core, server/game/client, Rust safety,
architecture/ADR, numerics against the C original, build tooling). About 185
raw findings collapsed to roughly 140 distinct issues after deduplication. The
duplicate map and per-task breakdown are in
`docs/plans/2026-10-26-full-review-fixes.md`; finding IDs below (C-, S-, R-,
A-, M-, B-) refer to it.

Outcome: 72 commits after the lint gate (a3c1044..HEAD). Every finding is fixed,
rejected with a reason, or mapped to a tracking issue. `make ci` (fmt check,
clippy `--all-targets -D warnings` on host and wasm32, check, 565 tests),
`make wasm` and `make test-browser` (6/6) pass. `cargo audit` dropped from 19
vulnerabilities to 0, with one unmaintained warning (bincode, via q2-shared).

## Summary

Before the review, `main` CI was red because the toolchain was not pinned and a
new clippy lint shipped. Test code was never linted. The engine had several
input-driven failure modes: a runt packet poisoned the netchan sequence, a
truncated frame hung the client, a truncated or cyclic BSP panicked or looped,
and `load_file("/etc/hosts")` read host files. The wire format claimed
protocol 34 but used a custom entity encoding and a different usercmd field
order. The pmove port differed from C in float precision at several points.

## Domain findings

**Network and protocol.** Netchan now rejects runt and out-of-band packets,
reserves header space in the staging buffer like C, and sets `fatal_error` on
reliable overflow. Usercmd and entity deltas match protocol 34 byte for byte;
tests pin bytes derived from `movemsg.c`. New entities delta from baselines.
The client rejects truncated frames, negative or unsorted entity numbers and a
wrong protocol version, and skips opcodes it has no handler for instead of
dropping the rest of the packet.

**Collision and filesystem.** BSP cross-references are validated at load,
with child index greater than parent, which also rules out cycles. This check
is stricter than C; demo1-3 still load. An empty map no longer builds a
self-referencing box hull. Paths with `..` or absolute components are
rejected. PAK lengths are bounded before allocation, and PAKs override loose
files, as in C.

**Numerics.** AngleVectors, VectorNormalize and the 0.7 slope test follow the
C double/float mix. Fly move returns early when `addspeed <= 0`. Snapping wraps
like C shorts instead of saturating. Air acceleration is a runtime field.
`calc_fov` errors outside 1..179.

**Server and game.** Game frames run at 10 Hz like `sv_main.c`. Map change
clears configstrings and baselines. Configstrings the game writes now reach the
server each frame. Trace stubs return `endpos = end`, and `write_dir` uses the
real normal table. Out-of-range client slots return an error instead of
panicking.

**Render and wasm.** Negative or overflowing BSP fields are rejected with
checked casts. GL objects are freed on map reload and on shader errors. The
renderer's pitch sign now matches C. The dead `catch_unwind` (wasm32 is
panic=abort) is gone. A second `start_game` is refused.

**Safety and architecture.** The `unsafe impl Send/Sync` in q2-platform is
gone: the unused `Send + Sync` bounds on `PakReader`, `GameImport` and
`GameExport` were removed, and `Mutex` became `RefCell`. Unsafe now exists only
in q2-render (8 blocks, each with a SAFETY invariant). Unused dependencies,
including matchbox_socket in the q2-net stub (the source of all 19 audit
hits), were removed. README, ADR and CHANGELOG now describe what the code does.

**Build and tooling.** CI runs fmt, clippy with `--all-targets` on host and
wasm32, `--locked` tests and cargo audit, on a pinned toolchain. `make
test-browser` runs again. The demo download is checksummed. The devserver
parses Accept-Encoding q-values, sends `Vary` and no longer sets permissive
CORS. pak-repack reports Brotli write errors. The bundler fails when a glue
patch does not match.

## Behaviour changes to note

- The wire format is now C protocol 34. Nothing outside tests decodes it yet.
- About a dozen existing tests encoded non-C behaviour and were updated; each
  commit names the test it changed.
- PlayerController speed is clamped to the pmove limits (300, 100 ducked), so
  run and walk both cap at 300 until movement moves onto pmove.
- Public API changes: `Server::frame` takes `&ServerGameImport`, server time is
  u32 ms, `GameLogic::new(max_clients)`, `parse_server_message` and
  `write_packet_entities_list` return `Q2Result`, and `calc_fov` returns
  `Q2Result<f32>`.

## Deferred to existing issues

| Findings | Issue |
|----------|-------|
| S-5, R-24, S-25 rest (entity numbering, SlotMap world links) | #9, #31 |
| S-6, S-21 (spawn table, spawn reset) | #11 |
| S-7, S-17, S-30, A-4 (complete GameImport, per-frame import) | #18, #10 |
| S-12 rest, S-16 (temp entities, stufftext, interpolation) | #19, #6, #7 |
| A-2/M-8/C-25, A-29 (PlayerController on pmove, input path) | #12, #19 |
| A-5 (world loading through the Renderer trait) | #20 |
| B-12/R-37 (AST-based bundler patching) | #2 |

Five findings first proposed as new issues were fixed on this branch
instead, at the user's request:

- A-17: `Q2Error` is defined in q2-shared (q2-common re-exports it), so the
  Renderer trait, gl3 and q2-platform return `Q2Result` and reach JS
  through `q2err_to_js`.
- A-20: `q2_common::bsp_file` validates the BSP header and lump ranges for
  both collision and the renderer; collision loads the entity string like
  C CMod_LoadEntityString, and spawn reads it from there. The plane, node,
  leaf, texinfo and model records both loaders use are decoded once in
  `bsp_file`; each loader keeps its own transform and C checks. A
  fingerprint of both loaders on demo1-3 is identical before and after.
- C-20: `CollisionMap::world_trace` reports the world entity on a hit, and
  `clip_move_to_entities` ports C CL_ClipMoveToEntities, together the whole
  of CL_PMTrace; it is called once client prediction exists (#19).
  Server-side SV_ClipMoveToEntities needs linked entities and a server
  collision map (#11, #31).
- C-18(d): `areas_connected` and `set_area_portal_state` return ERR_DROP
  for out-of-range indices instead of silently answering.
- R-29: the engine refits render size and fov when the page resizes the
  canvas; checked live at 800x600 and 1200x500.

Fixed by the integrator after the workers merged: walk speed 200 so run is
faster again (the pm_maxspeed clamp had made them equal), A-21 (content
flags defined once in q2-shared), the q2-client half of M-15/A-24/A-26, and
workspace version inheritance. R-14b was already resolved by the T3 mesh
refactor. A headless run of demo1 against local gamedata confirmed mouse
look up/down and forward movement with no console errors.

## Not verified

- The CI workflow has not run on GitHub yet: rustup install, taiki-e install
  action, `actions/checkout@v5`.
- R-4 lump-overflow test cannot fail on a 64-bit host; no wasm test runner.
- R-13, R-15, R-30, R-31 are GL- or browser-only and have no unit test.
