//! The last two server frames the client holds and the client clock that
//! interpolates between them. C ref: `cl.frame`, `cl.time` and the time
//! clamp of `CL_ParseFrame` (client/cl_parse.c), the lerp fraction of
//! `CL_AddEntities` and the player-state lerp of `CL_CalcViewValues`
//! (client/cl_entities.c).

use q2_shared::types::{EntityState, PlayerState, PmType, Vec3f};

use crate::entities::lerp_angle;

/// Milliseconds between server frames (C `servertime = serverframe * 100`).
const FRAME_MSEC: i32 = 100;

#[derive(Debug, Clone, Default)]
pub struct FrameHistory {
    /// Client time in milliseconds (C `cl.time`).
    time: i32,
    /// Microseconds not yet added to `time`.
    pending_us: u32,
    /// The newest server frame, `None` before the first.
    serverframe: Option<i32>,
    /// Entity states of the frame before `current` and of the newest frame,
    /// sorted by number.
    pub previous: Vec<EntityState>,
    pub current: Vec<EntityState>,
    old_ps: PlayerState,
    ps: PlayerState,
}

impl FrameHistory {
    /// Client time in milliseconds (C `cl.time`): what light styles,
    /// `EF_ROTATE` and the refdef time run on.
    pub fn time(&self) -> i32 {
        self.time
    }

    /// The newest server frame number, if one has arrived.
    pub fn serverframe(&self) -> Option<i32> {
        self.serverframe
    }

    /// Advance the client clock by `usec` (C `CL_Frame`'s
    /// `cl.time += timedelta / 1000`), carrying the sub-millisecond
    /// remainder that C drops once per frame.
    pub fn advance(&mut self, usec: u32) {
        let total = self.pending_us.saturating_add(usec);
        self.pending_us = total % 1000;
        self.time = self.time.wrapping_add((total / 1000) as i32);
    }

    /// Take server frame `serverframe` (C `CL_ParseFrame`): the newest
    /// frame becomes the previous one, and the clock is clamped to within
    /// one frame before the new frame's time.
    pub fn receive(&mut self, serverframe: i32, entities: Vec<EntityState>, ps: &PlayerState) {
        self.previous = std::mem::replace(&mut self.current, entities);
        self.old_ps = std::mem::replace(&mut self.ps, ps.clone());
        if self.serverframe.is_none() {
            self.old_ps = ps.clone();
        }
        self.serverframe = Some(serverframe);
        self.clamp_time();
    }

    /// How far the client clock is from the previous frame to the newest
    /// one, 0..=1 (C `cl.lerpfrac`), clamping the clock as `CL_AddEntities`
    /// does first.
    pub fn lerpfrac(&mut self) -> f32 {
        let Some(servertime) = self.clamp_time() else {
            return 1.0;
        };
        1.0 - (servertime - self.time) as f32 * 0.01
    }

    /// Clamp `time` into the newest frame's interval; its server time.
    fn clamp_time(&mut self) -> Option<i32> {
        let servertime = self.serverframe?.wrapping_mul(FRAME_MSEC);
        self.time = self.time.clamp(servertime - FRAME_MSEC, servertime);
        Some(servertime)
    }

