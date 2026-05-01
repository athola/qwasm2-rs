# Changelog

All notable changes to qwasm2-rs are documented here.

## [0.1.2] - 2026-04-30

### Added
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta, packet-entity delta (30 tests) — CP-4 client delta
- `PakReader` trait with lazy WASM backend — avoids copying the full PAK into linear memory until individual assets are requested
- Server frame loop with `GameImport`/`GameExport` bridge replacing C function-pointer tables — CP-3 complete
- CI badge in README

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
