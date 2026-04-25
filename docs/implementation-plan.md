# Phase 2: Game Logic — Implementation Plan v0.2.0

**Author**: Claude (attune:project-planning)
**Date**: 2026-04-10
**Branch**: `game-logic-0.1.1`
**Specification**: `docs/specification.md`
**Status**: CORE COMPLETE — CP-2 passes, ready for PR to main

## Completion Summary (2026-04-10)

| Sprint | Status | Tasks | Tests Added | Commits |
|--------|--------|-------|-------------|---------|
| Sprint 1: Core Framework | **DONE** | 6/6 | +114 | `7b77c53` |
| Sprint 2: World Entities | **DONE** | 5/5 | +39 | `de5ba67` |
| Sprint 3: AI + Game Loop | **DONE** | 3/3* | +23 | `90dc60d` |
| Sprint 4: Player + Monsters | **DONE** | 3/3* | +14 | `2800ace` |

*Sprints 3-4 restructured from original plan — AI/game loop and
player/monsters combined for CP-2 checkpoint efficiency.

**Final metrics**: 41 files, 11,507 lines, 208 tests, 0 unsafe, 0 clippy warnings.

### Deferred to separate branches (GitHub issues created):
- #14: Full monster attack/pain animations
- #15: Save/load system (serde + callback registry)
- #16: Player weapon state machine + DM rules
- #17: Full SV_Push with entity displacement rollback

---

**Estimated Total**: ~18,000 new Rust LOC replacing ~22,000 C LOC (11,507 delivered)

---

## Architecture

### System Overview

All game logic lives in `q2-game`. The crate has one primary struct —
`GameWorld` — that owns all game state and exposes the `GameExport` trait for
the server to call into. The game communicates with the engine exclusively
through the `GameImport` trait (passed as `Box<dyn GameImport>`).

```
┌─────────────────────────────────────────────────────┐
│  q2-server  (Phase 3)                               │
│  Implements GameImport, calls GameExport             │
└───────────┬─────────────────────────┬───────────────┘
            │ &dyn GameImport         │ &mut dyn GameExport
            ▼                         ▼
┌─────────────────────────────────────────────────────┐
│  q2-game  (THIS PHASE)                              │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │  GameWorld  (central state)                  │   │
│  │  ├── entities: EntityStorage (SlotMap)        │   │
│  │  ├── level: LevelLocals                      │   │
│  │  ├── game: GameLocals                        │   │
│  │  ├── items: Vec<ItemDef>                     │   │
│  │  ├── spawn_table: SpawnTable                 │   │
│  │  └── gi: Box<dyn GameImport>                 │   │
│  └──────────────────────────────────────────────┘   │
│                                                     │
│  Modules:                                           │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌──────────┐  │
│  │ physics │ │ combat  │ │ items   │ │ weapons  │  │
│  └────┬────┘ └────┬────┘ └────┬────┘ └────┬─────┘  │
│       │           │           │            │        │
│  ┌────┴────┐ ┌────┴────┐ ┌───┴────┐ ┌─────┴─────┐  │
│  │triggers │ │targets  │ │  func  │ │   misc    │  │
│  └─────────┘ └─────────┘ └────────┘ └───────────┘  │
│                                                     │
│  ┌──────────┐ ┌──────────┐ ┌────────────────────┐   │
│  │   ai     │ │  player  │ │  monster/ (20 types)│  │
│  └──────────┘ └──────────┘ └────────────────────┘   │
│                                                     │
│  ┌──────────────────────────────────────────────┐   │
│  │  savegame/ (serde serialization)             │   │
│  └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────┘
            │                         │
            ▼                         ▼
┌─────────────────────┐  ┌────────────────────────────┐
│  q2-common          │  │  q2-shared                  │
│  (collision, pmove)  │  │  (EntityState, Vec3f, etc.) │
└─────────────────────┘  └────────────────────────────┘
```

### Data Flow

1. **Server calls `GameExport::run_frame()`** → `GameWorld::run_frame()`
2. **G_RunFrame iterates entities** → dispatches to physics by movetype
3. **Physics calls think/touch/blocked callbacks** → modify entities via `&mut GameWorld`
4. **Combat calls pain/die callbacks** → may spawn new entities (gibs, explosions)
5. **AI reads entity state** → makes movement/attack decisions via GameImport traces
6. **GameImport writes** accumulate in engine's message buffer → flushed after frame

---

## File Structure

