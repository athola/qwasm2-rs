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

#![allow(dead_code)] // each test binary uses a different subset

/// x coordinate of the wall's face.
pub const WALL_X: f32 = 256.0;

/// Entity string with a worldspawn and a player start on the floor.
pub const ENTSTRING: &str = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}\n";

const CONTENTS_SOLID: i32 = 1;

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

/// `dleaf_t` with only contents, area and the leaf-brush range set.
fn leaf(w: &mut Writer, contents: i32, first_brush: u16, num_brushes: u16) {
    w.i32(contents);
    w.i16(0); // cluster
    w.i16(1); // area
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
    let mut lumps: [Vec<u8>; 19] = Default::default();

    lumps[0] = format!("{ENTSTRING}\0").into_bytes();

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
    lumps[1] = w.0;

    let mut w = Writer::default();
    node(&mut w, 0, 1, -1); // z = 0: above -> node 1, below -> leaf 0
    node(&mut w, 12, -3, -2); // x = WALL_X: beyond -> leaf 2, before -> leaf 1
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

    let mut w = Writer::default();
    leaf(&mut w, CONTENTS_SOLID, 0, 1); // floor
    leaf(&mut w, 0, 0, 0); // open air
    leaf(&mut w, CONTENTS_SOLID, 1, 1); // wall
    lumps[8] = w.0;

    let mut w = Writer::default();
    w.u16(0);
    w.u16(1);
    lumps[10] = w.0; // leaf brushes

    let mut w = Writer::default();
    for first in [0, 6] {
        w.i32(first);
        w.i32(6);
        w.i32(CONTENTS_SOLID);
    }
    lumps[14] = w.0; // brushes

    let mut w = Writer::default();
    for plane in 0..12u16 {
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
    lumps[13] = w.0; // world model

    let mut w = Writer::default();
    (0..4).for_each(|_| w.i32(0));
    lumps[17] = w.0; // areas 0 and 1, no portals

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
/// not installed (always the case in CI).
pub fn demo1_bsp() -> Option<Vec<u8>> {
    let pak = concat!(env!("CARGO_MANIFEST_DIR"), "/../../gamedata/baseq2");
    let mut fs = q2_common::filesystem::FileSystem::new("baseq2");
    fs.add_game_directory(std::path::Path::new(pak)).ok()?;
    fs.load_file("maps/demo1.bsp").ok()
}
