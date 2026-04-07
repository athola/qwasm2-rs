Cross-reference the current Rust implementation against the original C Quake 2 source at ~/Qwasm2.

Given a module or function name as $ARGUMENTS:

1. Find the corresponding Rust implementation in this repo (crates/)
2. Find the original C implementation in ~/Qwasm2/src/
3. Compare them side-by-side, checking:
   - Are all constants identical (physics values, protocol numbers, content flags)?
   - Are algorithmic steps in the same order?
   - Are edge cases handled the same way?
   - Are there any Rust-specific deviations that are intentional vs accidental?
4. Report: MATCH (faithful port), DIVERGENCE (intentional difference with reason), or DRIFT (unintentional mismatch that needs fixing)

If no arguments given, ask which module to cross-reference.