| File | Action | Purpose |
|------|--------|---------|
| `crates/q2-game/src/lib.rs` | Modify | Add new module declarations |
| `crates/q2-game/src/entity.rs` | Modify | Add callback fields, MoveInfo, MonsterInfo |
| `crates/q2-game/src/traits.rs` | Modify | Minor additions (Multicast enum, CVarHandle) |
| `crates/q2-game/src/spawn.rs` | Modify | Expand spawn table to 107 entries |
| `crates/q2-game/src/world.rs` | Create | GameWorld struct, G_RunFrame, InitGame, ShutdownGame |
| `crates/q2-game/src/constants.rs` | Create | FL_*, DAMAGE_*, MOD_*, IT_*, game constants |
| `crates/q2-game/src/physics.rs` | Create | 10 movetypes, 5 SV_Physics_* dispatch functions |
| `crates/q2-game/src/combat.rs` | Create | T_Damage, T_RadiusDamage, Killed, CanDamage |
| `crates/q2-game/src/items.rs` | Create | 31 item definitions, pickup/use/drop handlers |
| `crates/q2-game/src/weapons.rs` | Create | 11 weapon fire functions, projectile spawning |
| `crates/q2-game/src/triggers.rs` | Create | 11 trigger entity types |
| `crates/q2-game/src/targets.rs` | Create | 17 target entity types |
| `crates/q2-game/src/func.rs` | Create | 16 func_* entity types (doors, platforms, trains) |
| `crates/q2-game/src/misc.rs` | Create | ~23 misc entities + utility functions |
| `crates/q2-game/src/utils.rs` | Create | G_Find, G_PickTarget, G_UseTargets, G_FreeEdict |
| `crates/q2-game/src/ai.rs` | Create | 20 AI functions, M_Move*, AI_SetSightClient |
| `crates/q2-game/src/player/mod.rs` | Create | Player module root |
| `crates/q2-game/src/player/client.rs` | Create | ClientConnect/Begin/Disconnect/Think, respawn |
| `crates/q2-game/src/player/weapon.rs` | Create | Think_Weapon, ChangeWeapon, weapon state machine |
| `crates/q2-game/src/player/view.rs` | Create | SV_CalcViewOffset, view bob, damage kicks |
| `crates/q2-game/src/player/hud.rs` | Create | G_SetStats, HUD stat calculations |
| `crates/q2-game/src/player/trail.rs` | Create | PlayerTrail_Add/Pick/LastSpot for AI tracking |
| `crates/q2-game/src/monster/mod.rs` | Create | Monster module root, monster_start, shared AI |
| `crates/q2-game/src/monster/soldier.rs` | Create | Soldier (3 variants: light/normal/ss) |
| `crates/q2-game/src/monster/infantry.rs` | Create | Infantry monster |
| `crates/q2-game/src/monster/gunner.rs` | Create | Gunner monster |
| `crates/q2-game/src/monster/gladiator.rs` | Create | Gladiator monster |
| `crates/q2-game/src/monster/berserker.rs` | Create | Berserker monster |
| `crates/q2-game/src/monster/brain.rs` | Create | Brain monster |
| `crates/q2-game/src/monster/chick.rs` | Create | Chick monster |
| `crates/q2-game/src/monster/flipper.rs` | Create | Flipper monster |
| `crates/q2-game/src/monster/floater.rs` | Create | Float/Floater monster |
| `crates/q2-game/src/monster/flyer.rs` | Create | Flyer monster |
| `crates/q2-game/src/monster/hover.rs` | Create | Hover monster |
| `crates/q2-game/src/monster/medic.rs` | Create | Medic monster |
| `crates/q2-game/src/monster/mutant.rs` | Create | Mutant monster |
| `crates/q2-game/src/monster/parasite.rs` | Create | Parasite monster |
| `crates/q2-game/src/monster/insane.rs` | Create | Insane prisoner (non-hostile) |
| `crates/q2-game/src/monster/tank.rs` | Create | Tank + Tank Commander |
| `crates/q2-game/src/monster/supertank.rs` | Create | Supertank boss |
| `crates/q2-game/src/monster/boss2.rs` | Create | Boss2 (Hornet) |
| `crates/q2-game/src/monster/boss3.rs` | Create | Boss3 (Makron + Jorg) |
| `crates/q2-game/src/monster/turret.rs` | Create | turret_breach, turret_base, turret_driver |
| `crates/q2-game/src/savegame/mod.rs` | Create | Save/load module root |
| `crates/q2-game/src/savegame/serialize.rs` | Create | Serde serialization for game state |
| `crates/q2-game/src/savegame/registry.rs` | Create | Callback function name ↔ fn pointer registry |
| `crates/q2-game/Cargo.toml` | Modify | Add bitflags dependency |

---

## Task Breakdown

### Sprint 1: Core Framework (TASK-001 through TASK-006)

These tasks build the foundation that every subsequent task depends on.

---

#### TASK-001: Game Constants and Flag Enums

**Description**: Create `constants.rs` with all gameplay constants, flag enums
(FL_*, DAMAGE_*, MOD_*, IT_*, SVF_*), movement type enum, and dead/takedamage
enums. These are referenced by every other module.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 2 points
**Dependencies**: None
**Sprint**: 1
**Linked Requirements**: NFR-002 (fidelity)

**Files**: Create `constants.rs`, modify `lib.rs`

**C Reference**: `local.h:62-75` (FL_*), `local.h:188-202` (movetype),
`local.h:453-488` (MOD_*), `local.h:671-676` (DAMAGE_*), `local.h:213-219` (IT_*)

**Acceptance Criteria**:
- [ ] All FL_* flags defined as bitflags matching C values exactly
- [ ] All DAMAGE_* flags defined as bitflags matching C values
- [ ] All 34 MOD_* values defined as enum or constants
- [ ] MoveType enum with 10 variants matching C movetype_t
- [ ] IT_* item flags matching C values
- [ ] DeadFlag and TakeDamage enums defined
- [ ] `cargo test -p q2-game` passes
- [ ] `cargo clippy -p q2-game` clean

**Testing**:
- Unit: verify flag values match C (e.g., `FL_FLY == 0x01`, `MOD_BLASTER == 1`)
- Unit: verify bitflag combinations work (`FL_FLY | FL_SWIM`)

---

#### TASK-002: Entity Callback System and Struct Expansion

**Description**: Extend `Entity` with 7 callback function fields, `MoveInfo`,
`MonsterInfo`, and additional game fields from `edict_t`. Add `LevelLocals`
and `GameLocals` structs.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 5 points
**Dependencies**: TASK-001
**Sprint**: 1
**Linked Requirements**: FR-001, FR-002

**Files**: Modify `entity.rs`, create types in `world.rs`

**C Reference**: `local.h:362-390` (moveinfo_t), `local.h:407-439` (monsterinfo_t),
`local.h:972-1117` (edict_t), `local.h:795-886` (level_locals_t, game_locals_t)

