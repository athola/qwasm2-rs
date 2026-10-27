# ADR-003: Game Logic Design (q2-game)

**Status:** Accepted
**Date:** 2026-04-10 (consolidated 2026-10-26)

## Context

The C game DLL (`~/Qwasm2/src/game/`, about 22,000 lines) holds entity
management, physics, combat, items, weapons, AI, monsters, triggers, the
player subsystem and save/load. It reaches engine services through the
`game_import_t` function-pointer table and keeps its state in globals
(`g_edicts`, `level`, `game`). This ADR replaces the Phase 2 specification
and implementation plan that lived in `docs/specification.md` and
`docs/implementation-plan.md`.

## Decision

### GameWorld owns the state

`GameWorld` replaces the C globals. It owns the entity storage, level and
game locals, the item table, the spawn table, and the engine callbacks
(`gi: Rc<dyn GameImport>`). Game code receives `&mut GameWorld` plus an
`EntityKey`, never `&mut Entity`, which keeps the borrow checker satisfied
when one entity's callback touches another.

### Callbacks are plain function pointers

`think`, `touch`, `use`, `pain`, `die` and `blocked` are `fn` pointers such
as `fn(&mut GameWorld, EntityKey)`, not closures. They carry no captured
state, so they can be stored on entities without lifetime problems and can be
saved by name when save/load lands (#15).

### Entities

Entities live in a SlotMap (ADR-001). `Entity::in_use` is kept alongside slot
presence because `G_FreeEdict` semantics clear `inuse` while the slot stays
allocated, and the game iterates and searches only live entities.

### Fidelity

Gameplay constants (frame time, step size, gravity, damage, item counts,
monster health and pain thresholds) are copied from the C source, and tests
pin damage math, physics steps and AI transitions. q2-game has no unsafe code,
no global mutable state, and must compile for `wasm32-unknown-unknown`; all
engine I/O goes through `GameImport`.

### Scope

In: the base Quake II entity set (107 spawn types), 11 weapons, the item
table, 10 movement types, the base AI, 20 monster types, triggers, targets,
functional entities, and the player subsystem. Out: mission packs, CTF, and
anti-cheat. Deferred with issues: full monster attack and pain animations
(#14), save/load (#15), the player weapon state machine and deathmatch rules
(#16), and `SV_Push` rollback (#17).

## Current state (2026-10-27)

`GameWorld` implements `GameExport` and is the only game: q2-server drives
it, and the browser runs it in-process through `q2_server::local::LocalGame`.
Entities carry C edict numbers (0 the world, `1..=maxclients` the clients,
`G_Spawn`'s reuse rule for the rest) so `GameImport` calls and trace results
name entities the way C does. The game passes the import `EntityLink`s
(`SV_LinkEdict`'s inputs) and the server clips traces against them.
`ClientThink` runs pmove; spawning follows `ED_ParseEdict`,
`SelectSpawnPoint` and `PutClientInServer`, and its tail fires
`G_TouchTriggers` and pmove's touch list.

Spawn functions see only `EntityStorage`, so `spawn_entities` does the
parts of C `SP_*` functions that need the import: `gi.setmodel` for every
`*n` model before the spawn function (brush entities get their
inline-model bounds), the C model path for monsters and misc objects from
one classname table, worldspawn's configstrings (status bar, sky, item
names, light styles), `SP_light`, and a link for every entity afterwards.
Map items go through `SpawnItem` as in `ED_CallSpawn`. Doors, plats and
buttons follow C's `Move_Calc` movers; `func_door_rotating` and
`func_door_secret` are still static brushes. Remaining per-type spawn
details are tracked in #11, #14 and #16.

## Consequences

Callbacks taking `&mut GameWorld` keep game code safe Rust without
`RefCell` webs, at the price of entity lookups by key in every callback.
The engine callbacks are an `Rc<dyn GameImport>` held from construction,
so a pmove trace closure can hold its own handle while game code borrows
`&mut self`; the server shares the same `Rc<ServerGameImport>`.
