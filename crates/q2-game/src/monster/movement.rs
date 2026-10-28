//! Monster ground movement: step, turn, and chase.
//!
//! Port of `monster/misc/move.c` (C `m_move.c`) for walking, flying and
//! swimming monsters. C's `randk()` choices draw from
//! [`GameWorld::random_u32`].
//!
//! C ref: `~/Qwasm2/src/game/monster/misc/move.c`

use q2_shared::constants::CONTENTS_SOLID;
use q2_shared::types::*;

use crate::constants::*;
use crate::entity::EntityKey;
use crate::world::GameWorld;

/// C `DI_NODIR`.
const DI_NODIR: f32 = -1.0;

/// C `anglemod`: wrap to [0, 360) on the 16-bit angle grid.
pub fn anglemod(a: f32) -> f32 {
    (360.0 / 65536.0) * (((a * (65536.0 / 360.0)) as i32 & 65535) as f32)
}

/// C `vectoyaw` (g_utils.c), truncated to whole degrees like C.
pub fn vectoyaw(v: Vec3f) -> f32 {
    if v.x == 0.0 {
        if v.y > 0.0 {
            90.0
        } else if v.y < 0.0 {
            -90.0
        } else {
            0.0
        }
    } else {
        let yaw = (v.y.atan2(v.x).to_degrees() as i32) as f32;
        if yaw < 0.0 {
            yaw + 360.0
        } else {
            yaw
        }
    }
}

/// Whether the monster stands on ground under (nearly) its whole box.
/// C ref: `M_CheckBottom`.
pub fn m_check_bottom(world: &GameWorld, key: EntityKey) -> bool {
    let Some(ent) = world.entities.get(key) else {
        return false;
    };
    let mins = ent.state.origin + ent.mins;
    let maxs = ent.state.origin + ent.maxs;

    // If all corner points are solid, the monster is on world ground.
    let corner = |x: bool, y: bool, z: f32| {
        Vec3f::new(
            if x { maxs.x } else { mins.x },
            if y { maxs.y } else { mins.y },
            z,
        )
    };
    let corners = [(false, false), (false, true), (true, false), (true, true)];
    if corners
        .iter()
        .all(|&(x, y)| world.gi.point_contents(corner(x, y, mins.z - 1.0)) == CONTENTS_SOLID)
    {
        return true;
    }

    // The midpoint must be on ground and the corners within a step of it.
    let pass = world.number_of(key);
    let mid_start = Vec3f::new((mins.x + maxs.x) * 0.5, (mins.y + maxs.y) * 0.5, mins.z);
    let stop_z = mins.z - 2.0 * STEPSIZE;
    let down = |start: Vec3f| {
        world.gi.trace(
            start,
            Vec3f::ZERO,
            Vec3f::ZERO,
            Vec3f::new(start.x, start.y, stop_z),
            pass,
            MASK_MONSTERSOLID,
        )
    };
    let trace = down(mid_start);
    if trace.fraction == 1.0 {
        return false;
    }
    let mid = trace.endpos.z;
    corners.iter().all(|&(x, y)| {
        let trace = down(corner(x, y, mins.z));
        trace.fraction != 1.0 && mid - trace.endpos.z <= STEPSIZE
    })
}