**Callback type**: `type ThinkFn = fn(&mut GameWorld, EntityKey);` (and similar
for each signature variant)

**Entity additions**:
- 7 callback fields: `prethink`, `think`, `blocked`, `touch`, `use_fn`, `pain`, `die`
- `nextthink: f32` (time at which `think` fires)
- `moveinfo: Option<Box<MoveInfo>>` (boxed to keep Entity small)
- `monsterinfo: Option<Box<MonsterInfo>>` (boxed)
- Remaining edict_t fields not already present: `movetype`, `flags`, `velocity`,
  `avelocity`, `mass`, `gravity_multiplier`, `ground_entity`, `watertype`,
  `waterlevel`, `noise_index`, `noise_index2`, `classname` (String), etc.

**Acceptance Criteria**:
- [ ] Entity struct has all 7 callback fields as `Option<CallbackFn>`
- [ ] MoveInfo struct has all fields from C moveinfo_t
- [ ] MonsterInfo struct has all fields from C monsterinfo_t (10 AI callbacks)
- [ ] LevelLocals has framenum, time, level_name, etc.
- [ ] GameLocals has maxclients, maxentities, cvars
- [ ] Existing 18 tests still pass
- [ ] New tests for callback invocation via EntityKey

**Testing**:
- Unit: set think callback, verify it can be called with `(world, key)` args
- Unit: MoveInfo default has zeroed fields
- Unit: MonsterInfo AI callbacks are all `None` by default

---

#### TASK-003: GameWorld Struct and Utility Functions

**Description**: Create `GameWorld` as the central state holder and implement
core utility functions: `G_Spawn`, `G_FreeEdict`, `G_Find`, `G_PickTarget`,
`G_UseTargets`, `G_SetMovedir`, `KillBox`, `vtos`, `vectoyaw`.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 5 points
**Dependencies**: TASK-002
**Sprint**: 1
**Linked Requirements**: FR-010, FR-014

**Files**: Create `world.rs`, create `utils.rs`, modify `lib.rs`

**GameWorld fields**: entities (EntityStorage), level (LevelLocals),
game (GameLocals), items (Vec<ItemDef>), spawn_table (SpawnTable),
gi (Box<dyn GameImport>).

**Key methods on GameWorld**:
- `spawn() -> EntityKey` — allocate entity
- `free(key)` — mark entity unused
- `find(start, field, value) -> Option<EntityKey>` — search by field
- `pick_target(targetname) -> Option<EntityKey>` — pick random matching target
- `use_targets(ent, activator)` — activate entity's targets and killtargets
- `run_frame()` — main game tick (skeleton, filled in TASK-014)

**Also**: `MockGameImport` struct for unit testing all game code without a real
server.

**Acceptance Criteria**:
- [ ] `GameWorld::new(gi)` initializes all fields
- [ ] `spawn()` / `free()` work correctly
- [ ] `G_Find` iterates entities and matches by field value
- [ ] `G_UseTargets` fires `use_fn` on all matching targets and frees killtargets
- [ ] `MockGameImport` implements all trait methods with no-op/defaults
- [ ] All tests pass using `MockGameImport`

**Testing**:
- Unit: spawn 5 entities, free one, verify count and keys
- Unit: G_Find returns correct entity by classname
- Unit: G_UseTargets activates chain of 3 targets
- Integration: GameWorld round-trip with MockGameImport

---

#### TASK-004: Physics System

**Description**: Implement the 5 physics dispatch functions and helper utilities
for entity movement, collision, and gravity.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 8 points
**Dependencies**: TASK-003
**Sprint**: 1
**Linked Requirements**: FR-003

**Files**: Create `physics.rs`

**C Reference**: `g_phys.c` (1,300 lines, 17 functions)

**Functions to implement**:
- `G_RunEntity(world, key)` — dispatch by movetype
- `SV_Physics_None` — no-op
- `SV_Physics_Noclip` — update origin from velocity, no collision
- `SV_Physics_Pusher` — push other entities (doors/platforms)
- `SV_Physics_Step` — gravity, ground check, step up/down (monsters)
- `SV_Physics_Toss` — gravity, bounce, fly (projectiles)
- Helpers: `SV_CheckVelocity`, `SV_Impact`, `SV_ClipVelocity`, `SV_FlyMove`,
  `SV_Push`, `SV_PushEntity`, `SV_AddGravity`, `SV_AddRotationalFriction`,
  `M_CheckGround`, `M_CheckBottom`, `M_CategorizePosition`

**Physics → GameImport interaction**: Physics uses `gi.trace()` for collision
detection and `gi.link_entity()` after position changes.

**Acceptance Criteria**:
- [ ] MOVETYPE_TOSS entity with gravity falls and stops on ground
- [ ] MOVETYPE_PUSH entity pushes blocking entities out of the way
- [ ] MOVETYPE_BOUNCE entity reflects velocity on surface hit
- [ ] MOVETYPE_STEP entity walks and steps up small ledges
- [ ] SV_Impact calls both entities' touch callbacks
- [ ] SV_CheckVelocity clamps to sv_maxvelocity
- [ ] Physics uses gi.trace() (via MockGameImport) for all collision

**Testing**:
- Unit: toss entity in open space → velocity and origin change by gravity
- Unit: toss entity into floor → fraction < 1.0, entity stops, touch fires
- Unit: push entity into player → player is displaced
- Unit: step entity steps up STEPSIZE (18.0) ledge
- Unit: SV_ClipVelocity reflects correctly for 45-degree surface

---

#### TASK-005: Combat and Damage System

