//! Shared fixtures for the server integration tests.
//!
//! CI has no game data, so tests that must run there use a map built here: a
//! floor whose top is z = 0 and a wall whose face is x = `WALL_X`, both
//! 500 units deep. The BSP tree is
//!
//! ```text
//! node 0: z = 0   back -> leaf 0 (solid, floor brush)
//!                 front -> node 1
//! node 1: x = WALL_X  back -> leaf 1 (empty), front -> leaf 2 (solid, wall)
//! ```
//!
//! which satisfies CM_LoadMap's rules (leaf 0 solid, an empty leaf exists,
//! children numbered above their parents).
//!
//! Inline model `*1` is a door-sized box, `DOOR_MINS`..`DOOR_MAXS`, reached
//! only through its own head node 2 (x = 96: front -> leaf 3, the box brush;
//! back -> leaf 1), so the world tree does not see it.
//!
//! `two_cluster_bsp_with` splits the open air at y = 0 (node 1's back child
//! becomes node 3: y = 0, front -> leaf 1, cluster 0; back -> leaf 4,
//! cluster 1) and adds a visibility lump in which neither cluster sees the
//! other, so PVS culling can be tested without game data.
//!
//! `two_area_bsp_with` makes the same split without vis data, but puts the
//! -y half in area 2 behind area portal 1 (closed when the map loads) and
//! its solid leafs in area 0, as a compiled map does, so area portals can
//! be tested without game data.

#![allow(dead_code)] // each test binary uses a different subset

/// x coordinate of the wall's face.
pub const WALL_X: f32 = 256.0;

/// Bounds of inline model `*1` as stored in the BSP.
pub const DOOR_MINS: [f32; 3] = [96.0, -64.0, 0.0];
pub const DOOR_MAXS: [f32; 3] = [128.0, 64.0, 128.0];

/// Entity string with a worldspawn and a player start on the floor.
pub const ENTSTRING: &str = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}\n";

use q2_shared::constants::CONTENTS_SOLID;

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

/// `dplane_t`: normal, dist, type.
fn plane(w: &mut Writer, normal: [f32; 3], dist: f32, kind: i32) {
    normal.iter().for_each(|&n| w.f32(n));
    w.f32(dist);
    w.i32(kind);
}

/// `dleaf_t` with only contents, cluster, area and the leaf-brush range set.
fn leaf(w: &mut Writer, contents: i32, cluster: i16, area: i16, brushes: (u16, u16)) {
    let (first_brush, num_brushes) = brushes;
    w.i32(contents);
    w.i16(cluster);
    w.i16(area);
    (0..6).for_each(|_| w.i16(0)); // mins, maxs
    w.u16(0);
    w.u16(0); // leaf faces
    w.u16(first_brush);
    w.u16(num_brushes);
}

/// `dnode_t` with only the plane and children set.
fn node(w: &mut Writer, plane: i32, front: i32, back: i32) {
    w.i32(plane);
    w.i32(front);
    w.i32(back);
    (0..6).for_each(|_| w.i16(0));
    w.u16(0);
    w.u16(0);
}

/// The floor-and-wall map as `.bsp` bytes. It also carries `ENTSTRING`.
pub fn floor_wall_bsp() -> Vec<u8> {
    floor_wall_bsp_with(ENTSTRING)
}

/// The floor-and-wall map with `entities` as its entity lump.
pub fn floor_wall_bsp_with(entities: &str) -> Vec<u8> {
    build_bsp(entities, Split::None)
}

/// The floor-and-wall map split into two clusters at y = 0 that cannot see
/// each other (see the module doc), with `entities` as its entity lump.
pub fn two_cluster_bsp_with(entities: &str) -> Vec<u8> {
    build_bsp(entities, Split::Clusters)
}

/// The floor-and-wall map split at y = 0 into area 1 (+y) and area 2 (-y),
/// joined by area portal 1 (see the module doc).
pub fn two_area_bsp_with(entities: &str) -> Vec<u8> {
    build_bsp(entities, Split::Areas)
}

/// How the open air is divided at y = 0.
#[derive(Clone, Copy, PartialEq)]
enum Split {
    None,
    Clusters,
    Areas,
}

