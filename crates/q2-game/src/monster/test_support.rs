//! Fixtures the monster tests share: a flat floor world (`FloorImport`,
//! also used by the AI and player trail tests), the same floor whose traces
//! also hit one target box, the player (`ArenaImport`), worlds and players
//! built on them, the C move-table checkers, and RNG, sound and run helpers.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use q2_shared::constants::{CONTENTS_MONSTER, CONTENTS_SOLID, CONTENTS_WATER};
use q2_shared::protocol::SvcOp;
use q2_shared::types::*;

use crate::ai;
use crate::constants::*;
use crate::entity::{AnimFrameFn, ClientData, EntityKey, MonsterInfo, MonsterMove, ThinkFn};
use crate::misc::wire::{wire_world, Wire, WireImport};
use crate::spawn::{FinishFn, SpawnFn};
use crate::traits::{EntityLink, GameImport, ModelInfo};
use crate::world::{GameWorld, MockGameImport};

// ---------------------------------------------------------------------------
// Floor world: flat floor at z = 0 with an optional wall and water.
// `MockGameImport` traces never hit anything, so a walking monster would
// always find a ledge.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct FloorImport {
    pub inner: MockGameImport,
    /// Solid at and beyond this x, if set.
    pub wall_x: Option<f32>,
    /// Contents of the liquid filling the world below `water.0`, if set.
    pub water: Option<(f32, i32)>,
    /// `(x, z)`: the floor is at height z (a stair up, or a ledge down)
    /// at and beyond x, if set.
    pub step: Option<(f32, f32)>,
    /// Solid above this height, if set.
    pub ceiling: Option<f32>,
}

impl FloorImport {
    fn solid(&self, p: Vec3f) -> bool {
        p.z < self.floor_under(p.x, p.x)
            || self.wall_x.is_some_and(|w| p.x >= w)
            || self.ceiling.is_some_and(|c| p.z > c)
    }

    /// The highest floor under the x span `lo..=hi`.
    fn floor_under(&self, lo: f32, hi: f32) -> f32 {
        match self.step {
            Some((x, z)) if lo >= x => z,
            Some((x, z)) if hi >= x => z.max(0.0),
            _ => 0.0,
        }
    }
}

/// A world with the floor import and the world entity at number 0.
pub fn floor_world(wall_x: Option<f32>) -> GameWorld {
    floor_world_with(FloorImport {
        wall_x,
        ..Default::default()
    })
    .0
}

/// `floor_world` with a given import, kept to read what it recorded.
pub fn floor_world_with(import: FloorImport) -> (GameWorld, Rc<FloorImport>) {
    let gi = Rc::new(import);
    let mut world = GameWorld::new(gi.clone(), 64);
    world.entities.spawn_numbered(0).unwrap();
    (world, gi)
}

/// A live walking monster standing on the floor at (x, y).
pub fn walker(world: &mut GameWorld, x: f32, y: f32) -> EntityKey {
    let key = world.spawn().unwrap();
    let ground = world.entities.key_for_number(0);
    let ent = world.entities.get_mut(key).unwrap();
    ent.state.origin = Vec3f::new(x, y, 24.0);
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.yaw_speed = 20.0;
    ent.game.health = 100;
    ent.game.ground_entity = ground;
    ent.monsterinfo = Some(Box::default());
    world.link_entity(key);
    key
}

