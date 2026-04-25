# Phase 2: Game Logic — Specification v0.1.0

**Author**: Claude (attune:project-specification)
**Date**: 2026-04-10
**Status**: Draft
**Branch**: `game-logic-0.1.1`
**C Source Reference**: `~/Qwasm2/src/game/` (58,871 lines, 74 files)

## Change History

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 0.1.0 | 2026-04-10 | Claude | Initial draft from C source cross-reference |

---

## Overview

**Purpose**: Port the Quake 2 game DLL (`src/game/`) to idiomatic Rust in the
`q2-game` crate, replacing ~22,000 lines of C with safe Rust while preserving
exact gameplay behavior. The game DLL handles entity management, combat,
weapons, items, physics, AI, triggers, functional entities, player logic, and
save/load — everything that makes Q2 a playable game rather than an engine
shell.

**Scope**:
- **IN**: All game logic from `src/game/` — entity callbacks, spawn table
  (107 types), combat, weapons (11), items (31), physics (10 movement types),
  AI (20 functions), monsters (20 types), triggers (11), targets (17),
  functional entities (16), player subsystem (client, weapon, view, HUD,
  trail), save/load via serde, and the `GameExport` trait implementation.
- **OUT**: Server frame loop (Phase 3), client parsing (Phase 4), renderer
  (Phase 5), WASM platform (Phase 6), networking (Phase 7). Monster animation
  frame data (art assets, not code). GL1/soft renderer game interactions.

**Stakeholders**:
- Engine integration: q2-server consumes `GameExport` to run game frames
- Gameplay fidelity: behavior must match original C for all entity interactions

---

## Functional Requirements

### FR-001: Entity Callback System

**Description**: Extend the existing `Entity` struct with 7 callback function
fields (prethink, think, blocked, touch, use, pain, die) matching the C
`edict_t` callback signatures. Callbacks are invoked by the physics and combat
systems during `G_RunFrame`.

**C Reference**: `local.h:1043-1051`

**Callback Signatures (Rust equivalents)**:
```
prethink(self_key)
think(self_key)
blocked(self_key, other_key)
touch(self_key, other_key, plane, surface)
use(self_key, other_key, activator_key)
pain(self_key, other_key, kick: f32, damage: i32)
die(self_key, inflictor_key, attacker_key, damage: i32, point: Vec3f)
```

**Design**: Use `Option<fn(&mut GameWorld, EntityKey, ...)>` for callbacks. The
`GameWorld` struct holds `EntityStorage` plus game-wide state, replacing global
`g_edicts[]` access. Free functions are used instead of closures to keep
entities `Send + Sync` and serializable.

**Acceptance Criteria**:
- [ ] Given an entity with a `think` callback and `nextthink` <= level.time,
      when `G_RunEntity` processes it, then the think callback is invoked
- [ ] Given an entity with no `think` callback, when `G_RunEntity` processes
      it, then no panic occurs and the entity is skipped
- [ ] Given two entities colliding, when `SV_Impact` is called, then both
      entities' `touch` callbacks fire with correct plane/surface data
- [ ] Given an entity taking lethal damage, when health <= 0, then `die` is
      called instead of `pain`

**Priority**: High
**Dependencies**: None (extends existing entity.rs)
**Estimated Effort**: M

---

### FR-002: MoveInfo and MonsterInfo Structs

**Description**: Add `MoveInfo` and `MonsterInfo` structs to the entity system
to support platform/door movement and monster AI state machines respectively.

**C Reference**: `local.h:362-390` (moveinfo_t), `local.h:407-439` (monsterinfo_t)

**MoveInfo fields**: start_origin, start_angles, end_origin, end_angles,
sound_start/middle/end, accel, speed, decel, distance, wait, state, dir,
current_speed, move_speed, next_speed, remaining_distance, decel_distance,
endfunc callback.

**MonsterInfo fields**: current_move (animation), aiflags, nextframe, scale,
10 AI callbacks (stand, idle, search, walk, run, dodge, attack, melee, sight,
checkattack), pausetime, attack_finished, saved_goal, search_time, trail_time,
last_sighting, attack_state, lefty, idle_time, linkcount, power_armor_type/power.

**Acceptance Criteria**:
- [ ] Given a `func_plat` entity with MoveInfo, when triggered, then it
      moves from start_origin to end_origin respecting accel/decel curves