/// Try to move the monster by `step`, stepping up or down at most
/// STEPSIZE and refusing to walk off ledges or into water. Returns
/// whether it moved. C ref: `SV_movestep` (walking branch).
pub fn sv_movestep(world: &mut GameWorld, key: EntityKey, step: Vec3f, relink: bool) -> bool {
    let Some(ent) = world.entities.get(key) else {
        return false;
    };
    if ent
        .game
        .flags
        .intersects(EntityFlags::FLY | EntityFlags::SWIM)
    {
        return sv_flystep(world, key, step, relink);
    }
    let old_origin = ent.state.origin;
    let (mins, maxs, waterlevel, flags) = (ent.mins, ent.maxs, ent.game.waterlevel, ent.game.flags);
    let nostep = ent
        .monsterinfo
        .as_ref()
        .is_some_and(|mi| mi.aiflags.contains(AiFlags::NOSTEP));
    let stepsize = if nostep { 1.0 } else { STEPSIZE };
    let pass = world.number_of(key);

    // Push down from a step height above the wished position.
    let mut new_origin = old_origin + step;
    new_origin.z += stepsize;
    let end = new_origin - Vec3f::new(0.0, 0.0, stepsize * 2.0);
    let mut trace = world
        .gi
        .trace(new_origin, mins, maxs, end, pass, MASK_MONSTERSOLID);
    if trace.allsolid {
        return false;
    }
    if trace.startsolid {
        new_origin.z -= stepsize;
        trace = world
            .gi
            .trace(new_origin, mins, maxs, end, pass, MASK_MONSTERSOLID);
        if trace.allsolid || trace.startsolid {
            return false;
        }
    }

    // Don't go in to water.
    if waterlevel == 0 {
        let test = trace.endpos + Vec3f::new(0.0, 0.0, mins.z + 1.0);
        if world.gi.point_contents(test) & MASK_WATER != 0 {
            return false;
        }
    }

    if trace.fraction == 1.0 {
        // The move is either off an edge, or the floor was pulled out
        // from under a monster that is trying to correct.
        if !flags.contains(EntityFlags::PARTIALGROUND) {
            return false; // walked off an edge
        }
        if let Some(ent) = world.entities.get_mut(key) {
            ent.state.origin += step;
            ent.game.ground_entity = None;
        }
        if relink {
            relink_and_touch(world, key);
        }
        return true;
    }

    // Check that the new position is on ground.
    if let Some(ent) = world.entities.get_mut(key) {
        ent.state.origin = trace.endpos;
    }
    if !m_check_bottom(world, key) {
        if flags.contains(EntityFlags::PARTIALGROUND) {
            if relink {
                relink_and_touch(world, key);
            }
            return true;
        }
        if let Some(ent) = world.entities.get_mut(key) {
            ent.state.origin = old_origin;
        }
        return false;
    }

    let ground = trace
        .ent_index
        .and_then(|n| world.entities.key_for_number(n));
    let ground_linkcount = ground
        .and_then(|g| world.entities.get(g))
        .map(|g| g.linkcount);
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.flags.remove(EntityFlags::PARTIALGROUND);
        ent.game.ground_entity = ground;
        if let Some(linkcount) = ground_linkcount {
            ent.game.groundentity_linkcount = linkcount;
        }
    }
    if relink {
        relink_and_touch(world, key);
    }
    true
}

/// Flying and swimming monsters don't step up: try the move rising or
/// sinking toward the goal, then (with an enemy) flat; a flyer won't
/// enter water, a swimmer won't leave it. C ref: `SV_movestep` (the
/// FL_FLY / FL_SWIM branch).
fn sv_flystep(world: &mut GameWorld, key: EntityKey, step: Vec3f, relink: bool) -> bool {
    for attempt in 0..2 {
        let Some(ent) = world.entities.get(key) else {
            return false;
        };
        let (origin, mins, maxs) = (ent.state.origin, ent.mins, ent.maxs);
        let (flags, waterlevel, enemy) = (ent.game.flags, ent.game.waterlevel, ent.game.enemy);
        let mut new_origin = origin + step;
        if let (0, Some(enemy)) = (attempt, enemy) {
            let goal = ent.game.goalentity.unwrap_or(enemy);
            if let Some(ent) = world.entities.get_mut(key) {
                ent.game.goalentity = Some(goal);
            }
            let (goal_z, goal_is_client) = world
                .entities
                .get(goal)
                .map_or((0.0, false), |g| (g.state.origin.z, g.client.is_some()));
            let dz = origin.z - goal_z;
            if goal_is_client {
                if dz > 40.0 {
                    new_origin.z -= 8.0;
                }
                let shallow_swimmer = flags.contains(EntityFlags::SWIM) && waterlevel < 2;
                if !shallow_swimmer && dz < 30.0 {
                    new_origin.z += 8.0;
                }
            } else {
                // As C: from up to 8 below the goal, dz is added, not
                // subtracted.
                new_origin.z += if dz > 8.0 {
                    -8.0
                } else if dz > 0.0 {
                    -dz
                } else if dz < -8.0 {
                    8.0
                } else {
                    dz
                };
            }
        }

        let trace = world.gi.trace(
            origin,
            mins,
            maxs,
            new_origin,
            world.number_of(key),
            MASK_MONSTERSOLID,
        );
        let test = trace.endpos + Vec3f::new(0.0, 0.0, mins.z + 1.0);
        // Fly monsters don't enter water voluntarily.
        if flags.contains(EntityFlags::FLY)
            && waterlevel == 0
            && world.gi.point_contents(test) & MASK_WATER != 0
        {
            return false;
        }
        // Swim monsters don't exit water voluntarily.
        if flags.contains(EntityFlags::SWIM)
            && waterlevel < 2
            && world.gi.point_contents(test) & MASK_WATER == 0
        {
            return false;
        }

        if trace.fraction == 1.0 {
            if let Some(ent) = world.entities.get_mut(key) {
                ent.state.origin = trace.endpos;
            }
            if relink {
                relink_and_touch(world, key);
            }
            return true;
        }
        if enemy.is_none() {
            break;
        }
    }
    false
}