**Description**: Implement T_Damage, T_RadiusDamage, Killed, CanDamage,
and damage-related helpers.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 5 points
**Dependencies**: TASK-003
**Sprint**: 1
**Linked Requirements**: FR-004

**Files**: Create `combat.rs`

**C Reference**: `g_combat.c` (762 lines, 4 core functions)

**Functions**:
- `T_Damage(world, target, inflictor, attacker, dir, point, normal, damage, knockback, dflags, mod)` — the core damage function
- `T_RadiusDamage(world, inflictor, attacker, damage, ignore, radius, mod)` — area damage
- `Killed(world, target, inflictor, attacker, damage, point)` — triggers die callback
- `CanDamage(world, target, inflictor)` — line-of-sight check
- `SpawnDamage(world, type, origin, normal)` — visual effect via gi
- `CheckTeamDamage(world, target, attacker)` — team damage rules

**Damage flow**: check god mode → check notarget → armor absorption → apply
knockback → reduce health → if health <= 0: Killed() → else: pain callback

**Acceptance Criteria**:
- [ ] 100hp entity takes 30 damage → 70hp remaining
- [ ] Armor absorbs correct fraction (body=0.8, combat=0.6, jacket=0.3)
- [ ] Knockback applies velocity change proportional to damage/mass
- [ ] DAMAGE_NO_KNOCKBACK skips velocity change
- [ ] DAMAGE_NO_ARMOR bypasses armor
- [ ] Lethal damage calls Killed() → die callback
- [ ] T_RadiusDamage scales by distance, traces CanDamage per target
- [ ] God mode (FL_GODMODE) prevents health reduction

**Testing**:
- Unit: T_Damage with armor → correct health/armor math
- Unit: T_RadiusDamage center vs edge → damage scales linearly with distance
- Unit: Killed triggers die callback on target entity
- Unit: CanDamage returns false when line-of-sight blocked (mock trace)
- Unit: knockback direction and magnitude

---

#### TASK-006: Item Definitions and Pickup System

**Description**: Define 31 items and implement pickup, use, drop, and
weapon-think handlers. Implement item spawning and DM respawn logic.

**Type**: Implementation
**Priority**: P1 (High)
**Estimate**: 8 points
**Dependencies**: TASK-005
**Sprint**: 1
**Linked Requirements**: FR-005

**Files**: Create `items.rs`

**C Reference**: `g_items.c` (2,712 lines, 42 functions)

**ItemDef struct**: classname, pickup_fn, use_fn, drop_fn, weaponthink_fn,
pickup_sound, world_model, view_model, icon, pickup_name, quantity, ammo,
flags (IT_*), tag, precaches.

**Item categories**: 6 armor, 6 powerups, 11 weapons, 6 ammo, 2 utility.

**Key functions**: Pickup_Armor, Pickup_Health, Pickup_PowerArmor,
Pickup_Ammo, Pickup_Weapon, Use_Weapon, Drop_Weapon, Use_Quad,
Use_Invulnerability, Use_Breather, Use_Envirosuit, Use_Silencer,
SetItemNames, InitItems, PrecacheItem, SpawnItem, droptofloor,
Touch_Item (generic pickup trigger).

**Acceptance Criteria**:
- [ ] 31 items defined with correct classname, icon, model, quantity
- [ ] Pickup_Weapon adds weapon to inventory, switches if new
- [ ] Pickup_Ammo adds ammo, rejects if full
- [ ] Pickup_Armor absorbs damage correctly per type
- [ ] Use_Quad sets quad_framenum on player
- [ ] Drop_Weapon spawns dropped weapon entity
- [ ] DM respawn: item removed on pickup, respawns after delay
- [ ] Co-op: FL_COOP_TAKEN prevents re-pickup by same player

**Testing**:
- Unit: each item category has correct IT_* flags
- Unit: pickup weapon when inventory empty → weapon added, switched
- Unit: pickup ammo when full → rejected
- Unit: armor stacking rules correct
- Unit: item entity touch → calls correct pickup handler

---

### Sprint 2: World Entities (TASK-007 through TASK-011)

These tasks implement the world's interactive entities. They are partially
parallelizable — triggers, targets, and func_* are independent of each other
(all depend only on Sprint 1 foundations).

---

#### TASK-007: Weapon Fire System

**Description**: Implement 11 weapon fire functions and projectile entity spawning.

**Type**: Implementation
**Priority**: P1 (High)
**Estimate**: 8 points
**Dependencies**: TASK-005, TASK-006
**Sprint**: 2
**Linked Requirements**: FR-006

**Files**: Create `weapons.rs`

**C Reference**: `g_weapon.c` (1,231 lines), `player/weapon.c` (1,928 lines)

**Hitscan weapons**: fire_blaster, fire_shotgun, fire_supershotgun,
fire_machinegun, fire_chaingun, fire_railgun — use gi.trace() to detect hits.

**Projectile weapons**: fire_grenade, fire_rocket, fire_hyperblaster,
fire_bfg — spawn projectile entities with think/touch callbacks.

**Weapon state machine** (in player/weapon.rs, but logic shared):
idle → fire → cooldown, with ammo consumption and weapon switching.

**Acceptance Criteria**:
- [ ] Blaster fires hitscan trace, applies damage and spawns effect
- [ ] Shotgun fires spread pattern of N traces
- [ ] Rocket spawns entity with MOVETYPE_FLYMISSILE, touch → T_RadiusDamage
- [ ] Grenade spawns with MOVETYPE_BOUNCE, explodes after timer or on touch
- [ ] BFG fires projectile + per-frame laser damage to visible targets
- [ ] All weapons consume correct ammo quantity
- [ ] Empty ammo → weapon auto-switches

