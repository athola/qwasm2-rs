//! Area portals on the server: the areas C SV_LinkEdict gives an entity,
//! the area check of C SV_BuildClientFrame and PF_inPVS/PF_inPHS, and the
//! game's gi.SetAreaPortalState / gi.AreasConnected, against the two-area
//! fixture (area 1 at y > 0, area 2 at y < 0, joined by portal 1).

mod common;

use q2_game::traits::{EdictView, EntityLink, GameImport};
use q2_server::ents::build_client_frame;
use q2_server::ServerGameImport;
use q2_shared::types::{EntityState, Solid, Vec3f};

const CLIENT: usize = 1;
/// An eye in area 1 and one in area 2.
const EYE_1: Vec3f = Vec3f::new(0.0, 200.0, 40.0);
const EYE_2: Vec3f = Vec3f::new(0.0, -200.0, 40.0);
/// Origins well inside each area, clear of the floor and the wall.
const IN_1: Vec3f = Vec3f::new(0.0, 300.0, 40.0);
const IN_2: Vec3f = Vec3f::new(0.0, -300.0, 40.0);

fn loaded() -> ServerGameImport {
    let gi = ServerGameImport::new();
    gi.load_map(&common::two_area_bsp_with(common::ENTSTRING))
        .expect("fixture loads");
    gi
}

fn link(gi: &ServerGameImport, number: usize, origin: Vec3f) {
    let half = Vec3f::splat(8.0);
    gi.link_entity(&EntityLink {
        number,
        origin,
        mins: -half,
        maxs: half,
        absmin: origin - half - Vec3f::ONE,
        absmax: origin + half + Vec3f::ONE,
        solid: Solid::Bbox,
        ..Default::default()
    });
}

fn state(number: i32, origin: Vec3f) -> EntityState {
    EntityState {
        number,
        origin,
        modelindex: 3,
        ..Default::default()
    }
}

fn sent(gi: &ServerGameImport, eye: Vec3f, states: &[EntityState]) -> Vec<i32> {
    let views = states.iter().map(|state| EdictView {
        state,
        svflags: 0,
        owner: None,
        linked: true,
    });
    build_client_frame(gi, CLIENT, eye, views)
        .iter()
        .map(|s| s.number)
        .collect()
}

#[test]
fn link_entity_gives_an_entity_its_area_and_a_straddler_both() {
    let gi = loaded();
    link(&gi, 2, IN_1);
    link(&gi, 3, IN_2);
    link(&gi, 4, Vec3f::new(0.0, 0.0, 40.0));
    assert_eq!(gi.entity_areas(2), (1, 0));
    assert_eq!(gi.entity_areas(3), (2, 0));
    let (a, b) = gi.entity_areas(4);
    assert_eq!([a.min(b), a.max(b)], [1, 2]);
}

#[test]
fn entity_touching_only_solid_leafs_has_no_area() {
    let gi = loaded();
    link(&gi, 2, Vec3f::new(0.0, 300.0, -200.0)); // inside the floor
    assert_eq!(gi.entity_areas(2), (0, 0));
}

#[test]
fn areas_join_only_while_their_portal_is_open() {
    let gi = loaded();
    assert!(gi.areas_connected(1, 1));
    assert!(!gi.areas_connected(1, 2), "portals start closed");
    gi.set_area_portal_state(1, true);
    assert!(gi.areas_connected(1, 2));
    gi.set_area_portal_state(1, false);
    assert!(!gi.areas_connected(2, 1));
}

#[test]
fn bad_portal_or_area_numbers_are_dropped_not_fatal() {
    let gi = loaded();
    gi.set_area_portal_state(100_000, true);
    gi.set_area_portal_state(-1, true);
    assert!(!gi.areas_connected(1, 2));
    assert!(!gi.areas_connected(1, 99));
    assert!(!gi.areas_connected(-1, 1));
}

#[test]
fn entity_beyond_a_closed_portal_is_not_sent_until_it_opens() {
    let gi = loaded();
    link(&gi, 2, IN_1);
    link(&gi, 3, IN_2);
    let states = [state(2, IN_1), state(3, IN_2)];
    assert_eq!(sent(&gi, EYE_1, &states), [2], "own area only");
    assert_eq!(sent(&gi, EYE_2, &states), [3]);
    gi.set_area_portal_state(1, true);
    assert_eq!(sent(&gi, EYE_1, &states), [2, 3]);
    gi.set_area_portal_state(1, false);
    assert_eq!(sent(&gi, EYE_1, &states), [2]);
}

#[test]
fn door_straddling_two_areas_is_sent_to_both_while_the_portal_is_closed() {
    let gi = loaded();
    let door = Vec3f::new(0.0, 0.0, 40.0);
    link(&gi, 2, door);
    let states = [state(2, door)];
    assert_eq!(sent(&gi, EYE_1, &states), [2]);
    assert_eq!(sent(&gi, EYE_2, &states), [2]);
}

#[test]
fn the_client_itself_is_sent_from_behind_a_closed_portal() {
    let gi = loaded();
    link(&gi, CLIENT, IN_2);
    assert_eq!(
        sent(&gi, EYE_1, &[state(CLIENT as i32, IN_2)]),
        [CLIENT as i32]
    );
}

#[test]
fn closed_portal_blocks_the_games_sight_and_hearing_checks() {
    let gi = loaded();
    assert!(gi.in_pvs(IN_1, EYE_1) && gi.in_phs(IN_1, EYE_1));
    assert!(!gi.in_pvs(IN_1, IN_2));
    assert!(!gi.in_phs(IN_1, IN_2));
    gi.set_area_portal_state(1, true);
    assert!(gi.in_pvs(IN_1, IN_2));
    assert!(gi.in_phs(IN_1, IN_2));
}