fn relink_and_touch(world: &mut GameWorld, key: EntityKey) {
    world.link_entity(key);
    world.touch_triggers(key);
}

/// Turn toward `ideal_yaw` by at most `yaw_speed` degrees.
/// C ref: `M_ChangeYaw`.
pub fn m_change_yaw(world: &mut GameWorld, key: EntityKey) {
    let Some(ent) = world.entities.get_mut(key) else {
        return;
    };
    let current = anglemod(ent.state.angles.y);
    let ideal = ent.game.ideal_yaw;
    if current == ideal {
        return;
    }
    let mut turn = ideal - current;
    if ideal > current {
        if turn >= 180.0 {
            turn -= 360.0;
        }
    } else if turn <= -180.0 {
        turn += 360.0;
    }
    let speed = ent.game.yaw_speed;
    turn = turn.clamp(-speed, speed);
    ent.state.angles.y = anglemod(current + turn);
}

/// Turn toward `yaw` and step `dist` along it; the step is undone while
/// the monster still faces more than 45 degrees away.
/// C ref: `SV_StepDirection`.
pub fn sv_step_direction(world: &mut GameWorld, key: EntityKey, yaw: f32, dist: f32) -> bool {
    let Some(ent) = world.entities.get_mut(key) else {
        return false;
    };
    ent.game.ideal_yaw = yaw;
    m_change_yaw(world, key);

    let rad = yaw.to_radians();
    let step = Vec3f::new(rad.cos() * dist, rad.sin() * dist, 0.0);
    let Some(old_origin) = world.entities.get(key).map(|e| e.state.origin) else {
        return false;
    };
    let moved = sv_movestep(world, key, step, false);
    if moved {
        if let Some(ent) = world.entities.get_mut(key) {
            let delta = ent.state.angles.y - ent.game.ideal_yaw;
            if delta > 45.0 && delta < 315.0 {
                // Not turned far enough, so don't take the step.
                ent.state.origin = old_origin;
            }
        }
    }
    relink_and_touch(world, key);
    moved
}

