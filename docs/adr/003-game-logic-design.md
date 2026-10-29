# ADR-003: Game Logic Design (q2-game)

**Status:** Accepted
**Date:** 2026-04-10 (consolidated 2026-10-26)

## Context

The C game DLL (`~/Qwasm2/src/game/`, about 22,000 lines) holds entity
management, physics, combat, items, weapons, AI, monsters, triggers, the
player subsystem and save/load. It reaches engine services through the
`game_import_t` function-pointer table and keeps its state in globals
(`g_edicts`, `level`, `game`). This ADR replaces the Phase 2 specification
and implementation plan, which were local working notes (not in the
repository).

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
anti-cheat. Deferred with issues: save/load (#15) and `SV_Push` rollback
(#17).

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

Classnames live in one static table, `spawn::SPAWNS`, in C `spawns[]`
order: each row names the spawn function, the C model path for monsters
and misc objects, and an optional finish step. Spawn functions see only
`EntityStorage`, so the finish step and `spawn_entities` do the parts of C
`SP_*` functions that need the import: `gi.setmodel` for every `*n` model
before the spawn function (brush entities get their inline-model bounds),
worldspawn's configstrings (status bar, sky, item names, light styles),
`SP_light`, a monster's `item` key, and a link for every entity
afterwards. Map items go through `SpawnItem` ahead of the table as in
`ED_CallSpawn`. Doors, rotating and secret doors, water, plats, buttons,
trains, elevators and conveyors follow C's `Move_Calc`/`AngleMove_Calc`
movers.

Client records outlive the map. `spawn_entities` runs C `SaveClientData`
before it clears the entities; the records stay in `GameWorld::clients`,
and `ClientBegin` keeps `pers`, runs `InitClientPersistant` only for a
dead player, then `FetchClientEntData`. A level exit queues `gamemap`; `LocalGame` parses it
like `SV_Map` (`map$spawnpoint`, `*`, `+`), loads the next map into the
same game and begins the client again, and the browser re-registers
models and the sky in `CL_PrepRefresh` order. Monster moves are C
`mframe_t` tables: each frame has its own AI function, distance and think
function.

### Monsters, sound and areas (2026-10-28)

All 20 monsters are ported from their `m_*.c` files: every `mframe_t`
table frame by frame, every callback (sight, attack, melee, dodge, pain,
die) and their sounds, which a `SPAWNS` finish step indexes because spawn
functions see only `EntityStorage`. They run on the full `g_ai.c` and
`g_monster.c` (C's `enemy_vis`/`enemy_range` statics are `GameWorld`
fields), fire through `monster_fire_*` with the `m_flash.c` offsets, and
gib through `ThrowGib`/`ThrowHead`. Players make `PlayerNoise` when they
fire, so monsters hear them, and the `light_level` dark check applies.

Client records live in `GameWorld::clients` for the whole game; an
entity holds its slot, as C's `ent->client` points into `game.clients`.
The coop and deathmatch branches of persistence, spawning, spawn-point
selection and drops are ported; `deathmatch`, `coop` and `dmflags` are
game-state fields (no cvar path), so the browser plays single player.

Mover sounds, door messages and area portals are ported. Doors open and
close portals through `GameImport::set_area_portal_state`; the server
links entities with their areas, drops entities in unconnected areas from
the client frame, and sends the client's area bits, which the renderer
uses to skip the world behind a shut door. The game's sounds reach the
browser through a server event queue and a WebAudio backend. Picture
levels (`victory.pcx`) show the picture as C's `ss_pic` does.

### Player weapons and powerups (2026-10-28)

The player's weapons are `p_weapon.c`: `Think_Weapon`, `Weapon_Generic`
and every `Weapon_*`, run from `ClientThink` on a fresh attack press and
from `ClientBeginServerFrame` otherwise. Weapons switch through
`ClientCommand` (`use`, `weapnext`, `invuse`, ...), whose arguments come
from `GameImport::argv`/`args` as in C. The client draws the gun
(`CL_AddViewWeapon`) and plays muzzle flashes: in C the gunfire sounds of
players and monsters come from the client's `CL_AddMuzzleFlash`/`2`, so
the server hands its multicast messages to `LocalGame` and the client
ports those functions and the dlight list; lit world faces add GL3's
dynamic light term. Powerups and power armor are usable and droppable,
`P_WorldEffects`, `SV_CalcBlend` and `G_SetClientEffects` give them their
effects, and the deathmatch rules end matches through C's intermission.

### Known deviations from C (2026-10-28)

- **Player trail nodes are data.** C keeps the eight trail nodes as
  `player_trail` edicts; here they are plain data on `GameWorld`, so they
  take no entity numbers. Monsters follow them as C's `ai_run` does.
- **No cvars.** Skill, deathmatch, coop and dmflags are fields;
  `g_monsterfootsteps` stays off (its C default), so footsteps are silent.
- **Sound routing.** One local client hears every sound (no PHS or
  reliable routing); `timeofs` is a start delay. Music, OGG and the
  underwater filter are not ported.
- **Weapons.** Right-handed only (no userinfo `hand`), `aimfix`,
  `g_swap_speed` and `coop_pickup_weapons` at their yquake2 defaults.
  `CheckNeedPass` has nothing to do (it publishes password cvars, and
  there are none); items register their models and sounds when first
  used rather than through `PrecacheItem`; the player model's animation
  (`G_SetClientFrame`, seen only by other players) is not run, though
  the weapon code sets its frames. The game writes no temp entities
  for impacts, explosions or trails, and the renderer draws no
  particles, so those are invisible; monster flash particles and smoke
  likewise. Brush models moved by doors take dlights at their rest
  position.
- **Not ported:** cinematic and demo playback (`.cin`/`.dm2` levels are
  refused), savegames (#15; a dead single player's press sends
  `menu_loadgame`, which nothing handles), `P_DamageFeedback` (no damage
  flash or pain view kick), the scoreboard, obituaries (no
  `MOD_FRIENDLY_FIRE` bit), the random pick in an item team's respawn,
  `SP_FixCoopSpots`, and power-armor sparks.

## Consequences

Callbacks taking `&mut GameWorld` keep game code safe Rust without
`RefCell` webs, at the price of entity lookups by key in every callback.
The engine callbacks are an `Rc<dyn GameImport>` held from construction,
so a pmove trace closure can hold its own handle while game code borrows
`&mut self`; the server shares the same `Rc<ServerGameImport>`.
