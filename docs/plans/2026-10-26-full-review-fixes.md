# Full-codebase review fixes (server-0.1.2)

Objective: address every finding from the 2026-10-26 full review (six
reviewers, ~185 raw findings, deduplicated below). Fix bugs and C-parity
divergences with tests; map roadmap-sized findings to tracking issues.

Raw reports (session scratchpad): review-common.md (C-n), review-sgc.md (S-n),
review-rust.md (R-n), review-arch.md (A-n), review-math.md (M-n),
review-build.md (B-n). Consolidated report: `reviews/full-review-2026-10-26.md`.

Protocol decision: match C protocol 34 on the wire (CLAUDE.md requires C
parity for the network protocol). Encoder and client decoder change together.

## Duplicate map (canonical ID first)

C-1=R-6, C-2=R-5, C-3=R-7, C-5=M-16, C-9=S-2, C-14=R-1, C-15=R-2, C-28=R-18,
C-29=R-17, C-21=M-4, C-22=M-5, C-40=M-7, C-7=M-10=S-27=R-25=A-19(part),
C-8=S-14, C-11=R-41, C-34=R-39, C-17=M-14, C-23=M-1, C-25=M-8=A-2,
S-15=M-9=R-27, S-8=R-20, S-19=R-22, S-9=R-26, M-13=R-23, S-23=R-12,
A-1=R-11, A-6=B-3, A-16=B-15=R-36(part), A-9..11=R-9, A-27=B-16, A-28=B-5,
B-1=R-10(part), B-9=R-33, B-11=R-35, B-12=R-37, B-21=R-36, A-15=B-21(part),
A-14=R-16, A-17=R-40.

## Tasks

- [ ] T0 Gate: `cargo fmt`, clippy `--all-targets -D warnings` green, pin
  toolchain. Accept: fmt --check, clippy all-targets, tests all pass.
- [ ] T1 Network/protocol (netchan.rs, net_msg.rs, q2-client parse.rs,
  input.rs): C-1, C-2, C-3, C-4, C-5, C-6, C-7, C-8, C-9, C-10, C-11, C-12,
  C-13, M-11, M-12, S-1, S-9, S-10, S-11, S-12(skip-not-abort part), S-13,
  S-31, S-15. Tests: runt packet rejected; reliable overflow sets
  fatal_error; usercmd byte order equals C; entity encoding equals C bytes;
  truncated frame does not hang or store valid frame; negative entnum
  rejected; wrong protocol rejected; move scaling 200/400.
- [ ] T2 q2-common core (collision, filesystem, cvar, cmd, zone, binary,
  error, pmove, player_ctrl, PakReader Send+Sync + q2-platform pak.rs
  unsafe): C-14, C-15, C-16, C-17, C-18, C-19, C-20(partial), C-21, C-22,
  C-23/M-1, M-2, M-3, M-6, C-24, C-26, C-27, C-28, C-29, C-30, C-31, C-32,
  C-33, C-34, C-35, C-36, C-37(bounded parts), C-38, C-39, C-40, A-1,
  A-2(speed clamp + doc). Tests per finding as named in reports.
- [ ] T3 Server/game/render/wasm (q2-server, q2-game, q2-render,
  q2-render-api, q2-wasm, q2-platform except pak.rs): S-3, S-4, S-8, S-18,
  S-19, S-20, S-21, S-22(bound), S-23, S-24, S-25, S-26, S-29, M-13, M-15,
  R-3, R-4, R-13, R-14, R-15, R-16, R-28, R-30, R-31, A-3(configstring
  drain), A-22, A-23, A-24, A-25, A-26, GameImport/Export Send+Sync.
- [ ] T4 Build/tooling/deps/docs: B-2..B-28, R-8, R-9, R-32..R-38, A-6..A-16,
  A-27, A-28. Accept: `make -n` for every target sane; CI runs fmt,
  clippy all-targets + wasm32 -D warnings, tests, audit.
- [ ] T5 Integrate: merge T1-T4, full gate, `make wasm`, write report.

## Deferred (roadmap; mapped to existing issues)

S-5/R-24 -> #9, #31; S-6, S-21(spawn reset) -> #11; S-7, A-4, S-17, S-30 ->
#18, #10; S-12(opcode handlers), S-16 -> #19, #6, #7; C-25/M-8/A-2(pmove
rebuild), A-29 -> #12, #19; A-5 -> #20; B-12/R-37(AST approach) -> #2;
A-17 (Q2Error to q2-shared), A-20 (single BSP loader), C-20 (ent identity in
traces) -> proposed new issues, not filed without approval.

## Deviations

(record here)
