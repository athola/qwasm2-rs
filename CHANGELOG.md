# Changelog

All notable changes to qwasm2-rs are documented here.

## [0.1.2] - 2026-05-01

### Added
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta, packet-entity delta (30 tests) — CP-4 client delta
- `PakReader` trait with lazy WASM backend — avoids copying the full PAK into linear memory until individual assets are requested
- Server frame loop with `GameImport`/`GameExport` bridge replacing C function-pointer tables — CP-3 complete
- CI badge in README
- `q2-pak-repack` crate: CLI tool that filters a Q2 PAK by extension allowlist
  and writes a Brotli-compressed `.br` variant (level 11) for web delivery
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