- [ ] Given a monster entity with MonsterInfo, when its AI runs, then
      it dispatches to the correct state callback (stand/walk/run/attack)
- [ ] Given a MonsterInfo with `currentmove` animation data, when a frame
      advances, then the correct animation frame callback fires

**Priority**: High
**Dependencies**: FR-001
**Estimated Effort**: M

---

### FR-003: Movement Types and Physics Dispatch

**Description**: Implement 10 movement types and 5 physics dispatch functions
that govern how entities move each frame. This is the core physics loop called
from `G_RunEntity`.

**C Reference**: `local.h:188-202` (movetype_t), `g_phys.c` (1,300 lines)

**Movement types**: None, Noclip, Push, Stop, Walk, Step, Fly, Toss,
FlyMissile, Bounce.

**Physics functions**:
- `SV_Physics_None` — stationary
- `SV_Physics_Noclip` — no collision, free movement
- `SV_Physics_Pusher` — pushes other entities (doors, platforms)
- `SV_Physics_Step` — gravity + ground checks (monsters, players)
- `SV_Physics_Toss` — gravity + bounce (projectiles, gibs)

**Key helpers**: `SV_CheckVelocity`, `SV_Impact`, `SV_ClipVelocity`,
`SV_FlyMove`, `SV_Push`, `SV_PushEntity`, `M_CheckGround`,
`SV_AddGravity`, `SV_AddRotationalFriction`.

**Acceptance Criteria**:
- [ ] Given an entity with MOVETYPE_TOSS and nonzero velocity, when physics
      runs, then gravity is applied and the entity traces against BSP
- [ ] Given a MOVETYPE_PUSH entity (door), when it moves into a player,
      then the player is pushed and `blocked` callback fires if stuck
- [ ] Given a MOVETYPE_BOUNCE entity (grenade), when it hits a surface,
      then velocity is reflected with damping and `touch` callback fires
- [ ] Given a MOVETYPE_STEP entity (monster) on a ledge, when stepping,
      then it steps down up to STEPSIZE (18.0) units
- [ ] Given velocity exceeding `sv_maxvelocity`, when `SV_CheckVelocity`
      runs, then velocity is clamped

**Priority**: High
**Dependencies**: FR-001, FR-002
**Estimated Effort**: L

---

### FR-004: Combat and Damage System

**Description**: Implement damage application, armor absorption, knockback,
kill tracking, and radius damage. This is the core combat loop used by
weapons, monsters, triggers, and environmental hazards.

**C Reference**: `g_combat.c` (762 lines)

**Functions**: `T_Damage`, `T_RadiusDamage`, `Killed`, `SpawnDamage`,
`CheckTeamDamage`, `CanDamage` (line-of-sight check).

**34 means of death** (MOD_BLASTER through MOD_TARGET_BLASTER + MOD_FRIENDLY_FIRE).

**6 damage flags**: DAMAGE_RADIUS, DAMAGE_NO_ARMOR, DAMAGE_ENERGY,
DAMAGE_NO_KNOCKBACK, DAMAGE_BULLET, DAMAGE_NO_PROTECTION.

**Acceptance Criteria**:
- [ ] Given a player with 100 health and 50 armor, when taking 30 damage,
      then armor absorbs a portion and health decreases by the remainder
- [ ] Given an entity taking lethal damage, when `T_Damage` reduces health
      <= 0, then `Killed()` is called which invokes the entity's `die` callback
- [ ] Given a rocket explosion at origin, when `T_RadiusDamage` is called
      with radius 120, then entities within radius take distance-scaled damage
- [ ] Given DAMAGE_NO_KNOCKBACK flag, when damage is applied, then the
      target's velocity is unchanged
- [ ] Given a player kill, when the attacker is tracked, then the correct
      MOD_* value is recorded for obituary messages

**Priority**: High
**Dependencies**: FR-001
**Estimated Effort**: M

---

### FR-005: Item System

**Description**: Implement 31 item definitions with pickup, use, drop, and
weapon-think handlers. Items are spawned as world entities and enter player
inventories on pickup.

**C Reference**: `g_items.c` (2,712 lines), `local.h:234-261` (gitem_t)

**Item categories**:
- 6 Armor types (body, combat, jacket, shard, power screen, power shield)
- 6 Powerups (quad, invulnerability, adrenaline, silencer, breather, enviro)
- 11 Weapons (blaster, shotgun, super shotgun, machinegun, chaingun,
  grenade launcher, rocket launcher, hyperblaster, railgun, BFG10K,
  hand grenades)