#[test]
fn loading_a_map_closes_its_portals_and_forgets_entity_areas() {
    let gi = loaded();
    link(&gi, 2, IN_2);
    gi.set_area_portal_state(1, true);
    gi.load_map(&common::two_area_bsp_with(common::ENTSTRING))
        .expect("reloads");
    assert!(!gi.areas_connected(1, 2));
    assert_eq!(gi.entity_areas(2), (0, 0));
}

/// The numbers sent to a client with its eye at `eye`.
fn sent_from(world: &common::ServerWorld, eye: Vec3f) -> Vec<i32> {
    use q2_game::traits::GameExport;
    let game = &world.game;
    let entities = (1..game.num_edicts()).filter_map(|n| game.edict(n));
    build_client_frame(&world.gi, CLIENT, eye, entities)
        .iter()
        .map(|s| s.number)
        .collect()
}

#[test]
fn demo1_soldier_306_lands_and_lets_door_242_shut_again() {
    // Soldier 306 starts in door 242's touch field. Before m_droptofloor
    // it hung there without a ground entity, touched the field every frame
    // and held the door open for good. Now the door is down again at
    // about 5.7 s.
    use q2_game::func::{STATE_BOTTOM, STATE_TOP};
    let Some(map) = common::demo1_bsp() else {
        return;
    };
    let mut world = common::ServerWorld::start("demo1", &map);
    world.think(&Default::default());
    let soldier = world.game.entities.key_for_number(306).expect("soldier");
    let door = world.game.entities.key_for_number(242).expect("door");
    let (mut opened, mut shut) = (false, false);
    for _ in 0..100 {
        world.frame(100_000);
        let moveinfo = world.game.entities.get(door).unwrap().moveinfo.as_ref();
        match moveinfo.unwrap().state {
            STATE_TOP => opened = true,
            STATE_BOTTOM if opened => shut = true,
            _ => {}
        }
    }
    let s = world.game.entities.get(soldier).unwrap();
    assert!(s.in_use && s.game.classname == "monster_soldier");
    assert!(
        s.game.ground_entity.is_some(),
        "the soldier stands on the floor"
    );
    assert!(opened, "the soldier's touch opens door 242");
    assert!(shut, "door 242 shuts again");
}

#[test]
fn demo1_door_hides_the_far_area_until_it_opens_its_portal() {
    use q2_game::func::{door_use, STATE_BOTTOM, STATE_TOP};
    let Some(map) = common::demo1_bsp() else {
        return;
    };
    let mut world = common::ServerWorld::start("demo1", &map);
    world.think(&Default::default());
    world.frame(100_000);
    // demo1's one portal door (entity 242, targeting func_areaportal
    // style 1): a 20-unit slab at x -1122..-1102 that rises, with area 2
    // on its -x side and area 1, the player start's, on its +x side.
    let door = world.game.entities.key_for_number(242).expect("door");
    assert_eq!(
        world.game.entities.get(door).unwrap().game.classname,
        "func_door"
    );
    assert_eq!(world.gi.entity_areas(242), (1, 2));
    let behind = Vec3f::new(-1176.0, 1632.0, 16.0);
    assert_eq!(world.gi.point_area(behind), 2);
    let start = world
        .game
        .entities
        .get(world.game.entities.key_for_number(1).unwrap())
        .unwrap()
        .state
        .origin;
    assert_eq!(world.gi.point_area(start), 1);

    let in_area_2 = |w: &common::ServerWorld, n: i32| {
        let (a, b) = w.gi.entity_areas(n as usize);
        a == 2 || b == 2
    };
    // Wait for the door, opened by soldier 306 at the start, to shut.
    let door_state = |w: &common::ServerWorld| {
        w.game
            .entities
            .get(door)
            .unwrap()
            .moveinfo
            .as_ref()
            .unwrap()
            .state
    };
    for _ in 0..100 {
        if door_state(&world) == STATE_BOTTOM {
            break;
        }
        world.frame(100_000);
    }
    assert_eq!(door_state(&world), STATE_BOTTOM);
    assert!(!world.gi.areas_connected(1, 2), "shut door, shut portal");
    let closed = sent_from(&world, behind);
    let from_start_closed = sent_from(&world, start);
    assert!(closed.contains(&242), "the door straddles both areas");
    assert!(
        closed
            .iter()
            .all(|&n| n == CLIENT as i32 || in_area_2(&world, n)),
        "only area 2 behind the shut door: {closed:?}"
    );

    let player = world.game.entities.key_for_number(1).unwrap();
    door_use(&mut world.game, door, player, player);
    for _ in 0..30 {
        world.frame(100_000);
    }
    assert_eq!(door_state(&world), STATE_TOP);
    assert!(world.gi.areas_connected(1, 2));
    let open = sent_from(&world, behind);
    assert!(
        closed.iter().all(|n| open.contains(n)),
        "{closed:?} -> {open:?}"
    );
    assert!(
        open.iter()
            .any(|&n| !in_area_2(&world, n) && n != CLIENT as i32),
        "area 1 shows through the open door: {open:?}"
    );
    // Nothing in the player's own area disappears (a looping
    // target_speaker there may switch on meanwhile).
    let from_start_open = sent_from(&world, start);
    assert!(from_start_closed
        .iter()
        .all(|n| from_start_open.contains(n)));
}