impl GameImport for FloorImport {
    fn trace(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        _pass_ent: Option<usize>,
        _content_mask: i32,
    ) -> Trace {
        let floor = |p: Vec3f| self.floor_under(p.x + mins.x, p.x + maxs.x);
        let in_ceiling = |p: Vec3f| self.ceiling.is_some_and(|c| p.z + maxs.z > c);
        let in_floor_or_wall =
            |p: Vec3f| p.z + mins.z < floor(p) || self.wall_x.is_some_and(|w| p.x + maxs.x > w);
        // Only the ceiling can be traced out of, as C's startsolid without
        // allsolid.
        let startsolid = in_ceiling(start) || in_floor_or_wall(start);
        if in_floor_or_wall(start) || (in_ceiling(start) && in_ceiling(end)) {
            return Trace {
                allsolid: true,
                startsolid: true,
                fraction: 0.0,
                endpos: start,
                ent_index: Some(0),
                ..Trace::default()
            };
        }
        let (mut fraction, mut normal) = (1.0_f32, Vec3f::ZERO);
        let floor_end = floor(end);
        if end.z + mins.z < floor_end {
            normal = Vec3f::Z;
            fraction = match self.step {
                // Level into a stair: stop at its face.
                Some((x, _)) if start.z <= end.z => {
                    normal = Vec3f::NEG_X;
                    ((x - (start.x + maxs.x)) / (end.x - start.x)).max(0.0)
                }
                _ => (start.z + mins.z - floor_end) / (start.z - end.z),
            };
        }
        if let Some(w) = self.wall_x.filter(|&w| end.x + maxs.x > w) {
            let wall = (w - (start.x + maxs.x)) / (end.x - start.x);
            if wall < fraction {
                fraction = wall;
                normal = Vec3f::NEG_X;
            }
        }
        Trace {
            fraction,
            endpos: start + (end - start) * fraction,
            plane: Plane {
                normal,
                ..Plane::default()
            },
            ent_index: (fraction < 1.0).then_some(0),
            startsolid,
            ..Trace::default()
        }
    }
    fn point_contents(&self, point: Vec3f) -> i32 {
        if self.solid(point) {
            CONTENTS_SOLID
        } else {
            self.water
                .filter(|&(top, _)| point.z < top)
                .map_or(0, |(_, contents)| contents)
        }
    }
    fn bprintf(&self, l: i32, m: &str) {
        self.inner.bprintf(l, m)
    }
    fn dprintf(&self, m: &str) {
        self.inner.dprintf(m)
    }
    fn cprintf(&self, e: Option<usize>, l: i32, m: &str) {
        self.inner.cprintf(e, l, m)
    }
    fn centerprintf(&self, e: Option<usize>, m: &str) {
        self.inner.centerprintf(e, m)
    }
    fn sound(&self, e: Option<usize>, c: i32, s: i32, v: f32, a: f32, t: f32) {
        self.inner.sound(e, c, s, v, a, t)
    }
    fn positioned_sound(&self, o: Vec3f, e: usize, c: i32, s: i32, v: f32, a: f32, t: f32) {
        self.inner.positioned_sound(o, e, c, s, v, a, t)
    }
    fn model_index(&self, n: &str) -> i32 {
        self.inner.model_index(n)
    }
    fn sound_index(&self, n: &str) -> i32 {
        self.inner.sound_index(n)
    }
    fn image_index(&self, n: &str) -> i32 {
        self.inner.image_index(n)
    }
    fn set_model(&self, e: usize, n: &str) -> ModelInfo {
        self.inner.set_model(e, n)
    }
    fn in_pvs(&self, a: Vec3f, b: Vec3f) -> bool {
        self.inner.in_pvs(a, b)
    }
    fn in_phs(&self, a: Vec3f, b: Vec3f) -> bool {
        self.inner.in_phs(a, b)
    }
    fn areas_connected(&self, a: i32, b: i32) -> bool {
        self.inner.areas_connected(a, b)
    }
    fn entity_areas(&self, n: usize) -> (i32, i32) {
        self.inner.entity_areas(n)
    }
    fn link_entity(&self, e: &EntityLink) {
        self.inner.link_entity(e)
    }
    fn unlink_entity(&self, n: usize) {
        self.inner.unlink_entity(n)
    }
    fn box_edicts(&self, mins: Vec3f, maxs: Vec3f, max: usize, area: i32) -> Vec<usize> {
        self.inner.box_edicts(mins, maxs, max, area)
    }
    fn configstring(&self, n: i32, s: &str) {
        self.inner.configstring(n, s)
    }
    fn write_byte(&self, c: i32) {
        self.inner.write_byte(c)
    }
    fn write_short(&self, c: i32) {
        self.inner.write_short(c)
    }
    fn write_long(&self, c: i32) {
        self.inner.write_long(c)
    }
    fn write_float(&self, f: f32) {
        self.inner.write_float(f)
    }
    fn write_string(&self, s: &str) {
        self.inner.write_string(s)
    }
    fn write_position(&self, p: Vec3f) {
        self.inner.write_position(p)
    }
    fn write_dir(&self, d: Vec3f) {
        self.inner.write_dir(d)
    }
    fn write_angle(&self, f: f32) {
        self.inner.write_angle(f)
    }
    fn multicast(&self, o: Vec3f, to: Multicast) {
        self.inner.multicast(o, to)
    }
    fn unicast(&self, e: usize, r: bool) {
        self.inner.unicast(e, r)
    }
    fn argc(&self) -> i32 {
        self.inner.argc()
    }
    fn argv(&self, n: i32) -> String {
        self.inner.argv(n)
    }
    fn args(&self) -> String {
        self.inner.args()
    }
    fn add_command_string(&self, t: &str) {
        self.inner.add_command_string(t)
    }
}

