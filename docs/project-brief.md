# PAK Compression Pipeline — Project Brief

**Date**: 2026-05-01  
**Author**: athola  
**Status**: Draft

## Problem Statement

**Who**: Players and distributors of qwasm2-rs  
**What**: The Quake 2 demo PAK is 48 MB; the full retail PAK is ~200 MB. Both must be
downloaded in full before gameplay starts.  
**Where**: Static web hosting / GitHub Pages / any CDN  
**When**: Every first-visit user session  
**Why**: A 48 MB blocking download is unacceptable for a web game distributed via a
single HTML + asset pair. The full retail PAK compounds this to ~200 MB.  
**Current State**: `pak0.pak` is served verbatim by `q2-devserver` at
`/gamedata/baseq2/pak0.pak`. The WASM engine (1.5 MB HTML) fetches it via a JS
`fetch()` call and holds it in JS heap via `JsPakReader`. No compression or streaming
is applied.

## PAK Content Breakdown (demo pak, 48 MB)

| Type | Files | Size | % of total |
|------|-------|------|-----------|
| WAV (PCM audio) | 375 | 28.4 MB | 59% |
| PCX (palette textures) | 300 | 6.3 MB | 13% |
| BSP (maps) | 3 | 6.1 MB | 13% |
| MD2 (models) | 104 | 3.2 MB | 7% |
| WAL (wall textures) | 303 | 2.2 MB | 5% |
| Other | 21 | 0.8 MB | 2% |

Audio is the dominant target: replacing PCM WAV with Opus reduces it 8–12×.

## Current Engine Asset Usage (2026-05-01)

**Critical finding**: the WASM engine fetches the full 48 MB pak but currently reads
only BSP maps from it. All other asset types are unused at runtime:

| Asset type | Engine status | Downloaded | Actually read |
|-----------|---------------|-----------|--------------|
| BSP maps | ✅ Loaded + rendered | 6.1 MB | ~2 MB (1 map) |
| WAL textures | ❌ Not loaded (renderer uses names only) | 2.2 MB | 0 |
| PCX textures | ❌ Not loaded | 6.3 MB | 0 |
| MD2 models | ❌ Not loaded | 3.2 MB | 0 |
| WAV audio | ❌ Stubbed (`sound` is no-op) | 28.4 MB | 0 |

The full 48 MB traverses the network; only ~2 MB is ever used. This makes the
compression problem acute but also makes the near-term fix simple.

## Goals

1. **Near-term**: Reduce download to only what the engine currently reads (BSP maps
   + configs). Target ≤ 8 MB.
2. **Medium-term**: As audio/textures/models land in the engine, add a transcoding
   pipeline (WAV→Opus, WAL→WebP) to the repack tool so each feature arrives
   already web-optimized.
3. Zero manual per-asset curation — pipeline is fully automated.
4. No change to distribution shape (single HTML + one PAK-like file).
5. Full game remains achievable: the solution must scale to the retail pak (~200 MB).

## Constraints

### Technical
- Engine reads from `PakReader` trait — any solution must plug into this boundary.
- WASM browser environment: no native audio codec libraries, must use Web Audio API.
- The `JsPakReader` keeps the full asset file in JS heap — streaming per-asset (Range
  requests) is an alternative but requires server cooperation and many round trips.
- PAK format is simple (offset + length table): trivially repacked.
- The Rust asset pipeline runs at build time (native); the browser sees the output.

### Resources
- **Timeline**: Single sprint (1–2 weeks).
- **Team**: Solo.
- **Budget**: N/A — OSS tooling only.

### Integration
- Must continue to work with `make bundle` / `make play` workflow.
- Makefile gains a `make pak-web` step that produces `gamedata/baseq2/pak0-web.pak`.
- `q2-wasm` `start_game` function receives the pak URL; updating the URL is the only
  JS change needed.

### Success Criteria
- [ ] Demo PAK ≤ 15 MB on disk.
- [ ] All 3 maps load and are playable.
- [ ] All in-game sounds play correctly.
- [ ] `make pak-web` is idempotent and completes in < 60 s on a modern machine.
- [ ] Existing `cargo test --workspace` passes.
- [ ] Browser Playwright smoke test passes.

