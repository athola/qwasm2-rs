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
saved by name in save files (see Save and load).

### Map fixes

yquake2 ships fixed entity files for 12 retail maps (`stuff/mapfixes`),
which C's `CMod_LoadEntityString` loads in place of a map's own entities
when `sv_entfile` is set, its default. The port embeds them in `q2-server`
(`mapfixes`), so every platform loads them; `sv_entfile 0` turns them
off. They fix monster counts, spawn chains, deathmatch items in single
player, and remove biggun's `target_actor`, for which yquake2 has no code.
An `.ent` file in a pak is not read: only the shipped fixes apply.

### Save and load

The game writes its state as bytes and the server stores them, as C's
`WriteGame`/`ReadGame` and `WriteLevel`/`ReadLevel` do with files
(`savegame` module, `GameExport` save calls). The saved types derive serde
and are encoded with postcard behind a magic, version and kind byte; C's
files are raw struct dumps tied to one build, so there is no format to be
compatible with. A field added to a saved type fails to compile until it is
saved or skipped.

Callbacks and monster moves are saved by name. `build.rs` generates the
name tables from the sources, as C generates `gamefunc_list.h`: every
function of a callback signature that the crate uses as a value, and every
`MonsterMove` static. A copied `currentmove` is named by its frames static,
range and end function; C's identical insane crawl and runcrawl moves share
that name and load back the same animation. A callback missing from the
tables is a save error naming the entity and field.

A level also keeps the game's file statics C does not save (trail, enemy
check, quad and pain counters), so a loaded level runs on exactly as it
would have; `q2-server/tests/savegame.rs` pins that against an
uninterrupted run. Slotmap keys are saved as they are, so references stay
valid; free entities come back zeroed, as C's ReadLevel leaves them. A
loaded player takes the body waiting in the level (C ClientConnect and
ClientBegin); the port spawns bodies in `connect_client`, so a flag set at
connect tells begin which body came from the save.

The server half (`q2-server` `local/save.rs`) keeps C's `save/<slot>/`
files in `SaveFiles`: `server.ssv`, `game.ssv`, and each visited level's
`.sv2` (configstrings and area portals) and `.sav`. A unit's levels keep
their state between visits, a starred level wipes the unit, entering a
level writes the `save0` autosave, and `save`, `load` and `menu_loadgame`
are server commands (F6, F9 and F3 in the browser). The browser keeps the
files in IndexedDB through a small store in the page template
(`q2-bundler`, `window.qwasm2Saves`) rather than web-sys bindings in
`q2-platform`: the engine hands it each changed file and goes on. The
store is shared by every pak served from one origin, so the platform
tells `LocalGame` which maps its pak holds and a save of another pak's
map is refused.

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
anti-cheat. Deferred with an issue: `SV_Push` rollback (#17).

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
- **No cvars for the game rules.** Skill, deathmatch, coop, dmflags,
  fraglimit, timelimit and sv_maplist are fields with no cvar path, so
  the deathmatch rules never end a browser match; other cvars the game
  reads, such as `cl_gun` and `g_commanderbody_nogod`, go through
  `GameImport::cvar`, which `LocalGame::set_cvar` fills. Player footsteps play
  (`g_footsteps` 1); monster footsteps read `g_monsterfootsteps`, which
  defaults to 0 as in C, so they are silent unless it is set. The
  server still latches `skill`, `coop` and `deathmatch` into a save and
  refuses to save in deathmatch from its own cvar, as C's server does,
  though the game reads its rule fields instead.
- **Sound routing.** One local client hears every sound (no PHS or
  reliable routing); `timeofs` is a start delay. Music, OGG and the
  underwater filter are not ported.
- **Client state during connect and begin.** The server import learns
  the client slots' states after `ClientConnect` and `ClientBegin`, not
  before `ClientBegin` as C sets `cs_spawned`, so an unreliable
  multicast from `ClientBegin` is dropped. Only `maxclients > 1` sends
  one (the login flash), and the browser runs one client.
- **Weapons.** Userinfo `hand` (the page's `?hand=`: 0 right, 1 left,
  2 center) moves shots (`P_ProjectSource`), the gun (mirrored or not
  drawn) and heat beams; `aimfix`, `g_swap_speed` and
  `coop_pickup_weapons` are at their yquake2 defaults. `CheckNeedPass`
  has nothing to do (it publishes password cvars, and there are none);
  the player model's animation (`G_SetClientFrame`, seen only by other
  players) is not run, though the weapon code sets its frames (#32).
- **Client effects.** Temp entities, entity events, trails and flash
  smoke are drawn by the browser client (`q2_client::fx`, C
  `cl_tempentities.c`, `cl_particles.c`, `cl_effects.c`) through the
  renderer's particles, beams and sprites, mission-pack ones (rogue,
  xatrix, CTF: `q2_client::newfx`, sustains, player beams, lightning)
  included; color shells and power screens draw as C's color-only
  models; their lights share the client's 32 dlights as in C. The
  rogue shell combinations apply with C's `game` set to `rogue`
  (`?game=rogue`), which only the client reads: no rogue game code or
  data is loaded. Brush models moved by doors take dlights at their
  rest position.
- **Page settings.** The page starts on `?map=` (else demo1), and
  `?vid_fullscreen=1` goes fullscreen with the keyboard locked to the
  game where the browser allows it (Chromium), so Ctrl shortcuts do not
  reach the browser.
- **Not ported:** cinematic and demo playback (`.cin`/`.dm2` levels are
  refused), the save menu (F2 `menu_savegame`; ADR-005 covers F3), the inventory layout and its
  refresh (`InventoryMessage`), the `fov`/`spectator` userinfo and `ClientUserinfoChanged`'s `Info_Validate` check, the
  spectator and chase-camera stats, `SP_FixCoopSpots`, falling damage
  (`P_FallingDamage`), the dead view turning to the killer
  (`LookAtKiller`), and coop's per-cube tracking (`pers.power_cubes`), so
  a coop power cube counts like any key.
- **Effects left out on purpose:** `CL_QuadTrail`,
  `CL_Tracker_Explode` and `CL_GenericParticleEffect` have no caller in yquake2 or id's 3.21 release,
  so drawing them would change what valid input looks like. id's
  `TE_TRACKER_EXPLOSION` has the `CL_Tracker_Explode` call commented out
  in favor of the color flash and `CL_ColorExplosionParticles` drawn
  here, and `CL_QuadTrail` is `CL_FlagTrail` with the blue flag's color
  115, which quad carriers never get (they get the blue shell).
- **Opt-in additions (ADR-005), off by default:** `?cl_fxextra=1` gives
  C's otherwise unused `CL_SmokeTrail` a caller: rocket, grenade and
  `TE_EXPLOSION1`/`TE_EXPLOSION2` blasts out of water leave a 64-unit
  column of gray smoke (palette 4..11, every 4 units) rising from the
  blast. C has no such effect; the trigger is this port's invention.

## Consequences

Callbacks taking `&mut GameWorld` keep game code safe Rust without
`RefCell` webs, at the price of entity lookups by key in every callback.
The engine callbacks are an `Rc<dyn GameImport>` held from construction,
so a pmove trace closure can hold its own handle while game code borrows
`&mut self`; the server shares the same `Rc<ServerGameImport>`.