    /// The view origin and angles (C `CL_CalcViewValues` with prediction):
    /// `origin` and `live` are the player's position and state now, which
    /// the local game updates every command like C's prediction does;
    /// the view offset and kick angles change only on server frames, so
    /// they are lerped between the last two by `lerp`. A dead player's
    /// angles are lerped too, as C stops using the predicted ones.
    pub fn view(&self, origin: Vec3f, live: &PlayerState, lerp: f32) -> (Vec3f, Vec3f) {
        let ps = &self.ps;
        // A teleport of more than 256 units on an axis is not lerped.
        let teleported = (0..3).any(|i| {
            (i32::from(self.old_ps.pmove.origin[i]) - i32::from(ps.pmove.origin[i])).abs() > 256 * 8
        });
        let ops = if teleported { ps } else { &self.old_ps };
        let lerp_angles = |a: Vec3f, b: Vec3f| {
            Vec3f::new(
                lerp_angle(a.x, b.x, lerp),
                lerp_angle(a.y, b.y, lerp),
                lerp_angle(a.z, b.z, lerp),
            )
        };
        let vieworg = origin + ops.viewoffset + (ps.viewoffset - ops.viewoffset) * lerp;
        let angles = if (live.pmove.pm_type as i32) < (PmType::Dead as i32) {
            live.viewangles
        } else {
            lerp_angles(ops.viewangles, ps.viewangles)
        };
        (
            vieworg,
            angles + lerp_angles(ops.kick_angles, ps.kick_angles),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps_with(viewoffset_z: f32, kick_pitch: f32) -> PlayerState {
        PlayerState {
            viewoffset: Vec3f::new(0.0, 0.0, viewoffset_z),
            kick_angles: Vec3f::new(kick_pitch, 0.0, 0.0),
            ..Default::default()
        }
    }

    #[test]
    fn lerpfrac_follows_the_client_clock_between_frames() {
        let mut history = FrameHistory::default();
        history.receive(10, Vec::new(), &PlayerState::default());
        // The clock is clamped to the frame before (900..=1000 ms).
        assert_eq!(history.time(), 900);
        assert_eq!(history.lerpfrac(), 0.0);
        history.advance(25_000);
        assert_eq!(history.lerpfrac(), 0.25);
        history.advance(200_000);
        assert_eq!(history.lerpfrac(), 1.0, "clamped at the newest frame");
        assert_eq!(history.time(), 1000);
    }

    #[test]
    fn a_late_frame_holds_entities_at_the_newest_one_and_an_early_frame_restarts_the_lerp() {
        let mut history = FrameHistory::default();
        history.receive(10, Vec::new(), &PlayerState::default());
        history.advance(130_000);
        assert_eq!(history.lerpfrac(), 1.0);
        history.receive(11, Vec::new(), &PlayerState::default());
        assert_eq!(
            history.lerpfrac(),
            0.0,
            "time 1000 is the start of 1000..1100"
        );
        history.advance(60_000);
        history.receive(12, Vec::new(), &PlayerState::default());
        assert_eq!(history.time(), 1100, "clamped up to the new frame's start");
    }

    #[test]
    fn sub_millisecond_time_is_carried() {
        let mut history = FrameHistory::default();
        for _ in 0..144 {
            history.advance(6_944);
        }
        assert_eq!(history.time(), 999);
    }

    #[test]
    fn receive_keeps_the_previous_frame_for_interpolation() {
        let mut history = FrameHistory::default();
        let state = |number| EntityState {
            number,
            ..Default::default()
        };
        history.receive(1, vec![state(1)], &PlayerState::default());
        history.receive(2, vec![state(2)], &PlayerState::default());
        assert_eq!(history.previous[0].number, 1);
        assert_eq!(history.current[0].number, 2);
        assert_eq!(history.serverframe(), Some(2));
    }

    #[test]
    fn lerpfrac_is_one_before_any_frame() {
        assert_eq!(FrameHistory::default().lerpfrac(), 1.0);
    }

    #[test]
    fn view_offset_and_kick_lerp_between_frames_and_angles_are_live() {
        let mut history = FrameHistory::default();
        history.receive(1, Vec::new(), &ps_with(22.0, 0.0));
        history.receive(2, Vec::new(), &ps_with(-2.0, 4.0));
        let mut live = ps_with(-2.0, 4.0);
        live.viewangles = Vec3f::new(0.0, 90.0, 0.0);
        let (org, angles) = history.view(Vec3f::new(1.0, 2.0, 3.0), &live, 0.25);
        assert_eq!(org, Vec3f::new(1.0, 2.0, 3.0 + 16.0));
        assert_eq!(angles, Vec3f::new(1.0, 90.0, 0.0));
    }

    #[test]
    fn first_frame_does_not_lerp_from_a_default_player_state() {
        let mut history = FrameHistory::default();
        history.receive(1, Vec::new(), &ps_with(22.0, 0.0));
        let (org, _) = history.view(Vec3f::ZERO, &ps_with(22.0, 0.0), 0.0);
        assert_eq!(org.z, 22.0);
    }

    #[test]
    fn teleport_does_not_lerp_the_view_offset() {
        let mut history = FrameHistory::default();
        history.receive(1, Vec::new(), &ps_with(22.0, 0.0));
        let mut far = ps_with(-2.0, 0.0);
        far.pmove.origin[1] = 257 * 8;
        history.receive(2, Vec::new(), &far);
        let (org, _) = history.view(Vec3f::ZERO, &far, 0.0);
        assert_eq!(org.z, -2.0);
    }

    #[test]
    fn dead_player_angles_lerp_between_frames() {
        let mut history = FrameHistory::default();
        let mut a = PlayerState::default();
        a.viewangles.y = 10.0;
        let mut b = a.clone();
        b.viewangles.y = 30.0;
        b.pmove.pm_type = PmType::Dead;
        history.receive(1, Vec::new(), &a);
        history.receive(2, Vec::new(), &b);
        let (_, angles) = history.view(Vec3f::ZERO, &b, 0.5);
        assert_eq!(angles.y, 20.0);
    }
}
