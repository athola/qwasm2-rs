use super::*;
use crate::entity::{EntityKey, MonsterInfo, ThinkFn};
use crate::world::{test_world, test_world_recording, GameWorld};

fn not_a_game_callback(_: &mut GameWorld, _: EntityKey) {}

fn spawn(world: &mut GameWorld, classname: &str) -> EntityKey {
    let key = world.entities.spawn().expect("free slot");
    let ent = world.entities.get_mut(key).unwrap();
    ent.in_use = true;
    ent.game.classname = classname.to_string();
    key
}

#[test]
fn a_level_loads_back_entities_their_references_and_callbacks() {
    let mut world = test_world();
    world.level.time = 12.5;
    world.level.killed_monsters = 3;
    let soldier = spawn(&mut world, "monster_soldier");
    let target = spawn(&mut world, "path_corner");
    {
        let ent = world.entities.get_mut(soldier).unwrap();
        ent.game.health = 17;
        ent.game.goalentity = Some(target);
        ent.owner = Some(target);
        ent.think = Some(crate::func::move_done);
        ent.monsterinfo = Some(Box::new(MonsterInfo {
            currentmove: Some(crate::monster::soldier::SOLDIER_MOVE_STAND1.clone()),
            stand: Some(crate::monster::soldier::soldier_stand),
            ..MonsterInfo::default()
        }));
    }
    world.level.sight_entity = Some(soldier);
    let saved = world.write_level().expect("saves");

    let mut loaded = test_world();
    loaded.read_level(&saved).expect("loads");

    assert_eq!(loaded.level.time, 12.5);
    assert_eq!(loaded.level.killed_monsters, 3);
    assert_eq!(loaded.level.sight_entity, Some(soldier));
    let ent = loaded.entities.get(soldier).expect("same key");
    assert!(ent.in_use);
    assert_eq!(ent.game.health, 17);
    assert_eq!(ent.game.goalentity, Some(target));
    assert_eq!(ent.owner, Some(target));
    assert_eq!(
        ent.think.map(|f| f as usize),
        Some(crate::func::move_done as ThinkFn as usize)
    );
    let info = ent.monsterinfo.as_ref().unwrap();
    let mv = info.currentmove.as_ref().unwrap();
    assert!(std::ptr::eq(
        mv.frames,
        crate::monster::soldier::SOLDIER_MOVE_STAND1.frames
    ));
    assert_eq!(
        info.stand.map(|f| f as usize),
        Some(crate::monster::soldier::soldier_stand as ThinkFn as usize)
    );
    assert_eq!(
        loaded.entities.get(target).unwrap().game.classname,
        "path_corner"
    );
}

#[test]
fn a_level_with_an_unregistered_callback_is_not_saved() {
    let mut world = test_world();
    let key = spawn(&mut world, "info_null");
    world.entities.get_mut(key).unwrap().think = Some(not_a_game_callback);
    let err = world.write_level().unwrap_err();
    assert!(err.to_string().contains("callback"), "{err}");
}

#[test]
fn a_freed_entity_loads_back_free_and_reusable_like_c() {
    // C WriteLevel skips free edicts and ReadLevel zeroes them, so a freed
    // edict comes back with freetime 0 and no classname: G_Spawn may reuse
    // it at once. Its key stays valid, so a reference to it stays stale.
    let mut world = test_world();
    world.level.time = 30.0;
    let gib = spawn(&mut world, "gib");
    world.free_entity(gib);
    let saved = world.write_level().unwrap();

    let mut loaded = test_world();
    loaded.read_level(&saved).unwrap();
    let ent = loaded.entities.get(gib).expect("key kept");
    assert!(!ent.in_use);
    assert_eq!(ent.game.freetime, 0.0);
    assert_eq!(ent.game.classname, "");
}

#[test]
fn read_level_relinks_every_entity_in_use() {
    let mut world = test_world();
    let lamp = spawn(&mut world, "light");
    let number = world.number_of(lamp).unwrap();
    let saved = world.write_level().unwrap();

    let (mut loaded, gi) = test_world_recording();
    loaded.read_level(&saved).unwrap();
    assert!(gi.links.borrow().iter().any(|link| link.number == number));
}

#[test]
fn read_level_marks_clients_unconnected() {
    let mut world = test_world();
    world.clients[0].pers.connected = true;
    let saved_level = world.write_level().unwrap();
    let saved_game = world.write_game(false).unwrap();

    let mut loaded = test_world();
    loaded.read_game(&saved_game).unwrap();
    loaded.read_level(&saved_level).unwrap();
    assert!(!loaded.clients[0].pers.connected);
}

#[test]
fn read_level_starts_crosslevel_targets_after_their_delay() {
    let mut world = test_world();
    world.level.time = 4.0;
    let target = spawn(&mut world, "target_crosslevel_target");
    world.entities.get_mut(target).unwrap().game.delay = 1.5;
    let saved = world.write_level().unwrap();

    let mut loaded = test_world();
    loaded.read_level(&saved).unwrap();
    assert_eq!(loaded.entities.get(target).unwrap().game.nextthink, 5.5);
}