**Testing**:
- Unit: fire_blaster trace hits target → correct damage applied
- Unit: fire_rocket spawns entity with correct movetype and velocity
- Unit: grenade bounce → touch callback → T_RadiusDamage
- Unit: weapon switch on empty ammo

---

#### TASK-008: Trigger Entities

**Description**: Implement 11 trigger entity types.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 5 points
**Dependencies**: TASK-004, TASK-005
**Sprint**: 2
**Linked Requirements**: FR-007

**Files**: Create `triggers.rs`

**C Reference**: `g_trigger.c` (863 lines, 14 functions)

**Parallelizable with**: TASK-009, TASK-010

**Acceptance Criteria**:
- [ ] trigger_multiple fires on touch, respects wait time
- [ ] trigger_once fires and removes itself
- [ ] trigger_hurt applies damage per-tick
- [ ] trigger_push applies velocity to touching entities
- [ ] trigger_gravity changes gravity for entities inside
- [ ] trigger_key requires specific key item
- [ ] trigger_counter fires after N activations
- [ ] All trigger spawn functions registered in spawn table

**Testing**:
- Unit: trigger_multiple with wait=2, two touches < 2s → only one fire
- Unit: trigger_hurt deals correct damage per second
- Unit: trigger_push velocity application

---

#### TASK-009: Target Entities

**Description**: Implement 17 target entity types.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 5 points
**Dependencies**: TASK-003, TASK-005
**Sprint**: 2
**Linked Requirements**: FR-008

**Files**: Create `targets.rs`

**C Reference**: `g_target.c` (1,234 lines, 36 functions)

**Parallelizable with**: TASK-008, TASK-010

**Acceptance Criteria**:
- [ ] target_explosion creates explosion effect and applies radius damage
- [ ] target_changelevel sets level exit with next map
- [ ] target_speaker plays sound at entity origin
- [ ] target_laser traces beam and damages per-frame
- [ ] target_earthquake shakes player views
- [ ] All target spawn functions registered in spawn table

**Testing**:
- Unit: target_explosion damage and effect
- Unit: target_changelevel sets exit fields
- Unit: target_laser traces and damages

---

#### TASK-010: Functional Entities (Doors, Platforms, Trains)

**Description**: Implement 16 func_* entity types. This is the largest single
task — `g_func.c` is 3,012 lines with 72 functions.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 13 points
**Dependencies**: TASK-002 (MoveInfo), TASK-004 (physics push)
**Sprint**: 2
**Linked Requirements**: FR-009

**Files**: Create `func.rs`

**C Reference**: `g_func.c` (3,012 lines)

**Parallelizable with**: TASK-008, TASK-009

**Movement pattern**: All func_* entities use MoveInfo for smooth
acceleration/deceleration between start and end positions. The pattern is:
`Move_Begin → Move_Final → Move_Done`, with `endfunc` callback.

**Key entity types**:
- func_plat: elevator platform, triggered by player standing on it
- func_door / func_door_secret / func_door_rotating: doors with teams
- func_button: button that triggers targets
- func_train: follows path_corner waypoint chain
- func_rotating: continuous rotation
- func_water: rising/lowering water
- func_timer: periodic target activation

**Acceptance Criteria**:
- [ ] func_plat descends when player steps on, returns after wait
- [ ] func_door opens on trigger, team doors open together
- [ ] func_door_rotating rotates open around specified axis
- [ ] func_train follows path_corner chain with speed/wait per corner
- [ ] func_button presses in on use, fires targets, returns after wait
- [ ] func_explosive breaks on damage threshold, spawns debris
- [ ] func_killbox kills everything inside when triggered
- [ ] MoveInfo accel/decel curves produce smooth movement

**Testing**:
- Unit: MoveInfo acceleration curve from speed=0 to speed=100 over distance
- Unit: func_door state machine: closed → opening → open → closing → closed
- Unit: func_train waypoint chain traversal
- Unit: func_plat triggered by player touch

---

#### TASK-011: Miscellaneous Entities

**Description**: Implement ~23 misc entities and the path_corner waypoint
entity.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 5 points
**Dependencies**: TASK-003, TASK-004
**Sprint**: 2
**Linked Requirements**: FR-010

**Files**: Create `misc.rs`

**C Reference**: `g_misc.c` (2,726 lines)

**Key entities**: misc_explobox, misc_banner, misc_teleporter/dest,
misc_blackhole, misc_gib_arm/leg/head, misc_deadsoldier, misc_viper,
misc_strogg_ship, misc_eastertank/easterchick, path_corner, point_combat,
viewthing, light, info_null, info_notnull.

**Acceptance Criteria**:
- [ ] misc_explobox explodes on damage, radius damage to surroundings
- [ ] misc_teleporter teleports touching entities to dest
- [ ] path_corner entities form chains for func_train
- [ ] misc_gib_* entities are throwable gibs with MOVETYPE_BOUNCE
- [ ] All misc spawn functions registered in spawn table

**Testing**:
- Unit: misc_explobox damage → explosion → radius damage
- Unit: misc_teleporter moves entity to dest origin
- Unit: path_corner chain resolution

---

### Sprint 3: AI, Player, and Integration (TASK-012 through TASK-016)

These tasks build on the world entity framework. AI and player are the most
complex subsystems but can be partially parallelized.

---

#### TASK-012: Base AI System

**Description**: Implement the 20 core AI functions for monster behavior.

**Type**: Implementation
**Priority**: P1 (High)
**Estimate**: 8 points
**Dependencies**: TASK-004 (physics/step), TASK-005 (combat)
**Sprint**: 3
**Linked Requirements**: FR-011

**Files**: Create `ai.rs`, create `monster/mod.rs`