- 6 Ammo types (bullets, shells, grenades, rockets, cells, slugs)
- 2 Utility (bandolier, pack)

**7 item flags**: IT_WEAPON, IT_AMMO, IT_ARMOR, IT_STAY_COOP, IT_KEY,
IT_POWERUP, IT_INSTANT_USE.

**Acceptance Criteria**:
- [ ] Given a player touching a weapon entity, when pickup succeeds, then
      the weapon is added to inventory and the entity is removed (or respawns
      in DM)
- [ ] Given a player with a weapon, when `Use_Weapon` is called, then the
      active weapon changes and the weapon model updates
- [ ] Given a player with quad damage, when dealing damage, then damage is
      multiplied by 4
- [ ] Given full ammo, when touching an ammo pickup, then pickup is rejected
      and the item remains in the world
- [ ] Given a DM game, when an item is picked up, then it respawns after
      the configured delay

**Priority**: High
**Dependencies**: FR-001, FR-004
**Estimated Effort**: L

---

### FR-006: Weapon Fire System

**Description**: Implement weapon firing logic for all 11 weapons including
projectile entity creation, hitscan traces, and weapon state machines.

**C Reference**: `g_weapon.c` (1,231 lines), `player/weapon.c` (1,928 lines)

**Weapon types**:
- Hitscan: blaster, shotgun, super shotgun, machinegun, chaingun, railgun
- Projectile: grenade launcher, rocket launcher, hyperblaster, BFG10K
- Special: hand grenades (thrown projectile)

**Weapon state machine**: idle → firing → cooldown → idle, with reload and
ammo checks.

**Acceptance Criteria**:
- [ ] Given a player firing the shotgun, when `weapon_shotgun_fire` runs,
      then multiple hitscan traces spread in a pattern and damage is applied
      to hit entities
- [ ] Given a player firing the rocket launcher, when fire runs, then a
      rocket entity spawns with MOVETYPE_FLYMISSILE and a `think` callback
      for explosion
- [ ] Given a rocket entity hitting a wall, when `touch` fires, then
      `T_RadiusDamage` is called and the rocket entity is removed
- [ ] Given zero ammo, when the player attempts to fire, then the weapon
      switches to the next available weapon
- [ ] Given the BFG, when fire runs, then the BFG laser effect applies
      damage to visible targets each frame

**Priority**: High
**Dependencies**: FR-004, FR-005
**Estimated Effort**: L

---

### FR-007: Trigger Entities

**Description**: Implement 11 trigger entity types that activate effects when
touched or targeted.

**C Reference**: `g_trigger.c` (863 lines)

**Types**: trigger_always, trigger_once, trigger_multiple, trigger_relay,
trigger_push, trigger_hurt, trigger_key, trigger_counter, trigger_elevator,
trigger_gravity, trigger_monsterjump.

**Acceptance Criteria**:
- [ ] Given a trigger_multiple with a `wait` of 2.0, when touched twice
      within 2 seconds, then only the first touch fires
- [ ] Given a trigger_hurt with 10 dmg, when a player stands in it, then
      they take 10 damage per second
- [ ] Given a trigger_push with speed 1000, when a player enters, then
      their velocity is set to the push direction * speed
- [ ] Given a trigger_once, when fired, then it activates its targets
      and immediately removes itself

**Priority**: Medium
**Dependencies**: FR-001, FR-003, FR-004
**Estimated Effort**: M

---

### FR-008: Target Entities

**Description**: Implement 17 target entity types that produce effects when
triggered.

**C Reference**: `g_target.c` (1,234 lines)

**Types**: target_temp_entity, target_speaker, target_explosion,
target_changelevel, target_secret, target_goal, target_splash,
target_spawner, target_blaster, target_crosslevel_trigger/target,
target_laser, target_help, target_lightramp, target_earthquake,
target_character, target_string.

**Acceptance Criteria**:
- [ ] Given a target_changelevel, when triggered, then `level.exitintermission`
      is set with the correct next map name
- [ ] Given a target_explosion, when triggered, then a temp entity explosion
      effect is sent to clients and radius damage is applied
- [ ] Given a target_laser, when active, then it traces a beam each frame
      and damages entities it hits
- [ ] Given a target_speaker, when triggered, then a sound is played at the
      entity's origin

**Priority**: Medium
**Dependencies**: FR-001, FR-004
**Estimated Effort**: M

