# ADR-004: Web Asset Delivery

**Status:** Accepted (Phase 1 of the plan shipped; size targets not met)
**Date:** 2026-05-01 (consolidated 2026-10-26)

## Context

The browser build fetches `pak0.pak` before play starts. The demo pak is about
50 MB and the retail pak about 200 MB. In May 2026 the demo pak broke down as
59% WAV audio, 13% PCX textures, 13% BSP maps, 7% MD2 models and 5% WAL
textures, while the engine read only the BSP maps. Distribution must stay a
single HTML page plus one pak-like file that any static host can serve. This
ADR replaces the PAK compression brief, specification and plan, which were
local working notes (not in the repository).

## Options considered

Brotli precompression alone cuts about 27% and does nothing about unused
assets. Transcoding audio to Opus cuts the most but needs an audio engine
that does not exist yet and an ffmpeg build step. HTTP Range streaming would
fetch only what is read, but needs an async load path through the engine and
a host that honours Range requests. A Service Worker cache helps only repeat
visits.

## Decision

Repack at build time and compress, with room to transcode later.

- **Lazy reads.** The engine keeps the fetched pak in the JS heap behind
  `JsPakReader` (a `PakReader`, ADR-001) and copies a file into WASM memory
  only when it is opened.
- **Repacker.** `q2-pak-repack` writes `pak0-web.pak` from `pak0.pak`,
  byte-identical per file, and a `pak0-web.pak.br` Brotli variant through a
  temp file and rename, reporting write errors. It keeps every file unless
  `--allow` lists extensions.
- **Build.** `make pak-web` is a file rule that reruns only when `pak0.pak`
  or the repacker (including its `Cargo.toml` and `Cargo.lock`)
  changes.
- **Serving.** The dev server sends the `.br` variant with
  `Content-Encoding: br` and `Vary: Accept-Encoding` when the client accepts
  Brotli (honouring `q=0`) and the `.br` is not older than the pak; otherwise
  it serves the uncompressed file.

## Consequences

The demo pak goes from about 50 MB to about 28 MB on the wire. The original
targets (8 MB raw, 3 MB Brotli) assumed filtering out everything except maps,
which was dropped once the full asset set was kept for upcoming renderer and
audio work; meeting them needs WAV-to-Opus and texture transcoding steps in
the repacker when audio and textures land. Revisit Range streaming if the
retail pak becomes the target.