// ---------------------------------------------------------------------------
// Arena: the floor world whose monster and shot traces also hit one target
// box (the player), the way the server's traces hit entities. C
// `M_CheckAttack` needs its line-of-fire trace to reach the enemy.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ArenaImport {
    pub floor: FloorImport,
    /// Entity number, absmin, absmax of the box traces stop at.
    pub target: Cell<Option<(usize, Vec3f, Vec3f)>>,
    /// Every `write_*` and `multicast`, as `kind value` text.
    pub wire: RefCell<Vec<String>>,
}

impl ArenaImport {
    /// Make the target a box of half-size `half` around `center`.
    pub fn target_at(&self, center: Vec3f, half: f32, number: usize) {
        self.target.set(Some((
            number,
            center - Vec3f::splat(half),
            center + Vec3f::splat(half),
        )));
    }

    /// Fraction along `start..end` where a box `mins..maxs` first
    /// touches the target, and the face it hits, by slab intersection.
    fn target_hit(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
    ) -> Option<(f32, Vec3f)> {
        let (_, absmin, absmax) = self.target.get()?;
        let (lo, hi) = (absmin - maxs, absmax - mins);
        let (mut enter, mut exit, mut normal) = (0.0_f32, 1.0_f32, Vec3f::ZERO);
        for axis in 0..3 {
            let (s, d) = (start[axis], end[axis] - start[axis]);
            if d == 0.0 {
                if s < lo[axis] || s > hi[axis] {
                    return None;
                }
                continue;
            }
            let (a, b) = ((lo[axis] - s) / d, (hi[axis] - s) / d);
            if a.min(b) > enter {
                enter = a.min(b);
                normal = Vec3f::ZERO;
                normal[axis] = -d.signum();
            }
            exit = exit.min(a.max(b));
        }
        (enter <= exit).then_some((enter, normal))
    }

    fn record(&self, line: String) {
        self.wire.borrow_mut().push(line);
    }
}

impl GameImport for ArenaImport {
    fn trace(
        &self,
        start: Vec3f,
        mins: Vec3f,
        maxs: Vec3f,
        end: Vec3f,
        pass_ent: Option<usize>,
        mask: i32,
    ) -> Trace {
        // Record the pass entity and mask on the mock.
        self.floor
            .inner
            .trace(start, mins, maxs, end, pass_ent, mask);
        let floor = self.floor.trace(start, mins, maxs, end, pass_ent, mask);
        let target = self.target.get().map(|t| t.0);
        match self.target_hit(start, mins, maxs, end) {
            // An entity box only stops masks with CONTENTS_MONSTER, as
            // C SV_ClipMoveToEntities clips against its box contents.
            Some((fraction, normal))
                if mask & CONTENTS_MONSTER != 0
                    && target != pass_ent
                    && fraction < floor.fraction =>
            {
                Trace {
                    fraction,
                    endpos: start + (end - start) * fraction,
                    startsolid: fraction == 0.0,
                    plane: Plane {
                        normal,
                        ..Plane::default()
                    },
                    contents: CONTENTS_MONSTER,
                    ent_index: target,
                    ..Trace::default()
                }
            }
            _ => floor,
        }
    }
    fn point_contents(&self, p: Vec3f) -> i32 {
        self.floor.point_contents(p)
    }