---

### FR-009: Functional Entities

**Description**: Implement 16 `func_*` entity types — doors, platforms,
trains, buttons, rotating objects, and other interactive world geometry.

**C Reference**: `g_func.c` (3,012 lines — largest single game file)

**Types**: func_plat, func_button, func_door, func_door_secret,
func_door_rotating, func_rotating, func_train, func_water, func_conveyor,
func_areaportal, func_clock, func_wall, func_object, func_timer,
func_explosive, func_killbox.

**Acceptance Criteria**:
- [ ] Given a func_door with a trigger target, when triggered, then the
      door moves from closed to open position using MoveInfo accel/decel
- [ ] Given a func_plat, when a player steps on it, then it descends to
      the bottom and returns after `wait` seconds
- [ ] Given a func_train with path_corner waypoints, when activated, then
      it moves along the waypoint chain
- [ ] Given a func_explosive, when it takes enough damage, then it spawns
      debris gibs and removes itself
- [ ] Given a func_door in a team, when one opens, then all team members
      open together

**Priority**: Medium
**Dependencies**: FR-001, FR-002, FR-003
**Estimated Effort**: XL

---

### FR-010: Miscellaneous Entities and Utility Functions

**Description**: Implement ~23 misc entities (explobox, banner, gibs,
teleporters, etc.) and core utility functions (G_Find, G_PickTarget,
G_UseTargets, G_FreeEdict, G_Spawn, etc.).

**C Reference**: `g_misc.c` (2,726 lines), `g_utils.c` (714 lines)

**Acceptance Criteria**:
- [ ] Given `G_UseTargets` called on an entity, when it has `target` and
      `killtarget` fields, then matching entities are activated/removed
- [ ] Given `G_Find` with a fieldname and value, when called, then it
      returns the next entity matching that field value
- [ ] Given a misc_teleporter, when a player enters, then they are
      teleported to the misc_teleporter_dest
- [ ] Given a misc_explobox, when it takes enough damage, then it explodes
      with radius damage

**Priority**: Medium
**Dependencies**: FR-001, FR-003, FR-004
**Estimated Effort**: L

---

### FR-011: Base AI System

**Description**: Implement the 20 core AI functions that drive monster
behavior: pathfinding, target selection, movement decisions, attack logic,
and ground checking.

**C Reference**: `g_ai.c` (1,328 lines), `g_monster.c` (1,086 lines)

**Key functions**: AI_SetSightClient, ai_stand, ai_walk, ai_run, ai_charge,
ai_move, ai_turn, M_MoveToGoal, M_walkmove, M_MoveFrame, M_CheckGround,
M_CheckBottom, M_CategorizePosition, M_FlyCheck, M_SetEffects,
monster_start, monster_start_go, monster_triggered_start,
monster_use, monster_death_use.

**AI state machine**: Each monster cycles through stand/walk/run/attack/pain/die
states, driven by `monsterinfo.currentmove` which specifies frame ranges and
per-frame callbacks.

**Acceptance Criteria**:
- [ ] Given a monster in `stand` state with no enemy, when `ai_stand` runs,
      then it remains standing and periodically calls `idle` sound
- [ ] Given a monster that spots a player, when line-of-sight is confirmed,
      then `sight` callback fires and monster transitions to `run` state
- [ ] Given a monster in `run` state, when within attack range, then
      `checkattack` is called and may transition to `attack` state
- [ ] Given a monster taking damage, when `pain` fires, then it transitions
      to the pain animation sequence before returning to `run`
- [ ] Given `M_MoveToGoal`, when the path is blocked, then the monster
      attempts to step around obstacles

**Priority**: High
**Dependencies**: FR-001, FR-002, FR-003, FR-004
**Estimated Effort**: L

---

### FR-012: Monster Implementations (20 types)

**Description**: Port all 20 monster types from C to Rust. Each monster
defines animation frame tables, state transitions, attack patterns, pain
reactions, and death sequences.

**C Reference**: `monster/` directory (19,434 lines across 40 files)

**Monsters**: berserk, boss2, boss3 (makron + jorg), brain, chick, flipper,
float, flyer, gladiator, gunner, hover, infantry, insane, medic, mutant,
parasite, soldier (3 variants: light/normal/ss), supertank, tank (+ commander).

**Acceptance Criteria**:
- [ ] Given `monster_soldier` spawned, when `monster_start` runs, then
      the soldier enters `stand` state with correct animation frames