fn build_bsp(entities: &str, split: Split) -> Vec<u8> {
    let two_clusters = split == Split::Clusters;
    let split_y = split != Split::None;
    let mut lumps: [Vec<u8>; 19] = Default::default();

    lumps[0] = format!("{entities}\0").into_bytes();

    let mut w = Writer::default();
    // 0-5: floor brush, x/y in [-500, 500], z in [-500, 0].
    plane(&mut w, [0.0, 0.0, 1.0], 0.0, 2);
    plane(&mut w, [0.0, 0.0, -1.0], 500.0, 5);
    plane(&mut w, [1.0, 0.0, 0.0], 500.0, 0);
    plane(&mut w, [-1.0, 0.0, 0.0], 500.0, 3);
    plane(&mut w, [0.0, 1.0, 0.0], 500.0, 1);
    plane(&mut w, [0.0, -1.0, 0.0], 500.0, 4);
    // 6-11: wall brush, x in [WALL_X, WALL_X + 500], y/z in [-500, 500].
    plane(&mut w, [1.0, 0.0, 0.0], WALL_X + 500.0, 0);
    plane(&mut w, [-1.0, 0.0, 0.0], -WALL_X, 3);
    plane(&mut w, [0.0, 1.0, 0.0], 500.0, 1);
    plane(&mut w, [0.0, -1.0, 0.0], 500.0, 4);
    plane(&mut w, [0.0, 0.0, 1.0], 500.0, 2);
    plane(&mut w, [0.0, 0.0, -1.0], 500.0, 5);
    // 12: node 1's split, +X at the wall face.
    plane(&mut w, [1.0, 0.0, 0.0], WALL_X, 0);
    // 13-18: the *1 box brush.
    plane(&mut w, [1.0, 0.0, 0.0], DOOR_MAXS[0], 0);
    plane(&mut w, [-1.0, 0.0, 0.0], -DOOR_MINS[0], 3);
    plane(&mut w, [0.0, 1.0, 0.0], DOOR_MAXS[1], 1);
    plane(&mut w, [0.0, -1.0, 0.0], -DOOR_MINS[1], 4);
    plane(&mut w, [0.0, 0.0, 1.0], DOOR_MAXS[2], 2);
    plane(&mut w, [0.0, 0.0, -1.0], -DOOR_MINS[2], 5);
    // 19: node 2's split, the box's -X face.
    plane(&mut w, [1.0, 0.0, 0.0], DOOR_MINS[0], 0);
    // 20: node 3's split between the two clusters.
    plane(&mut w, [0.0, 1.0, 0.0], 0.0, 1);
    lumps[1] = w.0;

    let mut w = Writer::default();
    node(&mut w, 0, 1, -1); // z = 0: above -> node 1, below -> leaf 0
    if split_y {
        node(&mut w, 12, -3, 3); // x = WALL_X: beyond -> leaf 2, before -> node 3
    } else {
        node(&mut w, 12, -3, -2); // x = WALL_X: beyond -> leaf 2, before -> leaf 1
    }
    node(&mut w, 19, -4, -2); // *1: x = 96: beyond -> leaf 3, before -> leaf 1
    if split_y {
        node(&mut w, 20, -2, -5); // y = 0: +y -> leaf 1, -y -> leaf 4
    }
    lumps[4] = w.0;

    let mut w = Writer::default();
    (0..8).for_each(|_| w.f32(0.0)); // vecs
    w.i32(0);
    w.i32(0); // flags, value
    let mut name = [0u8; 32];
    name[..5].copy_from_slice(b"solid");
    w.0.extend_from_slice(&name);
    w.i32(-1);
    lumps[5] = w.0;

    // Solid leafs are outside every cluster in the two-cluster map, as in
    // a compiled map, so an entity touching the floor is not seen through it.
    let solid_cluster = if two_clusters { -1 } else { 0 };
    let solid_area = if split == Split::Areas { 0 } else { 1 };
    let mut w = Writer::default();
    leaf(&mut w, CONTENTS_SOLID, solid_cluster, solid_area, (0, 1)); // floor
    leaf(&mut w, 0, 0, 1, (0, 0)); // open air (+y half in a split map)
    leaf(&mut w, CONTENTS_SOLID, solid_cluster, solid_area, (1, 1)); // wall
    leaf(&mut w, CONTENTS_SOLID, solid_cluster, solid_area, (2, 1)); // *1 box
    match split {
        Split::None => {}
        Split::Clusters => leaf(&mut w, 0, 1, 1, (0, 0)), // open air, -y half
        Split::Areas => leaf(&mut w, 0, 0, 2, (0, 0)),    // open air, -y half
    }
    lumps[8] = w.0;

    if two_clusters {
        // dvis_t: numclusters, bitofs[cluster][PVS, PHS], then one
        // uncompressed row byte each: each cluster sees only itself;
        // cluster 0 hears both (row at 22), cluster 1 only itself.
        let mut w = Writer::default();
        w.i32(2);
        for (pvs, phs) in [(20, 22), (21, 21)] {
            w.i32(pvs);
            w.i32(phs);
        }
        w.0.extend_from_slice(&[0b01, 0b10, 0b11]);
        lumps[3] = w.0;
    }

    let mut w = Writer::default();
    w.u16(0);
    w.u16(1);
    w.u16(2);
    lumps[10] = w.0; // leaf brushes

    let mut w = Writer::default();
    for first in [0, 6, 12] {
        w.i32(first);
        w.i32(6);
        w.i32(CONTENTS_SOLID);
    }
    lumps[14] = w.0; // brushes

    let mut w = Writer::default();
    for plane in (0..12u16).chain(13..19) {
        w.u16(plane);
        w.i16(0);
    }
    lumps[15] = w.0; // brush sides

    let mut w = Writer::default();
    for v in [
        -500.0,
        -500.0,
        -500.0,
        WALL_X + 500.0,
        500.0,
        500.0,
        0.0,
        0.0,
        0.0,
    ] {
        w.f32(v);
    }
    w.i32(0);
    w.i32(0);
    w.i32(0); // head node, first face, num faces
    DOOR_MINS.iter().chain(&DOOR_MAXS).for_each(|&v| w.f32(v));
    (0..3).for_each(|_| w.f32(0.0)); // origin
    w.i32(2);
    w.i32(0);
    w.i32(0);
    lumps[13] = w.0; // world model, *1

    let mut w = Writer::default();
    if split == Split::Areas {
        // dareaportal_t: portalnum, otherarea; area 1 lists the first,
        // area 2 the second.
        let mut portals = Writer::default();
        for other_area in [2, 1] {
            portals.i32(1);
            portals.i32(other_area);
        }
        lumps[18] = portals.0;
        // darea_t: numareaportals, firstareaportal for areas 0, 1 and 2.
        for (count, first) in [(0, 0), (1, 0), (1, 1)] {
            w.i32(count);
            w.i32(first);
        }
    } else {
        (0..4).for_each(|_| w.i32(0)); // areas 0 and 1, no portals
    }
    lumps[17] = w.0;

    let mut out = Vec::new();
    out.extend_from_slice(b"IBSP");
    out.extend_from_slice(&38i32.to_le_bytes());
    let mut offset = 8 + 19 * 8;
    for lump in &lumps {
        out.extend_from_slice(&(offset as i32).to_le_bytes());
        out.extend_from_slice(&(lump.len() as i32).to_le_bytes());
        offset += lump.len();
    }
    for lump in &lumps {
        out.extend_from_slice(lump);
    }
    out
}