    fn bprintf(&self, l: i32, m: &str) {
        self.floor.bprintf(l, m)
    }
    fn dprintf(&self, m: &str) {
        self.floor.dprintf(m)
    }
    fn cprintf(&self, e: Option<usize>, l: i32, m: &str) {
        self.floor.cprintf(e, l, m)
    }
    fn centerprintf(&self, e: Option<usize>, m: &str) {
        self.floor.centerprintf(e, m)
    }
    fn sound(&self, e: Option<usize>, c: i32, s: i32, v: f32, a: f32, t: f32) {
        self.floor.sound(e, c, s, v, a, t)
    }
    fn positioned_sound(&self, o: Vec3f, e: usize, c: i32, s: i32, v: f32, a: f32, t: f32) {
        self.floor.positioned_sound(o, e, c, s, v, a, t)
    }
    fn model_index(&self, n: &str) -> i32 {
        self.floor.model_index(n)
    }
    fn sound_index(&self, n: &str) -> i32 {
        self.floor.sound_index(n)
    }
    fn image_index(&self, n: &str) -> i32 {
        self.floor.image_index(n)
    }
    fn set_model(&self, e: usize, n: &str) -> ModelInfo {
        self.floor.set_model(e, n)
    }
    fn in_pvs(&self, a: Vec3f, b: Vec3f) -> bool {
        self.floor.in_pvs(a, b)
    }
    fn in_phs(&self, a: Vec3f, b: Vec3f) -> bool {
        self.floor.in_phs(a, b)
    }
    fn areas_connected(&self, a: i32, b: i32) -> bool {
        self.floor.areas_connected(a, b)
    }
    fn entity_areas(&self, n: usize) -> (i32, i32) {
        self.floor.entity_areas(n)
    }
    fn link_entity(&self, e: &EntityLink) {
        self.floor.link_entity(e)
    }
    fn unlink_entity(&self, n: usize) {
        self.floor.unlink_entity(n)
    }
    fn box_edicts(&self, mins: Vec3f, maxs: Vec3f, max: usize, area: i32) -> Vec<usize> {
        self.floor.box_edicts(mins, maxs, max, area)
    }
    fn configstring(&self, n: i32, s: &str) {
        self.floor.configstring(n, s)
    }
    fn unicast(&self, e: usize, r: bool) {
        self.floor.unicast(e, r)
    }
    fn argc(&self) -> i32 {
        self.floor.argc()
    }
    fn argv(&self, n: i32) -> String {
        self.floor.argv(n)
    }
    fn args(&self) -> String {
        self.floor.args()
    }
    fn add_command_string(&self, t: &str) {
        self.floor.add_command_string(t)
    }
    fn write_byte(&self, c: i32) {
        self.record(format!("byte {:?}", c));
        self.floor.write_byte(c)
    }
    fn write_short(&self, c: i32) {
        self.record(format!("short {:?}", c));
        self.floor.write_short(c)
    }
    fn write_long(&self, c: i32) {
        self.record(format!("long {:?}", c));
        self.floor.write_long(c)
    }
    fn write_float(&self, f: f32) {
        self.record(format!("float {:?}", f));
        self.floor.write_float(f)
    }
    fn write_string(&self, s: &str) {
        self.record(format!("string {:?}", s));
        self.floor.write_string(s)
    }
    fn write_position(&self, p: Vec3f) {
        self.record(format!("position {:?}", p));
        self.floor.write_position(p)
    }
    fn write_dir(&self, d: Vec3f) {
        self.record(format!("dir {:?}", d));
        self.floor.write_dir(d)
    }
    fn write_angle(&self, f: f32) {
        self.record(format!("angle {:?}", f));
        self.floor.write_angle(f)
    }
    fn multicast(&self, o: Vec3f, to: Multicast) {
        self.record(format!("multicast {o:?} {to:?}"));
        self.floor.multicast(o, to)
    }
}

/// A `ArenaImport` world with the world entity at number 0, filled with
/// water below z = 1000 if `water`.
pub fn hit_world(water: bool) -> (GameWorld, Rc<ArenaImport>) {
    let floor = FloorImport {
        water: water.then_some((1000.0, CONTENTS_WATER)),
        ..Default::default()
    };
    let gi = Rc::new(ArenaImport {
        floor,
        ..Default::default()
    });
    let mut world = GameWorld::new(gi.clone(), 1024);
    world.entities.spawn_numbered(0).unwrap();
    (world, gi)
}

/// A live player (entity 1) at `origin`, this frame's sight client.
pub fn player_at(world: &mut GameWorld, origin: Vec3f) -> EntityKey {
    let key = world.spawn_test_client(crate::entity::ClientData::default());
    let ent = world.entities.get_mut(key).unwrap();
    ent.game.classname = "player".to_string();
    ent.state.origin = origin;
    ent.mins = Vec3f::new(-16.0, -16.0, -24.0);
    ent.maxs = Vec3f::new(16.0, 16.0, 32.0);
    ent.game.health = 100;
    ent.game.max_health = 100;
    ent.game.takedamage = crate::constants::TakeDamage::Aim;
    ent.game.viewheight = 22.0;
    world.link_entity(key);
    world.level.sight_client_key = Some(key);
    key
}

