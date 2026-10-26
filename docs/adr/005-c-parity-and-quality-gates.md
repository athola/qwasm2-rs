# ADR-005: C Parity Rules and Quality Gates

**Status:** Accepted
**Date:** 2026-10-26

## Context

Client prediction only works if client and server compute the same movement
bit for bit, and a Rust client must be able to talk to the C protocol. A
full-codebase review on 2026-10-26 (six independent passes, about 185 raw
findings) found the wire format claiming protocol 34 while encoding entities
and user commands differently, pmove numerics drifting from C's float and
double mix, and CI going red on a toolchain update with no code change. This
ADR records the rules that came out of that review. It replaces the review
reports in `reviews/` and the fix plan in `docs/plans/`.

## Decision

### Match C, and say so when we do not

The network protocol is C protocol 34, byte for byte; encoder and decoder
change together, and tests pin bytes derived from `movemsg.c`. Physics and
collision follow C's precision: trigonometry and normalisation in double
where C promotes, comparisons against double constants where C compares a
float to a double literal, and shorts that wrap rather than saturate.
Behaviour that is deliberately stricter than C is allowed when it only
rejects input C would mis-handle, and is written down where it lives. Today
that covers BSP node children having a higher index than their parent (rules
out cycles; the retail demo maps pass), entity numbers in a frame being
strictly ascending, and area queries rejecting negative or `== numareas`
indices that C would read past.

### Untrusted input never panics

Network messages, pak files and BSP files are untrusted. Their parsers check
lengths with overflow-safe arithmetic (wasm32 has a 32-bit `usize`), return
`Q2Error::Drop` the way C raises `ERR_DROP`, and validate cross-references
before traversal. wasm32 builds abort on panic, so a panic in these paths
would kill the whole page.

### Quality gates

The toolchain is pinned in `rust-toolchain.toml`. `make ci` runs
`cargo fmt --check`, clippy with `--all-targets -D warnings` on the host and
on the wasm32 libraries, `cargo check`, and the tests, all with `--locked`;
CI adds `cargo audit` and the Playwright suite. Tests come first for a fix,
and a test that encoded non-C behaviour is changed only together with the
code and with the reason in the commit message.

## Consequences

Parity work is measurable: a byte or bit difference is a failing test rather
than a judgement call. The cost is that some cleanups C would not do are off
the table without an ADR note, and toolchain upgrades are explicit changes
that can bring their own lint fixes. Findings that were too large for the
review pass are tracked as issues: entity numbering and SlotMap world links
(#9, #31), map loading and spawn reset (#11), a complete `GameImport` (#18,
#10), client opcodes, prediction and stufftext (#19, #6, #7), moving the
browser player onto pmove (#12), and loading the world through the Renderer
trait (#20).