#[test]
fn a_game_loads_back_clients_and_cross_level_state() {
    let mut world = test_world();
    world.game.serverflags = 0b101;
    world.game.helpmessage1 = "Find the exit".to_string();
    world.clients[0].pers.inventory[3] = 50;
    world.clients[0].pers.netname = "Player".to_string();
    let saved = world.write_game(true).unwrap();

    let mut loaded = test_world();
    loaded.read_game(&saved).unwrap();
    assert_eq!(loaded.game.serverflags, 0b101);
    assert_eq!(loaded.game.helpmessage1, "Find the exit");
    assert_eq!(loaded.clients[0].pers.inventory[3], 50);
    assert_eq!(loaded.clients[0].pers.netname, "Player");
    assert!(
        loaded.game.autosaved,
        "C writes game.autosaved for an autosave"
    );
    assert!(!world.game.autosaved, "and clears it again after the write");
}

#[test]
fn a_save_of_another_version_or_cut_short_is_refused() {
    let mut world = test_world();
    let mut level = world.write_level().unwrap();
    let game = world.write_game(false).unwrap();

    let mut loaded = test_world();
    assert!(loaded.read_level(&level[..level.len() / 2]).is_err());
    assert!(loaded.read_game(&game[..game.len() - 1]).is_err());
    assert!(
        loaded.read_level(&game).is_err(),
        "a game file is not a level"
    );
    level[MAGIC.len()] ^= 0xff; // version byte
    assert!(loaded.read_level(&level).is_err());
}

const MAP: &str = "{ \"classname\" \"worldspawn\" }\n{ \"classname\" \"info_player_start\" \"origin\" \"64 0 24\" }";

/// A world on `MAP` with client 1 in the game.
fn world_with_player() -> GameWorld {
    use crate::traits::GameExport;
    let mut world = test_world();
    world.set_max_clients(1);
    world.init();
    world.spawn_entities("base1", MAP, "");
    assert!(world.client_connect(1, ""));
    world.client_begin(1);
    world
}

fn player(world: &GameWorld) -> (&crate::entity::Entity, &crate::entity::ClientData) {
    let key = world.entities.key_for_number(1).unwrap();
    (world.entities.get(key).unwrap(), world.client(key).unwrap())
}

/// The server's loadgame order: the game, the map, its level file, then
/// the client connects and begins (C SV_Loadgame_f, SV_DirectConnect).
fn load(game_file: &[u8], level_file: Option<&[u8]>) -> GameWorld {
    use crate::traits::GameExport;
    let mut world = test_world();
    world.set_max_clients(1);
    world.init();
    world.read_game(game_file).unwrap();
    world.spawn_entities("base1", MAP, "");
    if let Some(level) = level_file {
        world.read_level(level).unwrap();
    }
    assert!(world.client_connect(1, ""));
    world.client_begin(1);
    world
}

#[test]
fn a_loaded_player_takes_the_body_waiting_in_the_level() {
    let mut world = world_with_player();
    let key = world.entities.key_for_number(1).unwrap();
    world.entities.get_mut(key).unwrap().game.health = 37;
    world.entities.get_mut(key).unwrap().state.origin = q2_shared::Vec3f::new(300.0, -20.0, 40.0);
    world.client_mut(key).unwrap().ps.viewangles = q2_shared::Vec3f::new(10.0, 90.0, 0.0);
    let game_file = world.write_game(false).unwrap();
    let level_file = world.write_level().unwrap();

    let loaded = load(&game_file, Some(&level_file));
    let (ent, client) = player(&loaded);
    assert_eq!(ent.game.health, 37);
    assert_eq!(ent.state.origin, q2_shared::Vec3f::new(300.0, -20.0, 40.0));
    assert!(client.pers.connected);
    // C ClientBegin: the client cleared its view angles on connecting, so
    // the delta angles carry the saved view.
    let delta = client.ps.pmove.delta_angles;
    assert_eq!(delta[0], crate::player::client::angle2short(10.0));
    assert_eq!(delta[1], crate::player::client::angle2short(90.0));
}

#[test]
fn an_autosave_keeps_the_inventory_on_a_fresh_body() {
    // C ClientConnect keeps `pers` when the game came from an autosave and
    // the player holds a weapon: the inventory they entered the level with.
    let mut world = world_with_player();
    let key = world.entities.key_for_number(1).unwrap();
    let quad = world.find_item("item_quad").unwrap();
    world.client_mut(key).unwrap().pers.inventory[quad] = 2;
    let autosave = world.write_game(true).unwrap();

    let loaded = load(&autosave, None);
    assert_eq!(player(&loaded).1.pers.inventory[quad], 2);
}

#[test]
fn a_manual_save_without_its_level_starts_the_player_afresh() {
    let mut world = world_with_player();
    let key = world.entities.key_for_number(1).unwrap();
    let quad = world.find_item("item_quad").unwrap();
    world.client_mut(key).unwrap().pers.inventory[quad] = 2;
    let manual = world.write_game(false).unwrap();

    let loaded = load(&manual, None);
    assert_eq!(player(&loaded).1.pers.inventory[quad], 0);
}

/// FNV-1a: stable across Rust releases, unlike `DefaultHasher`.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_save_layout_is_pinned_so_a_change_bumps_version() {
    // postcard writes fields by position, so a saved type that changes
    // shape misreads older saves. If this fails, bump VERSION and update
    // the pin.
    let mut world = world_with_player();
    let level = fingerprint(&world.write_level().unwrap());
    let game = fingerprint(&world.write_game(true).unwrap());
    assert_eq!(
        (VERSION, level, game),
        (1, 7_902_816_311_037_946_725, 5_231_457_406_819_656_232),
        "a saved type changed: bump VERSION"
    );
}