/// A player at `origin` that `gi`'s monster and shot traces hit.
pub fn hit_player(world: &mut GameWorld, gi: &ArenaImport, origin: Vec3f) -> EntityKey {
    let key = player_at(world, origin);
    let ent = world.entities.get(key).unwrap();
    let number = world.number_of(key).unwrap();
    gi.target
        .set(Some((number, origin + ent.mins, origin + ent.maxs)));
    key
}

/// A floor world with the world entity, and a live player standing at
/// (x, 0) as the sight client and the traces' target.
pub fn player_arena(x: f32) -> (GameWorld, Rc<ArenaImport>, EntityKey) {
    let gi = Rc::new(ArenaImport::default());
    let mut world = GameWorld::new(gi.clone(), 64);
    world.entities.spawn_numbered(0).unwrap();
    let player = hit_player(&mut world, &gi, Vec3f::new(x, 0.0, 24.0));
    (world, gi, player)
}

/// A monster spawned at the origin facing +x, started as `call_spawn`
/// and its first think would, and a live player at (`player_x`, 0).
pub fn monster_arena(
    spawn: SpawnFn,
    finish: FinishFn,
    player_x: f32,
) -> (GameWorld, Rc<ArenaImport>, EntityKey, EntityKey) {
    let gi = Rc::new(ArenaImport::default());
    let mut world = GameWorld::new(gi.clone(), 64);
    world.entities.spawn_numbered(0).unwrap();
    let player = world.spawn_test_client(ClientData::default());
    let key = world.spawn().unwrap();
    world.entities.get_mut(key).unwrap().state.origin = Vec3f::new(0.0, 0.0, 24.0);
    spawn(&mut world.entities, key, &HashMap::new());
    finish(&mut world, key, &HashMap::new());
    assert!(ai::monster_start(&mut world, key));
    let think = world.entities.get(key).unwrap().think.unwrap();
    think(&mut world, key);

    let p = world.entities.get_mut(player).unwrap();
    p.game.classname = "player".to_string();
    p.state.origin = Vec3f::new(player_x, 0.0, 24.0);
    p.mins = Vec3f::new(-16.0, -16.0, -24.0);
    p.maxs = Vec3f::new(16.0, 16.0, 32.0);
    p.game.health = 100;
    p.game.viewheight = 22.0;
    p.game.takedamage = TakeDamage::Aim;
    world.link_entity(player);
    let p = world.entities.get(player).unwrap();
    gi.target
        .set(Some((world.number_of(player).unwrap(), p.absmin, p.absmax)));
    world.level.sight_client_key = Some(player);
    (world, gi, key, player)
}

/// Run a monster's `spawn::SPAWNS` row as `call_spawn` does: spawn
/// function, finish function, model, `monster_start`, link.
pub fn spawn_monster(world: &mut GameWorld, classname: &str, origin: Vec3f) -> EntityKey {
    let &(_, sfn, model, finish) = crate::spawn::find_spawn(classname).unwrap();
    let key = world.spawn().unwrap();
    world.entities.get_mut(key).unwrap().state.origin = origin;
    let fields = HashMap::from([("classname".to_string(), classname.to_string())]);
    sfn(&mut world.entities, key, &fields);
    if let Some(finish) = finish {
        finish(world, key, &fields);
    }
    if let Some(model) = model {
        world.set_model(key, model);
    }
    assert!(ai::monster_start(world, key));
    world.link_entity(key);
    key
}

// ---------------------------------------------------------------------------
// Wire-recording world (`misc::wire`): one spawned monster, scripted traces.
// ---------------------------------------------------------------------------

/// A wire-recording world at time 1 holding one spawned monster.
pub fn monster_world(spawn: SpawnFn) -> (GameWorld, Rc<WireImport>, EntityKey) {
    let (mut world, gi) = wire_world();
    world.level.time = 1.0;
    let key = world.spawn().unwrap();
    spawn(&mut world.entities, key, &std::collections::HashMap::new());
    (world, gi, key)
}

/// Give `key` a live enemy at `at` with C's player view height.
pub fn give_enemy(world: &mut GameWorld, key: EntityKey, at: Vec3f) -> EntityKey {
    let enemy = world.spawn().unwrap();
    let ent = world.entities.get_mut(enemy).unwrap();
    ent.state.origin = at;
    ent.game.health = 100;
    ent.game.viewheight = 22.0;
    world.entities.get_mut(key).unwrap().game.enemy = Some(enemy);
    enemy
}

