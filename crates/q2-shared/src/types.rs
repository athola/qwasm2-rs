use serde::{Deserialize, Serialize};

/// Type alias for `glam::Vec3`. All crates should use this alias.
pub type Vec3f = glam::Vec3;

// ---------------------------------------------------------------------------
// Entity state — replaces entity_state_t (shared.h lines 107-131)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityState {
    pub number: i32,
    pub origin: Vec3f,
    pub angles: Vec3f,
    pub old_origin: Vec3f,
    pub modelindex: i32,
    pub modelindex2: i32,
    pub modelindex3: i32,
    pub modelindex4: i32,
    pub frame: i32,
    pub skinnum: i32,
    pub effects: u32,
    pub renderfx: i32,
    pub solid: i32,
    pub sound: i32,
    pub event: i32,
}

impl Default for EntityState {
    fn default() -> Self {
        Self {
            number: 0,
            origin: Vec3f::ZERO,
            angles: Vec3f::ZERO,
            old_origin: Vec3f::ZERO,
            modelindex: 0,
            modelindex2: 0,
            modelindex3: 0,
            modelindex4: 0,
            frame: 0,
            skinnum: 0,
            effects: 0,
            renderfx: 0,
            solid: 0,
            sound: 0,
            event: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Player‐movement type enum — replaces pmtype_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PmType {
    #[default]
    Normal,
    Spectator,
    Dead,
    Gib,
    Freeze,
}

// ---------------------------------------------------------------------------
// Player‐movement state — replaces pmove_state_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PmoveState {
    pub pm_type: PmType,
    pub origin: [i16; 3],
    pub velocity: [i16; 3],
    pub pm_flags: u8,
    pub pm_time: u8,
    pub gravity: i16,
    pub delta_angles: [i16; 3],
}

// ---------------------------------------------------------------------------
// Player state — replaces player_state_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub pmove: PmoveState,
    pub viewangles: Vec3f,
    pub viewoffset: Vec3f,
    pub kick_angles: Vec3f,
    pub gunangles: Vec3f,
    pub gunoffset: Vec3f,
    pub gunindex: i32,
    pub gunframe: i32,
    pub blend: [f32; 4],
    pub fov: f32,
    pub rdflags: i32,
    pub stats: [i16; 32],
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            pmove: PmoveState::default(),
            viewangles: Vec3f::ZERO,
            viewoffset: Vec3f::ZERO,
            kick_angles: Vec3f::ZERO,
            gunangles: Vec3f::ZERO,
            gunoffset: Vec3f::ZERO,
            gunindex: 0,
            gunframe: 0,
            blend: [0.0; 4],
            fov: 0.0,
            rdflags: 0,
            stats: [0; 32],
        }
    }
}

// ---------------------------------------------------------------------------
// User command — replaces usercmd_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserCmd {
    pub msec: u8,
    pub buttons: u8,
    pub angles: [i16; 3],
    pub forwardmove: i16,
    pub sidemove: i16,
    pub upmove: i16,
    pub impulse: u8,
    pub lightlevel: u8,
}

// ---------------------------------------------------------------------------
// Collision plane — replaces cplane_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plane {
    pub normal: Vec3f,
    pub dist: f32,
    pub plane_type: u8,
    pub signbits: u8,
}

impl Default for Plane {
    fn default() -> Self {
        Self {
            normal: Vec3f::ZERO,
            dist: 0.0,
            plane_type: 0,
            signbits: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Collision surface — replaces csurface_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Surface {
    pub name: String,
    pub flags: i32,
    pub value: i32,
}

// ---------------------------------------------------------------------------
// Trace result — replaces trace_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub allsolid: bool,
    pub startsolid: bool,
    pub fraction: f32,
    pub endpos: Vec3f,
    pub plane: Plane,
    pub surface: Option<Surface>,
    pub contents: i32,
    pub ent_index: Option<usize>,
}

impl Default for Trace {
    fn default() -> Self {
        Self {
            allsolid: false,
            startsolid: false,
            fraction: 1.0,
            endpos: Vec3f::ZERO,
            plane: Plane::default(),
            surface: None,
            contents: 0,
            ent_index: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Solid type enum — replaces solid_t
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Solid {
    #[default]
    Not,
    Trigger,
    Bbox,
    Bsp,
}

// ---------------------------------------------------------------------------
// Multicast destination enum — for gi.multicast
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Multicast {
    All,
    PHS,
    PVS,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec3f_is_glam_vec3() {
        let v = Vec3f::new(1.0, 2.0, 3.0);
        assert_eq!(v.x, 1.0);
        assert_eq!(v.y, 2.0);
        assert_eq!(v.z, 3.0);
    }

    #[test]
    fn vec3f_addition() {
        let a = Vec3f::new(1.0, 2.0, 3.0);
        let b = Vec3f::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vec3f::new(5.0, 7.0, 9.0));
    }

    #[test]
    fn entity_state_default() {
        let es = EntityState::default();
        assert_eq!(es.number, 0);
        assert_eq!(es.origin, Vec3f::ZERO);
        assert_eq!(es.effects, 0);
    }

    #[test]
    fn player_state_default() {
        let ps = PlayerState::default();
        assert_eq!(ps.pmove.pm_type, PmType::Normal);
        assert_eq!(ps.fov, 0.0);
        assert_eq!(ps.stats.len(), 32);
    }

    #[test]
    fn usercmd_fields() {
        let cmd = UserCmd {
            msec: 16,
            buttons: 1,
            angles: [100, 200, 0],
            forwardmove: 400,
            ..Default::default()
        };
        assert_eq!(cmd.msec, 16);
        assert_eq!(cmd.forwardmove, 400);
    }

    #[test]
    fn trace_default_fraction_is_one() {
        let t = Trace::default();
        assert_eq!(t.fraction, 1.0);
        assert!(!t.allsolid);
        assert!(!t.startsolid);
        assert_eq!(t.ent_index, None);
    }

    #[test]
    fn solid_enum_variants() {
        assert_ne!(Solid::Not, Solid::Bbox);
        assert_eq!(Solid::default(), Solid::Not);
    }

    #[test]
    fn pmove_state_fixed_point() {
        // 12.3 fixed point: value 8 = 1.0 in game units
        let pm = PmoveState {
            origin: [80, 160, 0], // 10.0, 20.0, 0.0 in game units
            ..Default::default()
        };
        assert_eq!(pm.origin[0], 80);
    }

    #[test]
    fn entity_state_serde_roundtrip() {
        let es = EntityState {
            number: 42,
            origin: Vec3f::new(100.0, 200.0, 300.0),
            modelindex: 7,
            ..Default::default()
        };
        let bytes = bincode::serialize(&es).unwrap();
        let es2: EntityState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(es2.number, 42);
        assert_eq!(es2.origin, Vec3f::new(100.0, 200.0, 300.0));
    }
}