**C Reference**: `g_ai.c` (1,328 lines), `g_monster.c` (1,086 lines)

**AI functions**: AI_SetSightClient, ai_stand, ai_walk, ai_run, ai_charge,
ai_move, ai_turn, M_MoveToGoal, M_walkmove, M_MoveFrame, M_SetEffects,
FindTarget, FoundTarget, HuntTarget, infront, visible, FacingIdeal.

**Monster lifecycle**: monster_start (sets defaults) → monster_start_go
(after spawn delay) → monster_triggered_start (if triggered only) →
AI state machine loop.

**Animation system**: `MonsterMove` (mmove_t) specifies frame range +
per-frame callback + end callback. `M_MoveFrame` advances through the
current animation.

**Acceptance Criteria**:
- [ ] AI_SetSightClient selects a random player for monsters to check
- [ ] visible() traces line-of-sight between monster and target
- [ ] FindTarget scans for visible players and transitions to run state
- [ ] ai_run moves toward enemy using M_MoveToGoal
- [ ] M_MoveFrame advances animation and calls per-frame callbacks
- [ ] monster_start initializes common monster defaults (health, flags, etc.)

**Testing**:
- Unit: visible() with clear sight → true; with blocked trace → false
- Unit: ai_stand with no enemy → stays in stand animation
- Unit: FindTarget with player in LOS → sets enemy, calls sight callback
- Unit: M_MoveFrame advances frame and calls callback
- Integration: spawn monster, set enemy, run ai_run → monster moves toward enemy

---

#### TASK-013: Monster Implementations — Tier 1 (Soldier template + 5 monsters)

**Description**: Port soldier (3 variants) as the template monster, then
infantry, gunner, gladiator, berserker. These are the most common enemies
and establish the pattern for all others.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 13 points
**Dependencies**: TASK-012
**Sprint**: 3
**Linked Requirements**: FR-012

**Files**: Create `monster/soldier.rs`, `monster/infantry.rs`,
`monster/gunner.rs`, `monster/gladiator.rs`, `monster/berserker.rs`

**C Reference**: `monster/soldier/` (1,784 lines), `monster/infantry/` (949),
`monster/gunner/` (1,071), `monster/gladiator/` (555), `monster/berserker/` (507)

**Soldier as template**: Three variants (light=blaster, normal=shotgun,
ss=machinegun) sharing the same state machine with different attack functions.
This establishes the pattern: define animation frames, register state
callbacks, implement attack/pain/die.

**Acceptance Criteria**:
- [ ] Soldier spawns, enters stand state with correct animation
- [ ] Soldier spots player → transitions to run → approaches
- [ ] Soldier in range → fires weapon (blaster/shotgun/machinegun per variant)
- [ ] Soldier takes damage → pain animation → returns to run
- [ ] Soldier dies → death animation → SVF_DEADMONSTER
- [ ] Infantry, gunner, gladiator, berserker follow same patterns
- [ ] All 8 spawn entries (3 soldier + 5 others) registered

**Testing**:
- Unit: per-monster spawn → correct initial state
- Unit: soldier attack fires correct weapon for variant
- Integration: monster spot + attack sequence (multi-frame)

---

#### TASK-014: Monster Implementations — Tier 2 (Remaining 15 monsters)

**Description**: Port the remaining 15 monster types. These are independent
of each other and can be done in any order using the soldier template as
reference.

**Type**: Implementation
**Priority**: P2 (Medium)
**Estimate**: 13 points
**Dependencies**: TASK-013 (uses soldier as template)
**Sprint**: 3 (can overflow into Sprint 4)
**Linked Requirements**: FR-012

**Files**: Create remaining `monster/*.rs` files (brain, chick, flipper,
floater, flyer, hover, medic, mutant, parasite, insane, tank, supertank,
boss2, boss3, turret)

**C Reference**: `monster/` remaining directories (~13,000 lines total)

**Parallelization note**: Each monster file is completely independent. If using
subagent-driven development, all 15 can be implemented in parallel.

**Acceptance Criteria**:
- [ ] All 20 monster types spawn and enter stand state
- [ ] Each monster has working attack, pain, and die sequences
- [ ] Boss monsters (boss2, boss3/makron/jorg) have multi-phase fights
- [ ] Turret entities (breach, base, driver) work as a composite
- [ ] All 22 monster spawn entries registered in spawn table

**Testing**:
- Unit: per-monster spawn + 1 AI frame without panic
- Unit: boss3 phase transition (jorg → makron)

---

#### TASK-015: Player Subsystem

**Description**: Implement player client management, weapon state machine,
view effects, HUD stats, and player trail.

**Type**: Implementation
**Priority**: P1 (High)
**Estimate**: 13 points
**Dependencies**: TASK-005 (combat), TASK-006 (items), TASK-007 (weapons)
**Sprint**: 3
**Linked Requirements**: FR-013

**Files**: Create `player/mod.rs`, `player/client.rs`, `player/weapon.rs`,
`player/view.rs`, `player/hud.rs`, `player/trail.rs`

**C Reference**: `player/client.c` (2,501), `player/weapon.c` (1,928),
`player/view.c` (1,426), `player/hud.rs` (657), `player/trail.c` (175)

**Key functions**:
- ClientConnect / ClientBegin / ClientDisconnect
- ClientThink (process UserCmd each frame)
- PutClientInServer (respawn with defaults)
- ClientBeginServerFrame / ClientEndServerFrame
- Think_Weapon (weapon state machine dispatch)
- SV_CalcViewOffset (view bob, damage kick, fall effect)
- G_SetStats (write stats to player_state_t.stats[])
- PlayerTrail_Add / PlayerTrail_Pick / PlayerTrail_LastSpot