/// The `MZ2_*` numbers of every muzzle flash written so far.
pub fn muzzle_flashes(gi: &WireImport) -> Vec<i32> {
    let wire = gi.wire.borrow();
    wire.windows(3)
        .filter_map(|w| match w {
            [Wire::Byte(op), Wire::Short(_), Wire::Byte(flash)]
                if *op == SvcOp::MuzzleFlash2 as i32 =>
            {
                Some(*flash)
            }
            _ => None,
        })
        .collect()
}

/// Every entity of `classname`.
pub fn all_of(world: &GameWorld, classname: &str) -> Vec<EntityKey> {
    let mut found = Vec::new();
    while let Some(k) = world.find_by_classname(found.last().copied(), classname) {
        found.push(k);
    }
    found
}

/// Queue `n` traces that reach their end and report `target` as hit:
/// clear for `visible` and a clear shot for `M_CheckAttack`.
pub fn clear_shots_at(
    gi: &WireImport,
    world: &GameWorld,
    target: EntityKey,
    endpos: Vec3f,
    n: usize,
) {
    let hit = Trace {
        fraction: 1.0,
        endpos,
        ent_index: world.number_of(target),
        ..Trace::default()
    };
    gi.inner
        .queued_traces
        .borrow_mut()
        .extend(std::iter::repeat_n(hit, n));
}

/// Start the monster as `call_spawn` does, then run its thinks a
/// frame at a time until `done` or `frames` run out.
pub fn run_monster(
    world: &mut GameWorld,
    key: EntityKey,
    frames: usize,
    done: impl Fn(&GameWorld) -> bool,
) -> bool {
    crate::ai::monster_start(world, key);
    world.link_entity(key);
    for _ in 0..frames {
        world.level.time += crate::constants::FRAMETIME;
        crate::ai::ai_set_sight_client(world);
        world.run_think(key);
        if done(world) {
            return true;
        }
    }
    false
}

/// (channel, sound name, attenuation) of every sound played so far.
pub fn wire_sounds(gi: &WireImport) -> Vec<(i32, String, f32)> {
    let names = gi.inner.sounds.borrow();
    gi.sounds
        .borrow()
        .iter()
        .map(|&(_, chan, index, attn)| (chan, names[index as usize - 1].clone(), attn))
        .collect()
}

// ---------------------------------------------------------------------------
// Sounds, monster state, RNG and running frames
// ---------------------------------------------------------------------------

/// (channel, sound name, attenuation) of every sound played.
pub fn sounds(gi: &MockGameImport) -> Vec<(i32, String, f32)> {
    let names = gi.sounds.borrow();
    gi.sound_calls
        .borrow()
        .iter()
        .map(|&(_, chan, index, _, attn, _)| (chan, names[index as usize - 1].clone(), attn))
        .collect()
}

/// Names of the sounds `key` played, in order, as (channel, name, attenuation).
pub fn sounds_of(
    world: &GameWorld,
    gi: &MockGameImport,
    key: EntityKey,
) -> Vec<(i32, String, f32)> {
    let number = world.number_of(key);
    let names = gi.sounds.borrow();
    gi.sound_calls
        .borrow()
        .iter()
        .filter(|c| c.0 == number)
        .map(|c| (c.1, names[c.2 as usize - 1].clone(), c.4))
        .collect()
}

pub fn mi(world: &GameWorld, key: EntityKey) -> &MonsterInfo {
    world
        .entities
        .get(key)
        .unwrap()
        .monsterinfo
        .as_ref()
        .unwrap()
}

pub fn mi_mut(world: &mut GameWorld, key: EntityKey) -> &mut MonsterInfo {
    world
        .entities
        .get_mut(key)
        .unwrap()
        .monsterinfo
        .as_mut()
        .unwrap()
}

/// Whether `key` is playing `mv` (C compares `currentmove` pointers).
pub fn playing(world: &GameWorld, key: EntityKey, mv: &MonsterMove) -> bool {
    mi(world, key)
        .currentmove
        .as_ref()
        .is_some_and(|cm| std::ptr::eq(cm.frames, mv.frames))
}

/// The next trace hits `target` halfway.
pub fn next_trace_hits(world: &GameWorld, gi: &MockGameImport, target: EntityKey) {
    let at = world.entities.get(target).unwrap().state.origin;
    *gi.next_trace.borrow_mut() = Some(Trace {
        fraction: 0.5,
        endpos: at,
        allsolid: false,
        startsolid: false,
        plane: Plane::default(),
        surface: None,
        contents: 0,
        ent_index: world.number_of(target),
    });
}