## Approach Comparison

### Approach A: Brotli precompression only ⭐ baseline

**Description**: Serve `pak0.pak.br` (brotli-compressed) with `Content-Encoding: br`.
Browser decompresses transparently.

**Pros**:
- Zero engine changes.
- Zero asset format changes.

**Cons**:
- WAV PCM is not very compressible; expect 20–30% reduction (~35 MB).
- Requires server to send correct Content-Encoding headers (devserver change).
- Doesn't scale: full retail PAK still ~150 MB compressed.

**Effort**: XS  
**Estimated result**: 48 MB → ~35 MB (27% reduction)

---

### Approach B: Audio transcoding pipeline (WAV → Opus) ⭐ recommended

**Description**: Build a `q2-pak-repack` native CLI crate that reads `pak0.pak`,
transcodes all WAV files to Opus (via `ffmpeg`/`opusenc` subprocess), and writes a
new `pak0-web.pak` containing Opus audio + original non-audio assets. Engine's
`load_file()` for `sound/*.wav` is patched to also try `sound/*.opus`. On WASM, audio
data is decoded via the Web Audio API (`AudioContext.decodeAudioData`).

**Stack**: Rust CLI tool (build time) + ffmpeg/opusenc subprocess + Web Audio API
(runtime).

**Pros**:
- Dominant cost (59%) reduced 8–12×: 28 MB WAV → ~3 MB Opus.
- Fully automated pipeline — no manual asset work.
- Browser Opus support: 100% of modern browsers (Chrome 35+, Firefox 26+, Safari 16.4+).
- PAK format unchanged: still a valid PAK file (just with `.opus` entries).
- `JsPakReader` works without modification — still slices bytes on demand.

**Cons**:
- Requires `ffmpeg` or `opusenc` at build time (developer dependency).
- Engine must dispatch audio to Web Audio API rather than a Q2-native PCM mixer.
- Needs a thin Rust/WASM audio layer: `load_file("sound/x.wav")` → find `.opus` →
  pass bytes to Web Audio API `decodeAudioData`.
- `AudioContext.decodeAudioData` is async; requires minor async plumbing on first sound
  load (subsequent loads cached in JS).

**Risks**:
- (Low) Safari Opus support only from v16.4 (2022); iOS 16.4+ covers ~85% of iOS fleet.
- (Low) Build-time dependency on `ffmpeg` / `opusenc`; Makefile prerequisite check.
- (Medium) Audio sync: Opus frames have 20 ms granularity; sub-frame Q2 sounds may
  have imperceptible timing drift. Acceptable for a game engine port.

**Effort**: M (3–5 days)  
**Estimated result**: 48 MB → ~18 MB (62% reduction, demo); ~200 MB → ~75 MB (full game)

---

### Approach C: HTTP Range requests (streaming PAK)

**Description**: Serve PAK with HTTP Range support; engine fetches only needed bytes
per-asset on demand, loading the directory index first (~70 KB) then individual assets
as needed.

**Pros**:
- Zero download before gameplay — only loads what's needed.
- No asset transcoding needed.

**Cons**:
- Many round trips: a single map load triggers 100+ range requests (textures, sounds,
  models).