- [ ] Given a soldier that spots a player, when attack check passes, then
      it fires its weapon (blaster/shotgun/machinegun per variant)
- [ ] Given a monster killed, when `die` runs, then the death animation
      plays and the entity transitions to `SVF_DEADMONSTER`
- [ ] Given all 20 monster types, when spawned in a test map entity string,
      then each initializes without panic and runs 1 AI frame

**Priority**: Medium (parallelizable — each monster is independent)
**Dependencies**: FR-011
**Estimated Effort**: XL

---

### FR-013: Player Subsystem

**Description**: Implement player client management, weapon handling, view
calculations, HUD updates, and player trail for AI tracking.

**C Reference**: `player/client.c` (2,501 lines), `player/weapon.c` (1,928),
`player/view.c` (1,426), `player/hud.c` (657), `player/trail.c` (175)

**Key functions**: ClientBeginServerFrame, ClientEndServerFrame,
ClientThink, ClientConnect, ClientBegin, ClientDisconnect,
PutClientInServer, respawn, spectator_respawn, body_die,
PlayerTrail_Add/Pick/LastSpot, Think_Weapon, ChangeWeapon,
ClientUserinfoChanged.

**Acceptance Criteria**:
- [ ] Given a new client connecting, when `ClientConnect` is called, then
      a player entity is allocated with default persistent data
- [ ] Given `ClientThink` called each frame, when processing the UserCmd,
      then the player's movement and weapon state update correctly
- [ ] Given the player's view, when `SV_CalcViewOffset` runs, then view
      bob, damage kicks, and fall effects are applied
- [ ] Given the HUD, when `G_SetStats` runs, then health, ammo, armor,
      and weapon icon stats are set in player_state_t.stats[]
- [ ] Given `PlayerTrail_Add`, when called, then trail nodes are recorded
      for monster AI pathfinding

**Priority**: High
**Dependencies**: FR-001, FR-004, FR-005, FR-006
**Estimated Effort**: XL

---

### FR-014: Game Main Loop

**Description**: Implement `G_RunFrame` (the per-tick game update),
`InitGame`, `ShutdownGame`, and the `GameExport` trait.

**C Reference**: `g_main.c` (514 lines)

**G_RunFrame sequence**:
1. Increment `level.framenum` and `level.time`
2. `AI_SetSightClient` — pick a client for monsters to target
3. Check exit intermission
4. For each entity: save old_origin, check ground entity staleness,
   dispatch player entities to `ClientBeginServerFrame`, dispatch all
   others to `G_RunEntity` (physics + think)
5. `CheckDMRules`, `CheckNeedPass`, `ClientEndServerFrames`

**Acceptance Criteria**:
- [ ] Given `G_RunFrame` called, when `level.framenum` increments, then
      `level.time` equals `framenum * FRAMETIME` (0.1s)
- [ ] Given `InitGame` called, when initializing, then all cvars are
      registered and entity/client storage is allocated
- [ ] Given the `GameExport` trait impl, when `spawn_entities` is called
      with a BSP entity string, then all entities are parsed and spawned
      via the spawn table
- [ ] Given `G_RunFrame` with mixed entity types, when running, then
      physics dispatch processes each entity's movetype correctly

**Priority**: High
**Dependencies**: FR-001, FR-003, FR-011, FR-013
**Estimated Effort**: M

---

### FR-015: Save/Load System

**Description**: Implement game state serialization using serde, replacing the
C function-pointer-to-string lookup tables with Rust's type system.

**C Reference**: `savegame/` directory (3,170 lines)

**Serialized state**: GameLocals, LevelLocals, all entities (with
EntityKey cross-references serialized as indices), client persistent data.

**SlotMap key stability**: EntityKey values contain version counters that are
NOT stable across save/load. Serialize entity cross-references as integer
indices, rebuild EntityKey handles on load.

**Acceptance Criteria**:
- [ ] Given a game state with 10 entities (some with owner/enemy refs),
      when saved and loaded, then all entities are restored with correct
      cross-references
- [ ] Given an entity with a `think` callback function, when saved, then
      the function is identified by name string; on load, it is resolved
      back to the correct function pointer
- [ ] Given a save file from a different session, when loaded, then the
      level state, client data, and entity states all restore correctly
- [ ] Given an entity freed before save, when loaded, then it remains
      absent from the SlotMap