**Acceptance Criteria**:
- [ ] ClientConnect allocates player entity with default persistent data
- [ ] ClientThink processes UserCmd, runs pmove, updates entity state
- [ ] Weapon state machine: idle → fire → cooldown → idle
- [ ] View bob oscillates smoothly with movement speed
- [ ] Damage kick rotates view temporarily
- [ ] HUD stats (health, ammo, armor, weapon icon) set correctly
- [ ] Player trail records nodes for monster AI

**Testing**:
- Unit: ClientConnect → entity allocated, persistent data initialized
- Unit: Think_Weapon fire → cooldown → idle transitions
- Unit: G_SetStats populates correct stat indices
- Unit: PlayerTrail stores and retrieves trail nodes

---

#### TASK-016: Game Main Loop and GameExport Trait

**Description**: Implement `G_RunFrame`, `InitGame`, `ShutdownGame`, and the
`GameExport` trait implementation for `GameWorld`. Wire the spawn table
expansion to 107 entries.

**Type**: Implementation
**Priority**: P0 (Critical)
**Estimate**: 5 points
**Dependencies**: TASK-004, TASK-012, TASK-015
**Sprint**: 3
**Linked Requirements**: FR-014, FR-016

**Files**: Modify `world.rs` (add run_frame, init, shutdown, GameExport impl),
modify `spawn.rs` (register all 107 spawn entries)

**C Reference**: `g_main.c` (514 lines)

**G_RunFrame sequence**:
1. `level.framenum += 1; level.time = framenum * FRAMETIME`
2. `AI_SetSightClient`
3. Check `level.exitintermission`
4. For each entity: save old_origin, check ground staleness, dispatch
   players to `ClientBeginServerFrame`, others to `G_RunEntity`
5. `CheckDMRules`, `CheckNeedPass`, `ClientEndServerFrames`

**Acceptance Criteria**:
- [ ] G_RunFrame increments level.time correctly
- [ ] All entity types dispatched correctly (players vs world entities)
- [ ] GameExport::spawn_entities parses entity string and spawns all 107 types
- [ ] GameExport::init allocates storage, registers cvars
- [ ] GameExport::client_connect/begin/disconnect delegate to player module
- [ ] GameExport::run_frame delegates to G_RunFrame
- [ ] CP-2: spawn player + soldier → soldier stands → run 1 AI frame → no panic

**Testing**:
- Unit: run_frame with empty world → no panic, time advances
- Unit: run_frame with 3 entities → each gets G_RunEntity call
- Integration: spawn_entities with real entity string → entities created
- **CP-2 Integration**: spawn player + soldier + run 10 frames

---

### Sprint 4: Save/Load and Polish (TASK-017 through TASK-018)

---

#### TASK-017: Save/Load System

**Description**: Implement serde-based game state serialization replacing
the C function-pointer-to-string lookup tables.

**Type**: Implementation
**Priority**: P3 (Low — not needed for gameplay testing)
**Estimate**: 8 points
**Dependencies**: TASK-016
**Sprint**: 4
**Linked Requirements**: FR-015

**Files**: Create `savegame/mod.rs`, `savegame/serialize.rs`,
`savegame/registry.rs`

**C Reference**: `savegame/` (3,170 lines)

**Callback registry**: Map callback function names (strings) to fn pointers
for serialization. On save: look up fn pointer → get name. On load: look up
name → get fn pointer. This is the Rust equivalent of the C
`functionList_t` table.

**SlotMap key stability**: EntityKey contains version counters. On save,
serialize cross-references (owner, enemy, etc.) as integer indices. On load,
rebuild index→EntityKey mapping table after re-inserting entities.

**Acceptance Criteria**:
- [ ] GameWorld serializes to binary via serde+bincode
- [ ] GameWorld deserializes and all entities restore correctly
- [ ] Entity cross-references (owner, enemy) survive round-trip
- [ ] Callback functions survive round-trip via name registry
- [ ] Save file from one session loads in another

**Testing**:
- Unit: serialize/deserialize empty GameWorld
- Unit: round-trip GameWorld with 10 entities and cross-references
- Unit: callback registry resolves names correctly
- Unit: SlotMap key rebuilding after load

---

#### TASK-018: Integration Testing and Cleanup

**Description**: Write comprehensive integration tests, verify CP-2 checkpoint,
clean up clippy warnings, verify WASM compatibility.

**Type**: Testing
**Priority**: P1 (High)
**Estimate**: 5 points
**Dependencies**: TASK-016
**Sprint**: 4
**Linked Requirements**: NFR-001 through NFR-005

**Acceptance Criteria**:
- [ ] `cargo test -p q2-game` passes with 200+ tests
- [ ] `cargo clippy -p q2-game` clean (zero warnings)
- [ ] `cargo check -p q2-game --target wasm32-unknown-unknown` passes
- [ ] Zero `unsafe` blocks in q2-game
- [ ] CP-2 checkpoint: spawn player + monster_soldier, run 10 frames, no panic
- [ ] Multi-entity integration: rocket → explosion → radius damage → monster dies
- [ ] Door team integration: trigger → 3 linked doors open simultaneously

**Testing**:
- Integration: full combat scenario (player fires rocket at soldier)
- Integration: level entity string with mixed entity types
- Integration: DM item respawn cycle
- Fidelity: key constants verified against C source

---

## Dependency Graph