/// Run whole game frames until `done` or `frames` run out; the frame
/// count it took.
pub fn run_until(
    world: &mut GameWorld,
    frames: usize,
    mut done: impl FnMut(&GameWorld) -> bool,
) -> Option<usize> {
    (1..=frames).find(|_| {
        world.run_frame();
        done(world)
    })
}

/// Run the monster's physics and think `frames` times, a frame apart
/// (C `G_RunEntity`), calling `observe` after each.
pub fn run(
    world: &mut GameWorld,
    key: EntityKey,
    frames: usize,
    mut observe: impl FnMut(&GameWorld),
) {
    for _ in 0..frames {
        world.level.framenum += 1;
        world.level.time = world.level.framenum as f32 * FRAMETIME;
        world.entities.set_level_time(world.level.time);
        world.run_entity(key);
        observe(world);
    }
}

/// Run `frames` monster thinks on `key` (C `monster_think` every
/// 0.1 s), with the level clock and sight client as `G_RunFrame` keeps
/// them.
pub fn think_for(world: &mut GameWorld, key: EntityKey, frames: usize) {
    for _ in 0..frames {
        world.level.framenum += 1;
        world.level.time = world.level.framenum as f32 * crate::constants::FRAMETIME;
        let think = world.entities.get(key).and_then(|e| e.think);
        if let Some(think) = think {
            think(world, key);
        }
        // Projectiles fly on their own; only the monster thinks here.
    }
}

/// Set the RNG so that `rolls`, replayed on it, returns true.
pub fn seed(world: &mut GameWorld, rolls: impl Fn(&mut GameWorld) -> bool) {
    let seed = (0..100_000)
        .find(|&s| {
            world.level.rand_state = s;
            rolls(world)
        })
        .expect("no seed gives those rolls");
    world.level.rand_state = seed;
}

/// Seed the random sequence so the next `random()` satisfies `want`.
pub fn seed_random(world: &mut GameWorld, want: impl Fn(f32) -> bool) {
    let seed = (0..100_000)
        .find(|&seed| {
            world.level.rand_state = seed;
            want(world.random())
        })
        .expect("no seed gives that roll");
    world.level.rand_state = seed;
}

#[test]
#[should_panic(expected = "no seed gives that roll")]
fn seed_random_fails_on_a_roll_no_seed_gives() {
    let mut world = crate::world::test_world();
    seed_random(&mut world, |r| r > 1.0);
}

// ---------------------------------------------------------------------------
// C move-table checkers, one per table style the monster tests use
// ---------------------------------------------------------------------------

/// One C `mmove_t`: the Rust move, C first and last frame numbers, C
/// endfunc name (`-` for NULL) and C's `mframe_t`s as `ai:dist:think`
/// (`ai_` dropped, `-` for NULL, `*n` for n equal frames in a row).
pub type CMoveRow = (&'static MonsterMove, i32, i32, &'static str, &'static str);

fn ai_name(f: Option<AnimFrameFn>) -> &'static str {
    let table: [(AnimFrameFn, &str); 6] = [
        (ai::ai_stand, "stand"),
        (ai::ai_walk, "walk"),
        (ai::ai_run, "run"),
        (ai::ai_charge, "charge"),
        (ai::ai_move, "move"),
        (ai::ai_turn, "turn"),
    ];
    f.and_then(|f| table.iter().find(|(g, _)| std::ptr::fn_addr_eq(*g, f)))
        .map_or("?", |(_, n)| n)
}

/// Every move matches its C `mmove_t` and `mframe_t` table. `names`
/// names the monster's think and end functions.
pub fn assert_moves_match_c(moves: &[CMoveRow], names: &[(ThinkFn, &str)]) {
    let name_of = |f: Option<ThinkFn>| match f {
        None => "-".to_string(),
        Some(f) => names
            .iter()
            .find(|(g, _)| std::ptr::fn_addr_eq(*g, f))
            .map_or("?".to_string(), |(_, n)| n.to_string()),
    };
    for &(mv, first, last, end, frames) in moves {
        assert_eq!((mv.firstframe, mv.lastframe), (first, last));
        assert_eq!(name_of(mv.endfunc), end, "endfunc of move {first}");
        let rust: Vec<String> = mv
            .frames
            .iter()
            .map(|f| format!("{}:{}:{}", ai_name(f.aifunc), f.dist, name_of(f.thinkfunc)))
            .collect();
        let c: Vec<String> = frames
            .split(' ')
            .flat_map(|tok| match tok.split_once('*') {
                Some((t, n)) => vec![t.to_string(); n.parse().unwrap()],
                None => vec![tok.to_string()],
            })
            .collect();
        assert_eq!(rust, c, "frames of move {first}");
    }
}

