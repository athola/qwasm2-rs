//! The retail campaign played through its level exits with saves: the
//! headless half of the save/load dogfood (#15). It needs the retail paks
//! in `gamedata/retail/baseq2` and skips without them.
//!
//! The level graph comes from every map's `target_changelevel` `map`
//! keys, starting at base1. Each exit is taken with `gamemap`, the command
//! C ExitLevel runs, so units keep and wipe their levels as in play.
//! Every level reached is saved and loaded back before moving on.

mod common;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use q2_common::filesystem::FileSystem;
use q2_server::init::{LevelKind, MapRequest};
use q2_server::local::LocalGame;
use q2_shared::constants::STAT_HEALTH;
use q2_shared::types::UserCmd;

/// The `map` keys of `map`'s `target_changelevel`s.
fn exits(fs: &FileSystem, map: &str) -> Vec<String> {
    let bsp = fs
        .load_file(&format!("maps/{map}.bsp"))
        .expect("map in the paks");
    let header = q2_common::bsp_file::BspHeader::parse(&bsp).expect("a BSP");
    let entities =
        q2_game::spawn::parse_entity_string(&header.entity_string(&bsp).unwrap()).unwrap();
    let mut levels: Vec<String> = entities
        .iter()
        .filter(|e| {
            e.get("classname")
                .is_some_and(|c| c == "target_changelevel")
        })
        .filter_map(|e| e.get("map").cloned())
        .collect();
    levels.sort();
    levels.dedup();
    levels
}

/// The game map a level string ends up in, following a cinematic's next
/// server; `None` for a picture level.
fn destination(level: &str) -> Option<String> {
    let request = MapRequest::parse(level);
    match (request.kind, request.next_server) {
        (LevelKind::Game, _) => Some(request.map),
        (LevelKind::Cinematic, Some(next)) => destination(&next),
        _ => None,
    }
}

/// Load what `take_map_change` hands out until nothing is pending (a
/// skipped cinematic queues its next server). Returns where it ended.
fn follow(game: &mut LocalGame, fs: &FileSystem) -> Option<String> {
    let mut reached = None;
    while let Some(level) = game.take_map_change() {
        let request = MapRequest::parse(&level);
        let bsp = match request.kind {
            LevelKind::Game => fs
                .load_file(&format!("maps/{}.bsp", request.map))
                .expect("map in the paks"),
            _ => Vec::new(),
        };
        game.change_map(&request, &bsp)
            .unwrap_or_else(|e| panic!("{level}: {e}"));
        reached = Some(request.map);
    }
    reached
}

fn idle(game: &mut LocalGame, seconds: usize) {
    for _ in 0..seconds * 10 {
        game.think(&UserCmd {
            msec: 100,
            ..UserCmd::default()
        })
        .unwrap();
        game.frame(100_000).unwrap();
    }
}

fn alive(game: &LocalGame) -> bool {
    game.player()
        .is_some_and(|(_, ps)| ps.stats[STAT_HEALTH] > 0)
}

#[test]
fn the_campaign_plays_through_every_exit_with_saves() {
    let Some(fs) = common::retail_fs() else {
        return;
    };
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut queue = VecDeque::from(["base1".to_string()]);
    while let Some(map) = queue.pop_front() {
        if graph.contains_key(&map) {
            continue;
        }
        let levels = exits(&fs, &map);
        queue.extend(levels.iter().filter_map(|level| destination(level)));
        graph.insert(map, levels);
    }
    let mut untaken: BTreeSet<(String, String)> = graph
        .iter()
        .flat_map(|(map, levels)| levels.iter().map(move |level| (map.clone(), level.clone())))
        .collect();
    eprintln!("{} maps, {} exits", graph.len(), untaken.len());

    let bsp = fs.load_file("maps/base1.bsp").unwrap();
    let mut game = LocalGame::start("base1", &bsp).unwrap();
    let mut here = "base1".to_string();
    let mut visited = BTreeSet::from([here.clone()]);
    let (mut saves, mut dead) = (0, 0);
    loop {
        let level = match untaken.iter().find(|(from, _)| *from == here) {
            Some(edge) => edge.1.clone(),
            None => match path_to_untaken(&graph, &untaken, &here) {
                Some(level) => level,
                None => break,
            },
        };
        untaken.remove(&(here.clone(), level.clone()));
        game.console(&format!("gamemap {level}")).unwrap();
        let Some(reached) = follow(&mut game, &fs) else {
            panic!("{here} -> {level}: nothing loaded");
        };
        eprintln!("{here} -> {level}");
        here = destination(&level).unwrap_or(reached);
        visited.insert(here.clone());

        idle(&mut game, 3);
        if !alive(&game) {
            dead += 1;
            continue; // C refuses to save a dead player.
        }
        let before = game.player().unwrap().0;
        game.console("save walk").unwrap();
        game.console("load walk").unwrap();
        assert_eq!(
            follow(&mut game, &fs).as_deref(),
            Some(here.as_str()),
            "load of {here}"
        );
        assert_eq!(
            game.player().unwrap().0,
            before,
            "{here}: loaded player moved"
        );
        idle(&mut game, 1);
        saves += 1;
    }
    eprintln!(
        "{} levels visited, {saves} saved and loaded, {dead} with the player dead",
        visited.len()
    );
    assert!(untaken.is_empty(), "exits never reached: {untaken:?}");
    let unvisited: Vec<_> = graph.keys().filter(|map| !visited.contains(*map)).collect();
    assert!(unvisited.is_empty(), "levels never reached: {unvisited:?}");
}

/// The first exit to take from `here` toward the nearest level that still
/// has an exit not taken (breadth first over every exit).
fn path_to_untaken(
    graph: &BTreeMap<String, Vec<String>>,
    untaken: &BTreeSet<(String, String)>,
    here: &str,
) -> Option<String> {
    let mut first: BTreeMap<String, String> = BTreeMap::new();
    let mut queue = VecDeque::from([here.to_string()]);
    let mut seen = BTreeSet::from([here.to_string()]);
    while let Some(map) = queue.pop_front() {
        if map != here && untaken.iter().any(|(from, _)| *from == map) {
            return first.get(&map).cloned();
        }
        for level in graph.get(&map).into_iter().flatten() {
            let Some(next) = destination(level) else {
                continue;
            };
            if seen.insert(next.clone()) {
                let step = first.get(&map).cloned().unwrap_or_else(|| level.clone());
                first.insert(next.clone(), step);
                queue.push_back(next);
            }
        }
    }
    None
}