**Priority**: Low (not needed for initial gameplay testing)
**Dependencies**: FR-001 through FR-014
**Estimated Effort**: L

---

### FR-016: Spawn Table Expansion

**Description**: Expand the spawn table from 4 entries to all 107 entity types
from the C source, registering the correct spawn function for each classname.

**C Reference**: `g_spawn.c:150-271`

**Entity categories**:
- 4 Player spawns
- 16 func_* entities
- 11 trigger_* entities
- 17 target_* entities
- 22 Monster spawns (20 unique + variants)
- 3 Turret spawns
- ~23 Misc entities
- ~11 Info/utility/items

**Acceptance Criteria**:
- [ ] Given a BSP entity string containing all 107 classnames, when
      `spawn_entities` runs, then each entity is dispatched to the correct
      spawn function without "unknown classname" warnings
- [ ] Given an unknown classname, when spawning, then a warning is logged
      and the entity is freed (no panic)

**Priority**: Medium (grows incrementally as FR-007 through FR-012 land)
**Dependencies**: FR-003 through FR-012
**Estimated Effort**: S (mechanical — just wiring up registrations)

---

## Non-Functional Requirements

### NFR-001: Safety — Zero Unsafe Code

**Requirement**: The q2-game crate must contain zero `unsafe` blocks. All
entity references use SlotMap generational keys. No raw pointers, no global
mutable state.

**Measurement**: `cargo clippy -p q2-game -- -D clippy::undocumented_unsafe_blocks`
reports zero findings. `grep -r "unsafe" crates/q2-game/src/` returns zero matches.

**Priority**: Critical

---

### NFR-002: Fidelity — Exact Gameplay Match

**Requirement**: Game behavior must match the C original for all deterministic
interactions: damage values, movement speeds, physics constants, item counts,
weapon fire rates, monster attack patterns.

**Measurement**: Side-by-side comparison of key constants against C source.
Unit tests for damage calculations, physics timesteps, and AI state
transitions must produce identical results to C.

**Priority**: Critical

---

### NFR-003: Performance — 10ms Game Frame Budget

**Requirement**: `G_RunFrame` must complete within 10ms on a mid-range 2024
CPU with 1024 entities (MAX_EDICTS). The original C code runs well under 1ms;
Rust overhead from SlotMap indirection and bounds checking should not push this
above 10ms.

**Measurement**: Benchmark `G_RunFrame` with 1024 active entities using
`criterion`. Target: < 5ms p99.

**Priority**: High

---

### NFR-004: Testability — 80%+ Function Coverage

**Requirement**: Every public function in q2-game must have at least one unit
test. Complex subsystems (combat, physics, AI) must have integration tests
with multi-entity scenarios.

**Measurement**: `cargo llvm-cov` or `cargo tarpaulin` on q2-game reports
>= 80% function coverage.

**Priority**: High

---

### NFR-005: Portability — WASM-Compatible

**Requirement**: All q2-game code must compile to `wasm32-unknown-unknown`.
No filesystem access, no threading, no platform-specific APIs. All I/O goes
through the `GameImport` trait.

**Measurement**: `cargo check -p q2-game --target wasm32-unknown-unknown`
passes.

**Priority**: Critical

---

## Technical Constraints

### TC-001: Architecture

- **Crate**: All game logic lives in `q2-game`. No circular dependencies.
- **DAG**: `q2-shared` → `q2-common` → `q2-game`. Game depends only on
  shared types and common utilities.
- **Trait boundary**: Game communicates with the engine exclusively through
  `GameImport` (callbacks to engine) and `GameExport` (engine calls into game).
- **No global state**: All state is owned by `GameWorld` struct or passed
  by reference. No `static mut`, no `lazy_static`, no thread-local storage.

### TC-002: Entity Callbacks as Free Functions

Entity callbacks (`think`, `touch`, `use`, `pain`, `die`, `blocked`) must be
free functions (`fn` pointers), not closures. This ensures:
- Entities remain `Send + Sync`
- Callbacks can be serialized by name for save/load
- No lifetime or borrow issues with self-referential closures

Type: `Option<fn(&mut GameWorld, EntityKey, ...)>`

### TC-003: GameWorld as Central State

A `GameWorld` struct replaces the C globals (`g_edicts`, `level`, `game`,
`globals`). All entity access goes through `GameWorld` methods. This is the
`&mut self` receiver for all game logic, avoiding the C pattern of reaching
into global arrays.

