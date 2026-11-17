use super::*;
use crate::entity::{ClientData, EntityKey, MonsterInfo, ThinkFn};
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
    let saved = world.encode_level().expect("saves");

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
    let err = world.encode_level().unwrap_err();
    assert!(err.to_string().contains("callback in think"), "{err}");
}

#[test]
fn an_unregistered_end_move_or_monster_move_is_named_and_not_saved() {
    let mut world = test_world();
    let key = spawn(&mut world, "func_door");
    world.entities.get_mut(key).unwrap().moveinfo = Some(Box::new(crate::entity::MoveInfo {
        endfunc: Some(not_a_game_callback),
        ..Default::default()
    }));
    let err = world.encode_level().unwrap_err();
    assert!(err.to_string().contains("moveinfo.endfunc"), "{err}");

    world.entities.get_mut(key).unwrap().moveinfo = None;
    static FRAMES: [crate::entity::MonsterFrame; 0] = [];
    world.entities.get_mut(key).unwrap().monsterinfo = Some(Box::new(MonsterInfo {
        currentmove: Some(crate::entity::MonsterMove {
            frames: &FRAMES,
            ..Default::default()
        }),
        ..MonsterInfo::default()
    }));
    let err = world.encode_level().unwrap_err();
    assert!(err.to_string().contains("monsterinfo.currentmove"), "{err}");
}

#[test]
fn an_unknown_callback_or_monster_move_name_is_refused_on_load() {
    #[derive(serde::Deserialize)]
    struct Fields {
        #[serde(with = "crate::savegame::fields::callback")]
        _think: Option<ThinkFn>,
        #[serde(with = "crate::savegame::fields::monster_move")]
        _currentmove: Option<crate::entity::MonsterMove>,
    }
    let decode = |think: &str, mv: &str| {
        let bytes = postcard::to_extend(&(Some(think), Some(mv)), Vec::new()).unwrap();
        postcard::from_bytes::<Fields>(&bytes).is_ok()
    };
    let think = registry::name_of(crate::func::move_done as ThinkFn).unwrap();
    let mv = registry::move_name(&crate::monster::soldier::SOLDIER_MOVE_STAND1).unwrap();
    assert!(decode(think, mv), "registered names load");
    assert!(!decode("no_such_think", mv), "unknown callback");
    assert!(!decode(think, "no_such_move"), "unknown monster move");
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
    let saved = world.encode_level().unwrap();

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
    let saved = world.encode_level().unwrap();

    let (mut loaded, gi) = test_world_recording();
    loaded.read_level(&saved).unwrap();
    assert!(gi.links.borrow().iter().any(|link| link.number == number));
}

