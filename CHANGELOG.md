# Changelog

All notable changes to qwasm2-rs are documented here.

## [Unreleased]

### Changed (breaking)
- `GameImport::set_model` returns `ModelInfo` (index and inline-model
  bounds); `Pmove::ground_entity` is the entity number (`Option<usize>`)
  and `Pmove` gains `touchents`; `Renderer` no longer requires `Send` and
  gains `draw_get_pic_size`; `RefDef` gains `lightstyles`
  (`[f32; MAX_LIGHTSTYLES]`)
- From the full review: `CVarSystem::get` returns `Option`; failed
  `NetMsg` reads advance the cursor like C and `read_float` past the end
  is -1.0; the zone module, `num_models`, `reliable_pending`, the panicking
  `read_*` helpers, `Server::models` and `lerp_entity` are removed;
  `fire_blaster` takes C's `effect`; `sv_new_chase_dir` takes a goal
  position. New: `q2_client::frame::FrameHistory`,
  `q2_server::ents::build_client_frame`, `LocalGame::take_map_change`,
  `ServerGameImport::{take_commands, client_phs, entity_in_phs}`,
  `ClientPersistent::ammo_max`, `deathtarget`, `GameWorld::trail`.
  Content/mask/effect/area constants live once, in q2-shared
- `GameExport::init` takes no import (`GameWorld::new` receives an
  `Rc<dyn GameImport>`), client calls take entity numbers, and
  `GameImport::link_entity` takes an `EntityLink`; `GameLogic`,
  `PlayerController` and `q2_game::spawn::find_player_start` are removed
- `Server::map` takes the BSP bytes, the import and the game
- Entity and usercmd encoding now match C protocol 34 byte for byte (8-bit
  entity angles, FRAME8/FRAME16 as in C, `event` sent whenever non-zero,
  usercmd `msec`/`lightlevel` written last); new entities delta from
  baselines. The client rejects any other protocol version
- `Server::frame` takes `&ServerGameImport` and runs game frames at 10 Hz;
  server time is `u32` milliseconds and `frametime` is gone
- `GameLogic::new(max_clients)` replaces `Default`; `GameImport`,
  `GameExport` and `PakReader` no longer require `Send + Sync`
- `parse_server_message`, `write_delta_entity`,
  `write_packet_entities_list`, `read_dir`, `parse_entity_string` and
  `calc_fov` return `Q2Result`; `NetChan::new` takes a source and qport
- `Q2Error`/`Q2Result` are defined in `q2_shared::error` (still re-exported
  from `q2_common::error`); `Renderer::init` and the q2-platform wasm
  functions return `Q2Result` instead of `Result<_, String>`
- `CollisionMap::areas_connected` and `set_area_portal_state` return
  `Q2Result` and reject out-of-range indices like C ERR_DROP
- BSP header constants and lump directory live in `q2_common::bsp_file`;
  `q2_render::bsp` no longer exports its own `LUMP_*`/`BspLump`, and
  `BspData::entities` is gone (use `CollisionMap::entity_string`)
- The server reads the game only through `GameExport`, which gains
  `num_edicts`, `edict(n) -> Option<EdictView>` and
  `client_player_state(n)`; `LocalGame` holds a `Box<dyn GameExport>`,
  `LocalGame::game()` and `FrameEntity` are removed, `entity_count` is
  `num_edicts`, and `Server::map` takes a spawnpoint
- `Gl3Renderer::new(gl, loader)` replaces `set_gl_context`,
  `set_file_loader` and `load_bsp`; `Renderer::begin_registration`
  returns `Q2Result` and loads the map's world; `register_image` returns
  real handles and `end_registration` frees untouched models and skins
- Monster moves are per-frame tables: `MonsterMove.frames` holds
  `MonsterFrame { aifunc, dist, thinkfunc }`; `frame_fn`/`dist` are gone
- Spawn data is one table, `spawn::SPAWNS`; the `register_*_spawns`
  methods, `classname_model` and `SpawnTable::insert` are removed;
  `ClientPersistent::weapon`/`last_weapon` are item indices
- `Entity::client` is the client's slot in `GameWorld::clients`
  (`world.client(key)`/`client_mut`); `GameImport` gains
  `positioned_sound`, `set_area_portal_state`, `areas_connected` and
  `entity_areas`; `Renderer` gains `begin_frame` and `light_level`, and
  `RefDef` gains `areabits`; `ServerState::Pic`, `LocalGame::{picture,
  take_sounds, area_bits, player_area}`; monster constants and
  `gib_health` are shared (`STUB_STAND_FRAMES` removed)