/// `gamedata/baseq2/pak0.pak`'s demo1 map, or `None` when the game data is
/// not installed (the case in CI). A test that returns early on `None`
/// passes without checking anything, so the skip is printed to keep it
/// visible in `cargo test -- --nocapture` and in failure output.
pub fn demo1_bsp() -> Option<Vec<u8>> {
    stock_bsp("demo1")
}

/// `maps/<name>.bsp` from `gamedata/baseq2/pak0.pak`, or `None` (printing
/// the skip) when the game data is not installed, as for `demo1_bsp`.
pub fn stock_bsp(name: &str) -> Option<Vec<u8>> {
    let pak = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gamedata/baseq2");
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    let map = fs
        .add_game_directory(std::path::Path::new(pak))
        .ok()
        .and_then(|_| fs.load_file(&format!("maps/{name}.bsp")).ok());
    if map.is_none() {
        eprintln!("SKIPPED: gamedata/baseq2/pak0.pak not installed; {name} test checks nothing");
    }
    map
}

/// A server and the concrete `GameWorld` it runs, with one client in the
/// game, for tests that inspect game state `LocalGame` keeps behind
/// `GameExport`. `frame` and `think` drive it as `LocalGame` does.
pub struct ServerWorld {
    pub sv: q2_server::state::Server,
    pub svs: q2_server::state::ServerStatic,
    pub gi: std::rc::Rc<q2_server::ServerGameImport>,
    pub game: q2_game::world::GameWorld,
}

impl ServerWorld {
    /// `bsp` loaded as `maps/<name>.bsp` with the client at its player start.
    pub fn start(name: &str, bsp: &[u8]) -> Self {
        use q2_game::traits::GameExport;
        let gi = std::rc::Rc::new(q2_server::ServerGameImport::new());
        let mut game = q2_game::world::GameWorld::new(gi.clone(), q2_shared::constants::MAX_EDICTS);
        game.set_max_clients(1);
        game.init();
        let mut svs = q2_server::state::ServerStatic::default();
        svs.init_game(1);
        let mut sv = q2_server::state::Server::default();
        sv.map(&mut svs, name, bsp, "", &gi, &mut game)
            .expect("map loads");
        svs.connect_local_client(&mut game, "")
            .expect("client accepted");
        Self { sv, svs, gi, game }
    }

    pub fn think(&mut self, cmd: &q2_shared::types::UserCmd) {
        use q2_game::traits::GameExport;
        self.game.client_think(1, cmd);
    }

    pub fn frame(&mut self, frametime_us: u32) {
        self.sv
            .frame(&mut self.svs, frametime_us, &mut self.game, &self.gi);
    }
}