/// Pick a new direction toward `goal` (C reads only the goal entity's
/// origin) when the straight step failed. C ref: `SV_NewChaseDir`.
pub fn sv_new_chase_dir(world: &mut GameWorld, key: EntityKey, goal: Vec3f, dist: f32) {
    let Some(actor) = world.entities.get(key) else {
        return;
    };
    let olddir = anglemod(((actor.game.ideal_yaw / 45.0) as i32 * 45) as f32);
    let turnaround = anglemod(olddir - 180.0);
    let deltax = goal.x - actor.state.origin.x;
    let deltay = goal.y - actor.state.origin.y;

    let mut d1 = if deltax > 10.0 {
        0.0
    } else if deltax < -10.0 {
        180.0
    } else {
        DI_NODIR
    };
    let mut d2 = if deltay < -10.0 {
        270.0
    } else if deltay > 10.0 {
        90.0
    } else {
        DI_NODIR
    };

    // Try the direct diagonal.
    if d1 != DI_NODIR && d2 != DI_NODIR {
        let tdir = if d1 == 0.0 {
            if d2 == 90.0 {
                45.0
            } else {
                315.0
            }
        } else if d2 == 90.0 {
            135.0
        } else {
            215.0
        };
        if tdir != turnaround && sv_step_direction(world, key, tdir, dist) {
            return;
        }
    }

    // Try the other directions: the longer axis first, or at random.
    if (world.random_u32() & 3) & 1 != 0 || (deltay as i32).abs() > (deltax as i32).abs() {
        std::mem::swap(&mut d1, &mut d2);
    }
    for d in [d1, d2] {
        if d != DI_NODIR && d != turnaround && sv_step_direction(world, key, d, dist) {
            return;
        }
    }

    // There is no direct path to the player, so pick another direction.
    if olddir != DI_NODIR && sv_step_direction(world, key, olddir, dist) {
        return;
    }
    // Search every direction, clockwise or anticlockwise at random.
    let mut directions: Vec<f32> = (0..=315).step_by(45).map(|d| d as f32).collect();
    if world.random_u32() & 1 == 0 {
        directions.reverse();
    }
    for tdir in directions {
        if tdir != turnaround && sv_step_direction(world, key, tdir, dist) {
            return;
        }
    }
    if turnaround != DI_NODIR && sv_step_direction(world, key, turnaround, dist) {
        return;
    }

    // Can't move.
    if let Some(ent) = world.entities.get_mut(key) {
        ent.game.ideal_yaw = olddir;
    }
    // A monster that can't move may not have a valid standing position.
    if !m_check_bottom(world, key) {
        if let Some(ent) = world.entities.get_mut(key) {
            ent.game.flags |= EntityFlags::PARTIALGROUND; // SV_FixCheckBottom
        }
    }
}

/// Whether the boxes are within `dist` of touching. C ref: `SV_CloseEnough`.
pub fn sv_close_enough(world: &GameWorld, key: EntityKey, goal: EntityKey, dist: f32) -> bool {
    let (Some(ent), Some(goal)) = (world.entities.get(key), world.entities.get(goal)) else {
        return false;
    };
    (0..3).all(|i| goal.absmin[i] <= ent.absmax[i] + dist && goal.absmax[i] >= ent.absmin[i] - dist)
}

/// Step toward `goalentity` along `ideal_yaw`, finding a new direction
/// when blocked. Only monsters on the ground move. C ref: `M_MoveToGoal`.
pub fn m_move_to_goal(world: &mut GameWorld, key: EntityKey, dist: f32) {
    let goal = world
        .entities
        .get(key)
        .and_then(|e| e.game.goalentity)
        .and_then(|g| world.entities.get(g))
        .map(|g| g.state.origin);
    m_move_toward(world, key, goal, dist);
}

/// `M_MoveToGoal` with the goal given by its origin, for `ai_run`'s
/// pursuit, where C points `goalentity` at a temporary edict.
pub fn m_move_toward(world: &mut GameWorld, key: EntityKey, goal: Option<Vec3f>, dist: f32) {
    let Some(ent) = world.entities.get(key) else {
        return;
    };
    let (enemy, ideal_yaw) = (ent.game.enemy, ent.game.ideal_yaw);
    if ent.game.ground_entity.is_none()
        && !ent
            .game
            .flags
            .intersects(EntityFlags::FLY | EntityFlags::SWIM)
    {
        return; // in the air
    }
    // Don't step into the enemy.
    if enemy.is_some_and(|e| sv_close_enough(world, key, e, dist)) {
        return;
    }
    // Bump around: a random new chase direction one step in four, as C.
    if world.random_u32() & 3 == 1 || !sv_step_direction(world, key, ideal_yaw, dist) {
        let in_use = world.entities.get(key).is_some_and(|e| e.in_use);
        if let (true, Some(goal)) = (in_use, goal) {
            sv_new_chase_dir(world, key, goal, dist);
        }
    }
}

