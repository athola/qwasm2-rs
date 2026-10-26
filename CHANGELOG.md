# Changelog

All notable changes to qwasm2-rs are documented here.

## [Unreleased]

### Changed (breaking)
- Entity and usercmd encoding now match C protocol 34 byte for byte (8-bit
  entity angles, FRAME8/FRAME16 as in C, `event` sent whenever non-zero,
  usercmd `msec`/`lightlevel` written last); new entities delta from
  baselines. The client rejects any other protocol version
- `Server::frame` takes `&ServerGameImport` and runs game frames at 10 Hz;
  server time is `u32` milliseconds and `frametime` is gone
- `GameLogic::new(max_clients)` replaces `Default`; `GameImport`,
  `GameExport` and `PakReader` no longer require `Send + Sync`
- `parse_server_message`, `write_delta_entity`,
  `write_packet_entities_list`, `read_dir`, `parse_entity_string` and
  `calc_fov` return `Q2Result`; `NetChan::new` takes a source and qport
- `Q2Error`/`Q2Result` are defined in `q2_shared::error` (still re-exported
  from `q2_common::error`); `Renderer::init` and the q2-platform wasm
  functions return `Q2Result` instead of `Result<_, String>`
- `CollisionMap::areas_connected` and `set_area_portal_state` return
  `Q2Result` and reject out-of-range indices like C ERR_DROP
- BSP header constants and lump directory live in `q2_common::bsp_file`;
  `q2_render::bsp` no longer exports its own `LUMP_*`/`BspLump`, and
  `BspData::entities` is gone (use `CollisionMap::entity_string`)

### Added
- `CollisionMap::world_trace`, which reports the world entity on a hit
  like C CL_PMTrace
- The wasm engine refits render size and fov when the canvas is resized

### Fixed
- Netchan rejects runt packets, reserves header space when staging
  reliable data and sets `fatal_error` on overflow
- Client parsing no longer hangs on a truncated frame, stores a partial
  frame as valid, or accepts negative or unsorted entity numbers
- BSP loading rejects out-of-range or cyclic references and truncated
  lumps instead of panicking or looping (child index must exceed parent,
  which is stricter than C); an empty map no longer hangs the box hull
- Filesystem rejects absolute and `..` paths, bounds PAK sizes before
  allocating, and lets PAKs override loose files as C does
- pmove numerics follow C (double-precision AngleVectors, reciprocal
  normalize, 0.7 slope test, wrapping snap, runtime air acceleration,
  fly-move early return); PlayerController walks at 200 and runs at 300
- Cvar and command parsing: userinfo injection, numeric prefix parse,
  newline always ends a command, buffer and token limits
- Map change clears configstrings and baselines; game-written
  configstrings reach the server; out-of-range client slots are an error
- Renderer: checked BSP casts, GL objects freed on reload and shader
  errors, C pitch sign and submodel bounds; `calc_fov` rejects fov outside
  1..179
- `make test-browser` ran `cd tests/browser` twice and never reached
  Playwright; it now installs locked npm deps and runs the suite
- `q2-pak-repack` reports Brotli write errors (the stream was finished in
  `Drop`, which discards them), writes outputs via temp file + rename,
  removes a stale `.br` when run without `--brotli`, and rejects output
  over the 4 GiB PAK offset limit instead of truncating offsets
- `q2-devserver` honours `br;q=0`, sends `Vary: Accept-Encoding`, streams
  the pak instead of reading it per request, and skips a `.br` older than
  its pak; an invalid `PORT` is an error
- `q2-bundler` fails when wasm-bindgen glue no longer matches its rewrites
  instead of emitting a page that fetches a missing `.wasm`; page errors
  are inserted as text, not HTML
- Undocumented `unsafe` blocks are denied again (a rustflags entry had
  downgraded the workspace lint to a warning)

### Changed
- CI runs `make fmt-check lint check test` with `--locked`, clippy on all
  host targets and on wasm32 libraries, `cargo audit`, and the browser
  suite; the toolchain comes from `rust-toolchain.toml` (1.97.1)
- `make pak-web` is a file rule and only reruns when `pak0.pak` or the
  repacker changes; `make play` no longer requires 7z/curl/python3 when
  `pak0.pak` exists; demo download is checked with `curl --fail` and SHA-256
- `q2-devserver` no longer sends permissive CORS headers

### Removed
- Unused dependencies: `matchbox_socket` (q2-net), every q2-bin dependency,
  workspace `cpal`/`lewton`; this removed all 19 `cargo audit`
  vulnerabilities
- `q2-pak-repack --all` (never read; keeping every file is the default),
  the unused `PAK_WEB_ALLOW` variable, `make serve`, `make prereq-python3`,
  `make prereq-pak-repack`

## [0.1.2] - 2026-05-01

### Added
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta, packet-entity delta (30 tests) — CP-4 client delta
- `PakReader` trait with lazy WASM backend — avoids copying the full PAK into linear memory until individual assets are requested
- Server frame loop with `GameImport`/`GameExport` bridge replacing C function-pointer tables — CP-3 complete
- CI badge in README
- `q2-pak-repack` crate: CLI tool that repacks a Q2 PAK, optionally filtered
  by an extension allowlist (`make pak-web` keeps every file), and writes a
  Brotli-compressed `.br` variant (level 11) for web delivery
- `make pak-web` target: produces `pak0-web.pak` and `pak0-web.pak.br` from
  the downloaded demo data; integrated into `make play` and `make play-release`
- Devserver Brotli content negotiation: `q2-devserver` now serves
  `pak0-web.pak.br` when the client sends `Accept-Encoding: br`, reducing
  wire size from ~46 MB to ~26 MB

### Changed
- `collision.rs`: major refactor for correctness and clippy compliance
- `gl3/mod.rs`: renderer improvements
- `game_iface.rs`: expanded `GameImport` bridge implementation
- `player_ctrl.rs` / `pmove.rs`: player movement refinements
- README updated to reflect CP-3 completion and CP-4 in-progress status

## [0.1.1] - 2026-03-26

### Added
- Entity spawn system (`q2-game`)
- `GameExport` trait wired to server
- BSP map loading and WebGL2 rendering
- Collision traces passing (CP-1, CP-2)
