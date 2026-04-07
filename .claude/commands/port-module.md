Port a C module from ~/Qwasm2 to the Rust codebase.

Given a C source file path or module name as $ARGUMENTS:

1. Read the original C source from ~/Qwasm2/src/
2. Identify the target Rust crate based on the C file's directory:
   - common/ → q2-common
   - game/ → q2-game
   - server/ → q2-server
   - client/ → q2-client
   - backends/ → q2-render or q2-platform
3. Port the C code to idiomatic Rust:
   - Replace pointer arithmetic with slices/iterators
   - Replace malloc/free with Vec/Box
   - Replace global state with struct fields
   - Replace function-pointer tables with trait methods
   - Keep physics/protocol constants EXACTLY matching the C values
   - Add `// Ported from: <c-file>:<line>` comments for traceability
4. Write tests that verify the Rust implementation matches C behavior
5. Run `cargo test` and `cargo clippy` to verify

IMPORTANT: Numerical precision matters. Physics constants and fixed-point math MUST match C bit-for-bit for client/server prediction sync.