/// Step `dist` along `yaw`, for monsters on the ground. C ref: `M_walkmove`.
pub fn m_walkmove(world: &mut GameWorld, key: EntityKey, yaw: f32, dist: f32) -> bool {
    let Some(ent) = world.entities.get(key) else {
        return false;
    };
    if ent.game.ground_entity.is_none()
        && !ent
            .game
            .flags
            .intersects(EntityFlags::FLY | EntityFlags::SWIM)
    {
        return false;
    }
    let rad = yaw.to_radians();
    let step = Vec3f::new(rad.cos() * dist, rad.sin() * dist, 0.0);
    sv_movestep(world, key, step, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monster::test_support::*;

    #[test]
    fn walkmove_steps_along_the_floor() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));

        let origin = world.entities.get(key).unwrap().state.origin;
        assert!(
            (origin - Vec3f::new(10.0, 0.0, 24.0)).length() < 0.01,
            "{origin}"
        );
    }

    #[test]
    fn walkmove_into_a_wall_leaves_the_monster_where_it_was() {
        let mut world = floor_world(Some(20.0));
        let key = walker(&mut world, 0.0, 0.0);

        // The box's front is at x = 16; a 10 unit step would cross x = 20.
        assert!(!m_walkmove(&mut world, key, 0.0, 10.0));

        assert_eq!(
            world.entities.get(key).unwrap().state.origin,
            Vec3f::new(0.0, 0.0, 24.0)
        );
    }

    #[test]
    fn walkmove_refuses_a_monster_in_the_air() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        world.entities.get_mut(key).unwrap().game.ground_entity = None;

        assert!(!m_walkmove(&mut world, key, 0.0, 10.0));
    }

    #[test]
    fn change_yaw_turns_by_at_most_yaw_speed_the_short_way() {
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.state.angles.y = 10.0;
            ent.game.ideal_yaw = 300.0;
        }

        m_change_yaw(&mut world, key);

        let yaw = world.entities.get(key).unwrap().state.angles.y;
        assert!((yaw - 350.0).abs() < 0.01, "{yaw}");
    }

    #[test]
    fn move_to_goal_finds_a_way_around_a_wall_toward_the_goal() {
        let mut world = floor_world(Some(20.0));
        let key = walker(&mut world, 0.0, 0.0);
        let goal = walker(&mut world, 0.0, 300.0);
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.goalentity = Some(goal);
            ent.game.ideal_yaw = 0.0; // straight into the wall
        }

        for _ in 0..10 {
            m_move_to_goal(&mut world, key, 10.0);
        }

        let origin = world.entities.get(key).unwrap().state.origin;
        assert!(origin.x + 16.0 <= 20.0, "{origin}");
        assert!(origin.y > 0.0, "{origin}");
    }

    #[test]
    fn move_to_goal_turns_toward_an_unseen_goal_on_open_floor() {
        // Nothing blocks the old heading, so only C's random 1-in-4
        // `SV_NewChaseDir` can turn the monster toward its goal.
        let mut world = floor_world(None);
        let key = walker(&mut world, 0.0, 0.0);
        let goal = walker(&mut world, 0.0, 600.0);
        {
            let ent = world.entities.get_mut(key).unwrap();
            ent.game.goalentity = Some(goal);
            ent.game.ideal_yaw = 0.0; // heading away from the goal's axis
        }

        for _ in 0..40 {
            m_move_to_goal(&mut world, key, 10.0);
        }

        let origin = world.entities.get(key).unwrap().state.origin;
        assert!(origin.y > 100.0, "never turned toward the goal: {origin}");
    }

    #[test]
    fn vectoyaw_matches_c() {
        assert_eq!(vectoyaw(Vec3f::new(0.0, 5.0, 0.0)), 90.0);
        assert_eq!(vectoyaw(Vec3f::new(0.0, -5.0, 0.0)), -90.0);
        assert_eq!(vectoyaw(Vec3f::new(-1.0, -1.0, 0.0)), 225.0);
    }

    /// A flyer at (0, 0, z) with no ground under it.
    fn flyer(world: &mut GameWorld, z: f32) -> EntityKey {
        let key = walker(world, 0.0, 0.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.state.origin.z = z;
        ent.game.ground_entity = None;
        ent.game.flags |= EntityFlags::FLY;
        key
    }

    #[test]
    fn walkmove_flies_a_flyer_through_the_air() {
        let mut world = floor_world(None);
        let key = flyer(&mut world, 200.0);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));

        let origin = world.entities.get(key).unwrap().state.origin;
        assert!(
            (origin - Vec3f::new(10.0, 0.0, 200.0)).length() < 0.01,
            "{origin}"
        );
    }

    #[test]
    fn walkmove_flyer_into_a_wall_does_not_move() {
        let mut world = floor_world(Some(20.0));
        let key = flyer(&mut world, 200.0);

        assert!(!m_walkmove(&mut world, key, 0.0, 10.0));

        assert_eq!(world.entities.get(key).unwrap().state.origin.x, 0.0);
    }

    #[test]
    fn walkmove_flyer_descends_8_toward_a_player_more_than_40_below() {
        let mut world = floor_world(None);
        let key = flyer(&mut world, 200.0);
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::new(300.0, 0.0, 24.0);
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));

        let ent = world.entities.get(key).unwrap();
        assert!(
            (ent.state.origin.z - 192.0).abs() < 0.01,
            "{}",
            ent.state.origin
        );
        assert_eq!(
            ent.game.goalentity,
            Some(player),
            "C takes the enemy as goal"
        );
    }

    #[test]
    fn walkmove_flyer_climbs_8_toward_a_player_less_than_30_below() {
        let mut world = floor_world(None);
        let key = flyer(&mut world, 40.0);
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::new(300.0, 0.0, 24.0);
        world.entities.get_mut(key).unwrap().game.enemy = Some(player);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));

        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 48.0).abs() < 0.01, "{z}");
    }

    #[test]
    fn walkmove_flyer_closes_height_on_a_non_player_goal_by_at_most_8() {
        let mut world = floor_world(None);
        let key = flyer(&mut world, 100.0);
        let goal = walker(&mut world, 300.0, 0.0);
        world.entities.get_mut(goal).unwrap().state.origin.z = 95.0;
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.enemy = Some(goal);
        ent.game.goalentity = Some(goal);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));
        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 95.0).abs() < 0.01, "within 8: matches the goal, {z}");

        world.entities.get_mut(goal).unwrap().state.origin.z = 200.0;
        assert!(m_walkmove(&mut world, key, 0.0, 10.0));
        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 103.0).abs() < 0.01, "{z}");

        // C adds dz for a goal up to 8 above, moving away from it.
        world.entities.get_mut(goal).unwrap().state.origin.z = 107.0;
        assert!(m_walkmove(&mut world, key, 0.0, 10.0));
        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 99.0).abs() < 0.01, "{z}");
    }

    #[test]
    fn walkmove_flyer_does_not_fly_into_water() {
        let (mut world, _) = floor_world_with(FloorImport {
            water: Some((300.0, q2_shared::constants::CONTENTS_WATER)),
            ..Default::default()
        });
        let key = flyer(&mut world, 200.0);

        assert!(!m_walkmove(&mut world, key, 0.0, 10.0));
    }

    #[test]
    fn walkmove_swimmer_does_not_swim_out_of_the_water() {
        let mut world = floor_world(None);
        let key = flyer(&mut world, 200.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.flags = EntityFlags::SWIM;
        ent.game.waterlevel = 1;

        assert!(!m_walkmove(&mut world, key, 0.0, 10.0));

        let (mut world, _) = floor_world_with(FloorImport {
            water: Some((300.0, q2_shared::constants::CONTENTS_WATER)),
            ..Default::default()
        });
        let key = flyer(&mut world, 200.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.flags = EntityFlags::SWIM;
        ent.game.waterlevel = 1;
        assert!(m_walkmove(&mut world, key, 0.0, 10.0));
    }

    #[test]
    fn walkmove_swimmer_out_of_its_depth_does_not_climb_toward_a_player() {
        let (mut world, _) = floor_world_with(FloorImport {
            water: Some((300.0, q2_shared::constants::CONTENTS_WATER)),
            ..Default::default()
        });
        let key = flyer(&mut world, 100.0);
        let player = world.spawn_test_client(crate::entity::ClientData::default());
        world.entities.get_mut(player).unwrap().state.origin = Vec3f::new(300.0, 0.0, 90.0);
        let ent = world.entities.get_mut(key).unwrap();
        ent.game.flags = EntityFlags::SWIM;
        ent.game.waterlevel = 1;
        ent.game.enemy = Some(player);

        assert!(m_walkmove(&mut world, key, 0.0, 10.0));

        let z = world.entities.get(key).unwrap().state.origin.z;
        assert!((z - 100.0).abs() < 0.01, "{z}");
    }
}