- Requires an actual HTTP server with Range header support for deployment (not static-
  file friendly: GitHub Pages doesn't support custom range semantics easily).
- Latency spikes on first map load.
- Significant engine changes: `JsPakReader` must become async, `FileSystem::load_file`
  must become async, cascading async changes through the entire engine.

**Effort**: XL  
**Estimated result**: Eliminates initial download but at cost of per-asset latency.

---

### Approach D: Service Worker asset cache

**Description**: Register a Service Worker that intercepts the PAK fetch and caches
it in the Cache API. Second visit is instant; first visit unchanged.

**Pros**: Zero engine changes; second-visit download eliminated.

**Cons**: Doesn't solve the first-visit 48 MB download — the core problem.

**Effort**: S  
**Estimated result**: No first-visit improvement.

---

### Comparison Matrix

| Criterion | A: Brotli | B: Opus transcode | C: Range streaming | D: Service Worker |
|-----------|-----------|-------------------|--------------------|-------------------|
| Download reduction | 🟡 27% | 🟢 62% | 🟢 ~95% initial | 🔴 0% first visit |
| Engine changes | 🟢 None | 🟡 Audio layer | 🔴 Async cascade | 🟢 None |
| Build complexity | 🟢 Trivial | 🟡 ffmpeg dep | 🟢 None | 🟢 None |
| Static-host friendly | 🟢 Yes | 🟢 Yes | 🔴 Needs Range | 🟢 Yes |
| Full-game scalability | 🔴 Poor | 🟢 Good | 🟢 Good | 🔴 None |
| Risk level | 🟢 Low | 🟡 Medium | 🔴 High | 🟢 Low |

## Selected Approach: Phased minimal pak + extensible transcode pipeline ⭐

### Phase 1 (this sprint): Filtered pak + Brotli

**Validated empirically**: a test filter to `bsp,cfg,lst,pk,map` produces a 6.2 MB
pak with byte-identical BSP content (MD5-verified). Adding Brotli compression reduces
that to **2.1 MB** — a 96% total reduction from 47.6 MB.

Build `q2-pak-repack` (native Rust CLI crate) that reads the source PAK and emits
`pak0-web.pak` filtered by an extension allowlist. Zero engine changes.

The repack tool takes a `--allow` flag. Adding a new extension (e.g., `wal` when the
renderer loads textures) is a one-line Makefile change.

Optional (same sprint): devserver serves `pak0-web.pak.br` (pre-brotli-compressed)
with `Content-Encoding: br` if the `.br` file exists. Browser decompresses
transparently. On static hosts without Content-Encoding support, the 6.2 MB
uncompressed file is served as fallback.

**Validated result**: 47.6 MB → 2.1 MB (96% reduction) or 6.2 MB without Brotli

### Phase 2 (future, when audio lands): Opus transcode step

Extend `q2-pak-repack` with `--transcode-audio` flag. WAV entries are piped through
`ffmpeg -c:a libopus`; output entries use `.opus` extension. Engine audio layer reads
`.opus` and dispatches to Web Audio API. This reduces audio from ~28 MB to ~3 MB.

### Comparison Matrix (updated)

| Criterion | A: Brotli | B-phase1: BSP-only pak | B-phase2: +Opus | C: Range stream |
|-----------|-----------|------------------------|-----------------|-----------------|
| Near-term reduction | 🟡 27% | 🟢 86% | 🟢 95%+ | 🟢 ~100% |
| Engine changes needed now | 🟢 None | 🟢 None | 🟡 Audio layer | 🔴 Async cascade |
| Build complexity | 🟢 Trivial | 🟡 New crate | 🟡 +ffmpeg dep | 🔴 Major |
| Static-host friendly | 🟢 Yes | 🟢 Yes | 🟢 Yes | 🔴 No |
| Extensible as engine grows | 🔴 No | 🟢 Allowlist flag | 🟢 Built-in | N/A |

### Trade-offs Accepted
- **New build-time crate**: `q2-pak-repack` is ~200 LOC; low maintenance burden.
- **Pak URL changes**: `pak0.pak` → `pak0-web.pak`. One-line change in `qwasm2.html`
  `generate_html()` and devserver. Makefile target `pak-web` gates on this.
- **Future: ffmpeg at build time** (Phase 2 only): same category as existing `7z` dep.

### Rejected Approaches
- **C (Range streaming)**: XL async cascade; incompatible with static hosting.
- **D (Service Worker)**: Solves repeat-visit only.
- **A alone (Brotli)**: Only 27% reduction; doesn't target the actual problem.
- **Full Opus transcode now**: Audio is unimplemented; premature optimization.

## Next Steps

1. `/attune:specify` — Define `q2-pak-repack` CLI contract, allowlist schema, and
   Makefile integration.
2. `/attune:blueprint` — Task breakdown: repacker crate, `make pak-web` target,
   devserver URL update, HTML URL update.
3. `/attune:execute` — Implement.