```
TASK-001: Constants
    │
    ├─▶ TASK-002: Entity Callbacks + MoveInfo/MonsterInfo
    │       │
    │       ├─▶ TASK-003: GameWorld + Utilities
    │       │       │
    │       │       ├─▶ TASK-004: Physics ──────────────────────┐
    │       │       │       │                                    │
    │       │       │       ├─▶ TASK-008: Triggers ──────────┐   │
    │       │       │       ├─▶ TASK-011: Misc Entities ──┐  │   │
    │       │       │       └─▶ TASK-012: Base AI ─────┐  │  │   │
    │       │       │               │                  │  │  │   │
    │       │       ├─▶ TASK-005: Combat ──────────┐   │  │  │   │
    │       │       │       │                      │   │  │  │   │
    │       │       │       ├─▶ TASK-009: Targets  │   │  │  │   │
    │       │       │       └─▶ TASK-006: Items    │   │  │  │   │
    │       │       │               │              │   │  │  │   │
    │       │       │               └─▶ TASK-007: Weapons  │  │   │
    │       │       │                       │      │   │  │  │   │
    │       │       │                       ▼      ▼   ▼  ▼  ▼   ▼
    │       │       │              TASK-015: Player TASK-013: Monsters T1
    │       │       │                       │              │
    │       │       │                       ▼              ▼
    │       │       │              TASK-016: Main Loop  TASK-014: Monsters T2
    │       │       │                       │
    │       ├─▶ TASK-010: Func Entities ────┘
    │       │                               │
    │       │                               ▼
    │       │                      TASK-017: Save/Load
    │       │                      TASK-018: Integration Tests
```

### Critical Path

```
TASK-001 → TASK-002 → TASK-003 → TASK-004 → TASK-012 → TASK-013 → TASK-016 → TASK-018
   2pt       5pt       5pt       8pt        8pt        13pt        5pt        5pt
                                                                          = 51 points
```

### Parallelization Opportunities

| Parallel Group | Tasks | Condition |
|---------------|-------|-----------|
| **Group A** (Sprint 1) | TASK-004, TASK-005 | Both depend only on TASK-003 |
| **Group B** (Sprint 2) | TASK-008, TASK-009, TASK-010, TASK-011 | All depend only on Sprint 1 |
| **Group C** (Sprint 2-3) | TASK-007, TASK-012 | TASK-007 needs 005+006; TASK-012 needs 004+005 |
| **Group D** (Sprint 3) | TASK-013, TASK-015 | Independent once their deps land |
| **Group E** (Sprint 3) | TASK-014 (all 15 monsters) | Each monster file is independent |

---

## Sprint Schedule

### Sprint 1: Core Framework

**Goal**: All foundational systems operational — entities, physics, combat, items
**Tasks**: TASK-001 through TASK-006
**Total Points**: 33
**Deliverable**: `cargo test -p q2-game` with entity callbacks, physics, combat, items
**Risk**: Physics fidelity — may need careful cross-referencing with C for SV_Push
**Checkpoint**: entities move, take damage, pick up items

### Sprint 2: World Entities

**Goal**: All world entity types operational — triggers, targets, doors, platforms
**Tasks**: TASK-007 through TASK-011
**Total Points**: 36
**Deliverable**: Full world interactivity — doors open, triggers fire, weapons work
**Risk**: func.rs size (3,012 C lines → ~2,000 Rust) — largest single file
**Parallelization**: TASK-008/009/010/011 can run concurrently
**Checkpoint**: entity string with doors + triggers → interactive world

### Sprint 3: AI, Player, Integration

**Goal**: Monsters and players fully operational, game loop runs
**Tasks**: TASK-012 through TASK-016
**Total Points**: 52
**Deliverable**: **CP-2** — spawn player + soldier, soldier fights, game loop ticks
**Risk**: Monster volume (20 types, ~13,000 C lines) — mitigated by parallelization
**Parallelization**: All monster files independent, can be subagent-parallelized
**Checkpoint**: CP-2 passing

### Sprint 4: Save/Load and Polish

**Goal**: Save/load works, all integration tests pass, WASM-compatible
**Tasks**: TASK-017 through TASK-018
**Total Points**: 13
**Deliverable**: 200+ tests, zero clippy warnings, WASM check passes
**Risk**: Callback registry complexity for save/load
**Checkpoint**: all success criteria from specification met

---

## Risk Assessment

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Physics fidelity (SV_Push, SV_FlyMove) | High | Medium | Cross-reference every constant against C source; write C-parity tests |
| func.rs size (3,012 C lines) | Medium | High | Break into sub-functions; implement door/plat/train separately, merge |
| Monster volume (20 types, 19K LOC) | Medium | Low | Highly parallelizable; use soldier as template; each file is independent |
| Entity borrow conflicts (&mut GameWorld) | High | Medium | GameWorld owns all state; callbacks take &mut GameWorld + EntityKey, not &mut Entity |
| Callback serialization (save/load) | Medium | Medium | Build registry early (TASK-002); test round-trip in TASK-017 |
| WASM compatibility | Low | Low | Run `cargo check --target wasm32-unknown-unknown` after each sprint |
| Scope creep from C edge cases | Medium | High | Port mainline behavior first; add edge cases as follow-up issues |

---

## Success Metrics

- [ ] 18 tasks completed
- [ ] 200+ tests passing in q2-game
- [ ] Zero `unsafe` blocks
- [ ] Zero clippy warnings
- [ ] WASM target compiles
- [ ] CP-2 checkpoint passes
- [ ] All 107 spawn table entries registered
- [ ] `GameExport` trait fully implemented

---

## Next Steps

1. Review this plan
2. Start Sprint 1 with `TASK-001` (constants)
3. Execute using `/attune:execute` or `/superpowers:executing-plans`
4. After CP-2 passes, update `docs/superpowers/plans/2026-03-26-c-to-rust-conversion.md`
   progress table to mark Phase 2 as DONE