### Added
- The player fires the blaster: `ClientThink` latches the attack button
  and the weapon runs C's `Think_Weapon`/`Weapon_Generic` cycle (raise,
  fire, idle with random pauses, lower and `ChangeWeapon`), with muzzle
  flash, view kick and the no-ammo click and auto-switch; `ItemDef` gains
  `view_model` and `weaponthink` (#16, first slice)
- All ten weapons and hand grenades fire as in C (`Weapon_*`): shotgun
  and super shotgun spreads, the machinegun climb, chaingun spin-up,
  hyperblaster loop, rocket, grenade launcher, railgun, the BFG windup and
  cooked hand grenades that go off in the hand; picking up a new weapon
  switches to it, the weapon hum plays on the player and the BFG's pitch
  kick fades over `DAMAGE_TIME`
- Weapon and inventory commands (C `ClientCommand`): `use`, `drop`,
  `weapnext`, `weapprev`, `weaplast`, `invnext`/`invprev` (and the `w`/`p`
  variants), `invuse`, `invdrop`, with C's messages; the browser binds
  them like Q2's `default.cfg` (1-0 weapons, `[` `]` `'` Enter);
  `LocalGame::client_command` runs one, and `ServerGameImport` answers
  `argv`/`args` like `Cmd_TokenizeString`
- The first-person gun is drawn (C `CL_AddViewWeapon`): `ps.gunindex` at
  the eye with lerped `gunoffset`/`gunangles` and frames, in C GL3's own
  80 degree gun projection and the front 30% of the depth range
  (`RF_DEPTHHACK`) so it never pokes into walls; `RF_MINLIGHT` and
  `RF_DEPTHHACK` join `q2_shared::constants`
- Powerups and power armor can be used (C `Use_Quad`,
  `Use_Invulnerability`, `Use_Breather`, `Use_Envirosuit`,
  `Use_Silencer`, `Use_PowerArmor`) and dropped (`Drop_General`,
  `Drop_PowerArmor`); `Pickup_Powerup` keeps C's skill and coop limits,
  power screens and shields can be picked up, deathmatch items respawn
  (`SetRespawn`/`DoRespawn`) and instant items and dropped quads are used
  on pickup; touching an item plays its pickup sound (health by size)
  and selects it when usable
- Muzzle flashes (C `CL_AddMuzzleFlash`/`CL_AddMuzzleFlash2`): the
  player's and monsters' gunfire is heard (C plays those sounds on the
  client, so they were silent) and lights the scene; the server hands
  multicast messages to `LocalGame::take_messages`, the client keeps C's
  dlight list (`q2_client::effects`), and lit world faces take C GL3's
  dynamic lights; `monster_flash_offset` moves to `q2_shared::flash`
- Grenades: blasts record C's means of death by kind (launcher, hand,
  held) in `GameWorld::means_of_death`, set by every `t_damage` like C
  `meansOfDeath`; bouncing grenades clatter (`grenlb1b`, or one of two
  hand grenade sounds) and launched grenades get C's random lift and
  drift
- Gamma is tunable: `?gamma=1.2` sets `vid_gamma` at load and F7/F8 step
  it by 0.1 in game, within C's brightness slider range 0.1..2.0; the
  shaders read it as C's `gamma` uniform (`1/vid_gamma`) through the new
  `Renderer::set_gamma` (breaking for `Renderer` implementors)
- All 20 monsters ported from their C files: move tables, sight, attacks,
  pain, death, gibs and sounds, on the full `g_ai.c`/`g_monster.c`,
  `monster_fire_*` with `m_flash.c` offsets, and the medic's resurrection
- Sound: game sounds and entity loop sounds play in the browser through
  WebAudio (WAV with cue loop points, C spatialization and channels);
  doors, plats, buttons, trains and water play their sounds; door
  messages print
- Area portals: doors open and close them, the server drops entities in
  closed-off areas, and the renderer skips the world behind a shut door
- Monsters hear the player fire (`PlayerNoise`) and can't see a player in
  the dark (`light_level`)
- Coop and deathmatch branches of persistence, spawning, spawn points,
  drops and the body queue (reachable from tests; the browser is single
  player); power armor; per-frame gib and debris caps
- Picture levels: demo2's exit shows `victory.pcx` as C's `ss_pic`
- Level exits load the next map: `gamemap` is parsed like C `SV_Map`
  (`map$spawnpoint`, `*`, `+`), the game keeps running and the player
  keeps inventory, health and weapon (C `SaveClientData`/
  `FetchClientEntData`); the browser re-registers models and sky
- Monsters drop their map `item` on death (C `Drop_Item`,
  `monster_death_use`)
- `func_train` follows its path corners (wait, pathtarget, teleport,
  toggle, block-stops), `func_door_rotating`, `func_door_secret`,
  `func_water`, `func_conveyor` and `trigger_elevator` move as in C
- The soldier's stand, walk, run, pain and death moves match
  `m_soldier.c` frame by frame, at C's model scale 1.2
- CI fetches the demo `pak0.pak` (cached by checksum) so map-data tests
  run, and the browser suite loads demo1 in real WebGL; PVS culling has a
  synthetic multi-cluster fixture; the PHS is loaded for `in_phs`
- Brush entities get their inline-model bounds and collide; doors,
  plats and buttons port C's movers, touch fields and `Move_Calc`
- `ClientThink` fires `G_TouchTriggers` and pmove's touch list, and
  records the entity stood on (#12)
- Map items spawn, drop to the floor and can be picked up; monsters and
  misc objects carry their C models
- Worldspawn publishes the status bar, sky, item names and light styles;
  `G_SetStats` fills health, armor and pickup stats
- q2-client: HUD layout interpreter (`SCR_ExecuteLayoutString`) and
  entity interpolation (`CL_AddPacketEntities`)
- q2-render: PCX, WAL, TGA, MD2 and SP2 parsers; the world drawn textured
  and lightmapped with sky, warp and translucent surfaces; brush entities,
  lit and interpolated MD2 models, and 2D pics and conchars (#20)
- Light styles animate: the client turns `CS_LIGHTS` into per-frame
  brightness (`RefDef::lightstyles`), world faces blend up to four styled
  lightmaps and MD2 lighting follows the same styles; texture animation
  chains cycle at 2 fps on the world and by frame on brush entities (#20)
- The browser draws entities and the status bar every frame
- The browser runs an in-process server with the full game
  (`q2_server::local::LocalGame`): the map's entities spawn and think, and
  the player moves with pmove in `ClientThink` through `SV_Trace`
- `ServerGameImport` loads the map and implements C `SV_Trace`,
  `SV_PointContents`, entity linking and `SV_FindIndex`;
  `Server::map` follows `SV_SpawnServer`
- Entities carry C edict numbers with `G_Spawn` reuse and `G_FreeEdict`
  soft-free; spawning applies `ED_ParseEdict` fields and
  `SelectSpawnPoint`
- `CollisionMap::world_trace`, which reports the world entity on a hit
  like C CL_PMTrace, and `clip_move_to_entities` (C CL_ClipMoveToEntities)
  for client-side clipping against frame entities; unwired until client
  prediction (#19)
- `q2_common::bsp_file` decodes plane, node, leaf, texinfo and model
  records for both the collision and render loaders
- The wasm engine refits render size and fov when the canvas is resized

### Fixed
- `BossExplode` ran a ninth explosion before gibbing; the click prompt
  swallowed the click that locks the pointer and starts audio
- Soldier animations used the wrong model frames for stand, walk, pain
  and death; `M_MoveFrame` clears a stale `nextframe` like C
- Monsters walk like C (`m_move.c` port): animations switch to the new
  move's first frame, they turn at `yaw_speed`, chase with C's random
  chase directions, collide with walls and ledges, and follow the player
  trail after losing sight. Before, they slid in a straight line through
  walls playing the wrong frames
- The world was culled inside-out (now `GL_FRONT` like C) and gamma is
  C's default 1.0
- Full review (256 findings, ledgers in the review notes): shots and
  monster moves use C's masks so they hit monsters and players; freed
  entities stop thinking; level exits queue `gamemap` instead of freezing;
  `use_targets` cannot recurse into itself; ground is cleared and items
  come to rest like C; armor, health and ammo pickups follow C's caps;
  `SV_Push` and door/train teams are ported; `monster_berserk`,
  `misc_insane` and the coop/intermission spawns resolve; the game clock
  keeps sub-millisecond time (it ran 4-13% slow); client interpolation
  resets on teleports, model changes and long jumps; collision, box-leaf
  and PVS walks no longer recurse (no stack overflow on deep trees);
  malformed TGA/MD2/BSP/pak data is rejected instead of panicking or
  over-allocating; palette index 255 and skins get C's edge fixes
- Browser frame rate: MD2 and 2D vertex uploads no longer convert every
  float to bytes one at a time (71% of a debug frame; 18 -> 60 fps on an
  Apple M4 Pro), and `make play` builds optimized WASM (`make play-dev`
  keeps the debug build)
- Only entities in the player's PVS are sent to the renderer
  (`SV_BuildClientFrame`, `SV_FatPVS`, `SV_LinkEdict` clusters; the map's
  visibility lump is now loaded), and MD2 models outside the view frustum
  are skipped (`CullAliasModel`); demo1's start sends 16 of 133 entities
- Netchan rejects runt packets, reserves header space when staging
  reliable data and sets `fatal_error` on overflow
- Client parsing no longer hangs on a truncated frame, stores a partial
  frame as valid, or accepts negative or unsorted entity numbers
- BSP loading rejects out-of-range or cyclic references and truncated
  lumps instead of panicking or looping (child index must exceed parent,
  which is stricter than C); an empty map no longer hangs the box hull
- Filesystem rejects absolute and `..` paths, bounds PAK sizes before
  allocating, and lets PAKs override loose files as C does
- pmove numerics follow C (double-precision AngleVectors, reciprocal
  normalize, 0.7 slope test, wrapping snap, runtime air acceleration,
  fly-move early return); PlayerController walks at 200 and runs at 300
- Cvar and command parsing: userinfo injection, numeric prefix parse,
  newline always ends a command, buffer and token limits
- Map change clears configstrings and baselines; game-written
  configstrings reach the server; out-of-range client slots are an error
- Renderer: checked BSP casts, GL objects freed on reload and shader
  errors, C pitch sign and submodel bounds; `calc_fov` rejects fov outside
  1..179
- `make test-browser` ran `cd tests/browser` twice and never reached
  Playwright; it now installs locked npm deps and runs the suite
- `q2-pak-repack` reports Brotli write errors (the stream was finished in
  `Drop`, which discards them), writes outputs via temp file + rename,
  removes a stale `.br` when run without `--brotli`, and rejects output
  over the 4 GiB PAK offset limit instead of truncating offsets
- `q2-devserver` honours `br;q=0`, sends `Vary: Accept-Encoding`, streams
  the pak instead of reading it per request, and skips a `.br` older than
  its pak; an invalid `PORT` is an error
- `q2-bundler` fails when wasm-bindgen glue no longer matches its rewrites
  instead of emitting a page that fetches a missing `.wasm`; page errors
  are inserted as text, not HTML
- Undocumented `unsafe` blocks are denied again (a rustflags entry had
  downgraded the workspace lint to a warning)

### Changed
- CI runs `make fmt-check lint check test` with `--locked`, clippy on all
  host targets and on wasm32 libraries, `cargo audit`, and the browser
  suite; the toolchain comes from `rust-toolchain.toml` (1.97.1)
- `make pak-web` is a file rule and only reruns when `pak0.pak` or the
  repacker changes; `make play` no longer requires 7z/curl/python3 when
  `pak0.pak` exists; demo download is checked with `curl --fail` and SHA-256
- `q2-devserver` no longer sends permissive CORS headers

### Removed
- Unused dependencies: `matchbox_socket` (q2-net), every q2-bin dependency,
  workspace `cpal`/`lewton`; this removed all 19 `cargo audit`
  vulnerabilities
- `q2-pak-repack --all` (never read; keeping every file is the default),
  the unused `PAK_WEB_ALLOW` variable, `make serve`, `make prereq-python3`,
  `make prereq-pak-repack`

## [0.1.2] - 2026-05-01

### Added
- Q2 wire-protocol delta machinery: `parse_frame`, player-state delta, packet-entity delta (30 tests) — CP-4 client delta
- `PakReader` trait with lazy WASM backend — avoids copying the full PAK into linear memory until individual assets are requested
- Server frame loop with `GameImport`/`GameExport` bridge replacing C function-pointer tables — CP-3 complete
- CI badge in README
- `q2-pak-repack` crate: CLI tool that repacks a Q2 PAK, optionally filtered
  by an extension allowlist (`make pak-web` keeps every file), and writes a
  Brotli-compressed `.br` variant (level 11) for web delivery
- `make pak-web` target: produces `pak0-web.pak` and `pak0-web.pak.br` from
  the downloaded demo data; integrated into `make play` and `make play-release`
- Devserver Brotli content negotiation: `q2-devserver` now serves
  `pak0-web.pak.br` when the client sends `Accept-Encoding: br`, reducing
  wire size from ~46 MB to ~26 MB

### Changed
- `collision.rs`: major refactor for correctness and clippy compliance
- `gl3/mod.rs`: renderer improvements
- `game_iface.rs`: expanded `GameImport` bridge implementation
- `player_ctrl.rs` / `pmove.rs`: player movement refinements
- README updated to reflect CP-3 completion and CP-4 in-progress status

## [0.1.1] - 2026-03-26

### Added
- Entity spawn system (`q2-game`)
- `GameExport` trait wired to server
- BSP map loading and WebGL2 rendering
- Collision traces passing (CP-1, CP-2)
