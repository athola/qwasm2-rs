Run a quick architecture health check on the qwasm2-rs workspace.

1. Verify no circular dependencies: check all Cargo.toml files for cross-crate deps
2. Count unsafe blocks and verify all have SAFETY comments
3. Check that q2-wasm doesn't contain game logic (physics should be in q2-common/player_ctrl)
4. Verify trait boundaries are clean (Renderer, GameImport, GameExport)
5. Run `cargo clippy --workspace --exclude q2-wasm` and report any errors
6. Run `cargo test --workspace --exclude q2-wasm` and report results
7. Check crate sizes (warn if any crate > 10k LOC)

Report as: HEALTHY / WARNINGS / ISSUES with details.