#[test]
fn read_level_marks_clients_unconnected() {
    let mut world = test_world();
    world.clients[0].pers.connected = true;
    let saved_level = world.encode_level().unwrap();
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
    let saved = world.encode_level().unwrap();

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
    let mut level = world.encode_level().unwrap();
    let game = world.write_game(false).unwrap();

    let mut loaded = test_world();
    let refusal = |result: Q2Result<()>| result.unwrap_err().to_string();
    assert!(refusal(loaded.read_level(&level[..level.len() / 2])).contains("damaged"));
    assert!(refusal(loaded.read_game(&game[..game.len() - 1])).contains("damaged"));
    let mut longer = game.clone();
    longer.push(0);
    assert!(refusal(loaded.read_game(&longer)).contains("trailing data"));
    assert!(refusal(loaded.read_level(&game)).contains("another kind"));
    assert!(refusal(loaded.read_level(&level[1..])).contains("not a savegame"));
    level[MAGIC.len()] ^= 0xff; // version byte
    assert!(refusal(loaded.read_level(&level)).contains("incompatible version"));

    let wrong_count = encode(
        KIND_GAME,
        &GameSave {
            game: world.game.clone(),
            clients: Vec::new(),
        },
    )
    .unwrap();
    assert!(refusal(loaded.read_game(&wrong_count)).contains("client count"));
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
    let level_file = world.encode_level().unwrap();

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
#[test]
fn the_save_layout_is_pinned_so_a_change_bumps_version() {
    // postcard writes fields by position, so a saved type that changes
    // shape (a field added, removed, retyped or moved) misreads older
    // saves. The pin hashes each field's name and kind in write order, not
    // the values, so it moves only with the shape. If this fails, bump
    // VERSION and update the pin.
    let world = world_with_player();
    let level = schema::fingerprint(&world.level_save());
    let game = schema::fingerprint(&GameSave {
        game: world.game.clone(),
        clients: world.clients.clone(),
    });
    assert_eq!(
        (VERSION, level, game),
        (1, 3_385_623_578_859_607_330, 12_624_391_596_431_867_328),
        "a saved type changed: bump VERSION"
    );
}

#[test]
fn the_layout_pin_moves_with_a_field_order_but_not_a_value() {
    #[derive(serde::Serialize)]
    struct Ab {
        a: i32,
        b: i32,
    }
    #[derive(serde::Serialize)]
    struct Ba {
        b: i32,
        a: i32,
    }
    let ab = schema::fingerprint(&Ab { a: 1, b: 2 });
    assert_eq!(ab, schema::fingerprint(&Ab { a: 7, b: 9 }));
    assert_ne!(ab, schema::fingerprint(&Ba { b: 2, a: 1 }));
}

/// A serializer that hashes a value's shape: struct and field names,
/// variant names and primitive kinds, in the order postcard writes them.
mod schema {
    use serde::ser::{self, Serialize};

    pub fn fingerprint<T: Serialize>(value: &T) -> u64 {
        let mut shape = Shape(0xcbf2_9ce4_8422_2325);
        value.serialize(&mut shape).unwrap();
        shape.0
    }

    pub struct Shape(u64);

    impl Shape {
        fn mark(&mut self, text: &str) {
            for b in text.bytes().chain([0]) {
                self.0 = (self.0 ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    }

    #[derive(Debug)]
    pub struct Error;
    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("shape")
        }
    }
    impl std::error::Error for Error {}
    impl ser::Error for Error {
        fn custom<T: std::fmt::Display>(_: T) -> Self {
            Error
        }
    }

    type R = Result<(), Error>;

    macro_rules! kinds {
        ($($method:ident: $ty:ty),*) => {
            $(fn $method(self, _: $ty) -> R {
                self.mark(stringify!($ty));
                Ok(())
            })*
        };
    }

    impl ser::Serializer for &mut Shape {
        type Ok = ();
        type Error = Error;
        type SerializeSeq = Self;
        type SerializeTuple = Self;
        type SerializeTupleStruct = Self;
        type SerializeTupleVariant = Self;
        type SerializeMap = Self;
        type SerializeStruct = Self;
        type SerializeStructVariant = Self;

        kinds!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16,
            serialize_i32: i32, serialize_i64: i64, serialize_u8: u8,
            serialize_u16: u16, serialize_u32: u32, serialize_u64: u64,
            serialize_f32: f32, serialize_f64: f64, serialize_char: char,
            serialize_str: &str, serialize_bytes: &[u8]);

        fn serialize_none(self) -> R {
            self.mark("none");
            Ok(())
        }
        fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> R {
            self.mark("some");
            v.serialize(self)
        }
        fn serialize_unit(self) -> R {
            self.mark("unit");
            Ok(())
        }
        fn serialize_unit_struct(self, name: &'static str) -> R {
            self.mark(name);
            Ok(())
        }
        fn serialize_unit_variant(self, name: &'static str, _: u32, variant: &'static str) -> R {
            self.mark(name);
            self.mark(variant);
            Ok(())
        }
        fn serialize_newtype_struct<T: ?Sized + Serialize>(self, name: &'static str, v: &T) -> R {
            self.mark(name);
            v.serialize(self)
        }
        fn serialize_newtype_variant<T: ?Sized + Serialize>(
            self,
            name: &'static str,
            _: u32,
            variant: &'static str,
            v: &T,
        ) -> R {
            self.mark(name);
            self.mark(variant);
            v.serialize(self)
        }
        fn serialize_seq(self, _: Option<usize>) -> Result<Self, Error> {
            self.mark("seq");
            Ok(self)
        }
        fn serialize_tuple(self, _: usize) -> Result<Self, Error> {
            self.mark("tuple");
            Ok(self)
        }
        fn serialize_tuple_struct(self, name: &'static str, _: usize) -> Result<Self, Error> {
            self.mark(name);
            Ok(self)
        }
        fn serialize_tuple_variant(
            self,
            name: &'static str,
            _: u32,
            variant: &'static str,
            _: usize,
        ) -> Result<Self, Error> {
            self.mark(name);
            self.mark(variant);
            Ok(self)
        }
        fn serialize_map(self, _: Option<usize>) -> Result<Self, Error> {
            self.mark("map");
            Ok(self)
        }
        fn serialize_struct(self, name: &'static str, _: usize) -> Result<Self, Error> {
            self.mark(name);
            Ok(self)
        }
        fn serialize_struct_variant(
            self,
            name: &'static str,
            _: u32,
            variant: &'static str,
            _: usize,
        ) -> Result<Self, Error> {
            self.mark(name);
            self.mark(variant);
            Ok(self)
        }
    }

    macro_rules! elements {
        ($($trait:ident :: $method:ident),*) => {
            $(impl ser::$trait for &mut Shape {
                type Ok = ();
                type Error = Error;
                fn $method<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
                    v.serialize(&mut **self)
                }
                fn end(self) -> R {
                    Ok(())
                }
            })*
        };
    }
    elements!(
        SerializeSeq::serialize_element,
        SerializeTuple::serialize_element,
        SerializeTupleStruct::serialize_field,
        SerializeTupleVariant::serialize_field
    );

    impl ser::SerializeMap for &mut Shape {
        type Ok = ();
        type Error = Error;
        fn serialize_key<T: ?Sized + Serialize>(&mut self, k: &T) -> R {
            k.serialize(&mut **self)
        }
        fn serialize_value<T: ?Sized + Serialize>(&mut self, v: &T) -> R {
            v.serialize(&mut **self)
        }
        fn end(self) -> R {
            Ok(())
        }
    }

    macro_rules! fields {
        ($($trait:ident),*) => {
            $(impl ser::$trait for &mut Shape {
                type Ok = ();
                type Error = Error;
                fn serialize_field<T: ?Sized + Serialize>(&mut self, name: &'static str, v: &T) -> R {
                    self.mark(name);
                    v.serialize(&mut **self)
                }
                fn end(self) -> R {
                    Ok(())
                }
            })*
        };
    }
    fields!(SerializeStruct, SerializeStructVariant);
}