```rust
pub struct GameWorld {
    pub entities: EntityStorage,
    pub level: LevelLocals,
    pub game: GameLocals,
    pub items: Vec<ItemDef>,
    pub spawn_table: SpawnTable,
    pub gi: Box<dyn GameImport>,  // engine callbacks
}
```

### TC-004: Constants Match C Exactly

All gameplay constants (FRAMETIME, STEPSIZE, gravity, weapon damage values,
item quantities, monster health/pain thresholds) must be copied verbatim from
the C source with comments referencing the source file and line.

### TC-005: Dependencies

- `q2-shared` (workspace) — Vec3f, EntityState, PlayerState, UserCmd, Trace
- `q2-common` (workspace) — Q2Error, collision queries (via GameImport)
- `slotmap` (workspace) — EntityKey, SlotMap
- `serde` + `bincode` (workspace) — save/load serialization
- `tracing` (workspace) — debug logging
- `bitflags` (workspace) — flag enums (damage flags, entity flags, item flags)

No new external dependencies required.

---

## Out of Scope (v0.1.1)

- **Xatrix/Rogue mission pack entities** — only base Q2 entities
- **CTF game mode** — only DM and Coop
- **Monster animation frame data as assets** — frame indices are hardcoded
  constants matching the C source, not loaded from files
- **GL1/soft renderer interactions** — only GL3/GLES3 via existing trait
- **Network protocol changes** — game uses existing `GameImport` network
  methods unchanged
- **Multiplayer anti-cheat** — deferred to Phase 7+

---

## Dependencies

| Dependency | Status | Notes |
|-----------|--------|-------|
| q2-shared types | Done | EntityState, PlayerState, UserCmd |
| q2-common collision | Done | CM_BoxTrace, CM_PointContents (via GameImport) |
| q2-common pmove | Done | PlayerController (via GameImport::pmove) |
| q2-server GameImport impl | Phase 3 | Needed for integration testing; use MockGameImport for unit tests |

---

## Acceptance Testing Strategy

### Unit Tests (per-function)
Each module gets tests for core logic: damage math, physics timestep,
item pickup rules, AI state transitions, entity string parsing.

### Integration Tests (multi-entity)
Spawn multiple entities and run `G_RunFrame` for N frames:
- Soldier spots player, transitions to attack, fires weapon
- Rocket hits wall, creates explosion, damages nearby entities
- Door opens, player walks through, door closes after wait period
- Player picks up weapon, switches to it, fires, expends ammo

### Fidelity Tests (C cross-reference)
Key constants and calculations verified against C source values.

### Checkpoint Test (CP-2)
> Spawn `info_player_start` + `monster_soldier`, soldier enters `stand` state
> and runs 1 AI frame without panic.

This is the minimum viable Phase 2 outcome.

---

## Success Criteria

- [ ] All 107 spawn table entries registered and dispatch correctly
- [ ] `cargo test -p q2-game` passes with 200+ tests
- [ ] `cargo clippy -p q2-game` clean (zero warnings)
- [ ] `cargo check -p q2-game --target wasm32-unknown-unknown` passes
- [ ] Zero `unsafe` blocks in q2-game
- [ ] CP-2 checkpoint passes: spawn player + soldier, run 1 AI frame
- [ ] `GameExport` trait fully implemented and callable from q2-server

---

## Glossary

| Term | Definition |
|------|-----------|
| `edict_t` | C entity struct; replaced by `Entity` + `SlotMap` |
| `GameImport` | Engine-to-game callback trait (server implements) |
| `GameExport` | Game-to-engine interface trait (game implements) |
| `EntityKey` | Generational SlotMap key; safe replacement for `edict_t*` |
| `MoveInfo` | Movement data for doors/platforms (start/end positions, speed) |
| `MonsterInfo` | AI state for monsters (callbacks, animation, targeting) |
| `FRAMETIME` | 0.1 seconds (10Hz game tick rate) |
| `MOD_*` | Means of death — tracks what killed an entity |
| `SlotMap` | Data structure providing O(1) insert/remove/lookup with generational indices |

---

## References

- C source: `~/Qwasm2/src/game/` (Yamagi Quake II)
- Conversion plan: `docs/superpowers/plans/2026-03-26-c-to-rust-conversion.md` (Phase 2)
- ADR: `docs/adr/001-crate-decomposition-and-trait-boundaries.md`
- SlotMap crate: https://docs.rs/slotmap