/// Pin a move to C: frame range, one aifunc throughout, the non-zero
/// distances and the think functions by frame index, the endfunc.
pub fn check_move(
    mv: &MonsterMove,
    (first, last): (i32, i32),
    aifunc: AnimFrameFn,
    dists: &[(usize, f32)],
    thinks: &[(usize, ThinkFn)],
    endfunc: Option<ThinkFn>,
) {
    assert_eq!((mv.firstframe, mv.lastframe), (first, last));
    assert_eq!(mv.frames.len() as i32, last - first + 1, "{first}");
    for (i, f) in mv.frames.iter().enumerate() {
        let want = dists.iter().find(|d| d.0 == i).map_or(0.0, |d| d.1);
        assert_eq!(f.dist, want, "move {first} frame {i} dist");
        assert!(
            f.aifunc.is_some_and(|a| std::ptr::fn_addr_eq(a, aifunc)),
            "move {first} frame {i} aifunc"
        );
        let want = thinks.iter().find(|t| t.0 == i).map(|t| t.1);
        match (f.thinkfunc, want) {
            (None, None) => {}
            (Some(have), Some(want)) => {
                assert!(
                    std::ptr::fn_addr_eq(have, want),
                    "move {first} frame {i} think"
                )
            }
            _ => panic!("move {first} frame {i} think presence"),
        }
    }
    match (mv.endfunc, endfunc) {
        (None, None) => {}
        (Some(have), Some(want)) => assert!(std::ptr::fn_addr_eq(have, want), "{first} end"),
        _ => panic!("move {first} endfunc presence"),
    }
}

/// One C `mmove_t`: bounds, each frame's aifunc as runs of
/// (count, fn), dists, (frame index, thinkfunc) pairs and endfunc.
pub struct CMove {
    pub bounds: (i32, i32),
    pub ai: Vec<(usize, Option<AnimFrameFn>)>,
    pub dists: Vec<f32>,
    pub thinks: Vec<(usize, ThinkFn)>,
    pub end: Option<ThinkFn>,
}

/// `n` zero distances.
pub fn zeros(n: usize) -> Vec<f32> {
    vec![0.0; n]
}

pub fn assert_move(name: &str, mv: &MonsterMove, c: CMove) {
    assert_eq!((mv.firstframe, mv.lastframe), c.bounds, "{name} bounds");
    assert_eq!(
        mv.frames.len() as i32,
        c.bounds.1 - c.bounds.0 + 1,
        "{name} length"
    );
    let ai: Vec<Option<AnimFrameFn>> =
        c.ai.iter()
            .flat_map(|&(n, f)| std::iter::repeat_n(f, n))
            .collect();
    assert_eq!(ai.len(), mv.frames.len(), "{name} ai runs");
    for (i, (frame, want)) in mv.frames.iter().zip(&ai).enumerate() {
        let same = match (frame.aifunc, want) {
            (None, None) => true,
            (Some(a), Some(b)) => std::ptr::fn_addr_eq(a, *b),
            _ => false,
        };
        assert!(same, "{name} frame {i} aifunc");
    }
    let dists: Vec<f32> = mv.frames.iter().map(|f| f.dist).collect();
    assert_eq!(dists, c.dists, "{name} dists");
    let thinks: Vec<usize> = (0..mv.frames.len())
        .filter(|&i| mv.frames[i].thinkfunc.is_some())
        .collect();
    let want: Vec<usize> = c.thinks.iter().map(|&(i, _)| i).collect();
    assert_eq!(thinks, want, "{name} think frames");
    for &(i, f) in &c.thinks {
        assert!(
            std::ptr::fn_addr_eq(mv.frames[i].thinkfunc.unwrap(), f),
            "{name} frame {i} thinkfunc"
        );
    }
    match (mv.endfunc, c.end) {
        (None, None) => {}
        (Some(a), Some(b)) => assert!(std::ptr::fn_addr_eq(a, b), "{name} endfunc"),
        _ => panic!("{name} endfunc"),
    }
}