/// A game save of `world` after `tamper` changes its first client.
fn tampered_game(world: &GameWorld, tamper: impl FnOnce(&mut ClientData)) -> Vec<u8> {
    let mut clients = world.clients.clone();
    tamper(&mut clients[0]);
    encode(
        KIND_GAME,
        &GameSave {
            game: world.game.clone(),
            clients,
        },
    )
    .unwrap()
}

#[test]
fn a_game_save_whose_item_indices_leave_the_item_list_is_refused() {
    // They index the inventory and item list every frame: a tampered or
    // corrupt store must not reach that as a panic.
    use crate::traits::GameExport;
    let world = world_with_player();
    let items = world.items.len();
    type Tamper = Box<dyn Fn(&mut ClientData)>;
    let cases: [(&str, Tamper); 6] = [
        (
            "ammo_index",
            Box::new(|c| c.ammo_index = crate::constants::MAX_ITEMS),
        ),
        (
            "selected_item",
            Box::new(move |c| c.pers.selected_item = items as i32),
        ),
        ("weapon", Box::new(move |c| c.pers.weapon = Some(items))),
        ("newweapon", Box::new(move |c| c.newweapon = Some(items))),
        ("inventory", Box::new(move |c| c.pers.inventory[items] = 1)),
        (
            "coop_respawn",
            Box::new(move |c| c.resp.coop_respawn.last_weapon = Some(items)),
        ),
    ];
    assert!(world.check_game(&tampered_game(&world, |_| {})).is_ok());
    for (field, tamper) in cases {
        let bytes = tampered_game(&world, tamper);
        let err = world.check_game(&bytes).unwrap_err().to_string();
        assert!(err.contains(field), "{field}: {err}");
        let mut loaded = world_with_player();
        assert!(
            loaded.read_game(&bytes).is_err(),
            "{field}: read refused too"
        );
    }
}

#[test]
fn a_level_save_with_a_bad_trail_or_entity_numbering_is_refused() {
    use crate::traits::GameExport;
    let world = world_with_player();
    let level = |tamper: &dyn Fn(&mut LevelSave)| {
        let mut save = world.level_save();
        tamper(&mut save);
        encode(KIND_LEVEL, &save).unwrap()
    };
    assert!(world.check_level(&level(&|_| {})).is_ok());
    let trail = level(&|s| {
        s.trail
            .set_head_for_test(crate::player::trail::TRAIL_LENGTH)
    });
    assert!(world
        .check_level(&trail)
        .unwrap_err()
        .to_string()
        .contains("trail"));
    let numbering = level(&|s| s.entities.break_numbering_for_test());
    assert!(world
        .check_level(&numbering)
        .unwrap_err()
        .to_string()
        .contains("numbering"));
    let mut loaded = world_with_player();
    assert!(loaded.read_level(&numbering).is_err(), "read refused too");
}
