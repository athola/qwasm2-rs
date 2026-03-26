use q2_shared::types::*;
use q2_shared::constants::*;

/// Client connection states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnState {
    #[default]
    Disconnected,
    Connecting,
    Connected,
    Active,
}

/// Per-entity client-side state for interpolation
#[derive(Debug, Clone, Default)]
pub struct CEntity {
    pub baseline: EntityState,
    pub current: EntityState,
    pub prev: EntityState,
    pub lerp_origin: Vec3f,
}

/// Main client state (per-connection)
#[derive(Debug)]
pub struct ClientState {
    pub state: ConnState,
    pub timeoutcount: i32,
    /// Server time (in msec)
    pub servertime: i32,
    pub time: f32,
    /// View/render state
    pub viewangles: Vec3f,
    pub refdef: RefDefState,
    /// Player state
    pub frame: ClientFrame,
    pub predicted_origin: Vec3f,
    pub predicted_angles: Vec3f,
    /// Entity state (indexed by entity number)
    pub entities: Vec<CEntity>,
    /// Server info
    pub gamedir: String,
    pub playernum: i32,
    pub attractloop: bool,
    pub server_count: i32,
    /// Configstrings from server
    pub configstrings: Vec<String>,
    /// Models and images
    pub model_draw: Vec<Option<String>>,
    pub image_precache: Vec<Option<String>>,
    pub sound_precache: Vec<Option<String>>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self {
            state: ConnState::default(),
            timeoutcount: 0,
            servertime: 0,
            time: 0.0,
            viewangles: Vec3f::ZERO,
            refdef: RefDefState::default(),
            frame: ClientFrame::default(),
            predicted_origin: Vec3f::ZERO,
            predicted_angles: Vec3f::ZERO,
            entities: vec![CEntity::default(); MAX_EDICTS],
            gamedir: String::new(),
            playernum: 0,
            attractloop: false,
            server_count: 0,
            configstrings: vec![String::new(); MAX_CONFIGSTRINGS],
            model_draw: vec![None; MAX_MODELS],
            image_precache: vec![None; MAX_IMAGES],
            sound_precache: vec![None; MAX_SOUNDS],
        }
    }
}

/// Frame state received from server
#[derive(Debug, Clone, Default)]
pub struct ClientFrame {
    pub valid: bool,
    pub serverframe: i32,
    pub servertime: i32,
    pub deltaframe: i32,
    pub playerstate: PlayerState,
    pub num_entities: i32,
    pub parse_entities: i32,
}

/// View setup
#[derive(Debug, Clone, Default)]
pub struct RefDefState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub fov_x: f32,
    pub fov_y: f32,
}

/// Persistent client data (survives reconnects)
#[derive(Debug)]
pub struct ClientStatic {
    pub state: ConnState,
    pub realtime: f32,
    pub frametime: f32,
    /// Key bindings
    pub key_bindings: Vec<Option<String>>,
}

impl Default for ClientStatic {
    fn default() -> Self {
        Self {
            state: ConnState::default(),
            realtime: 0.0,
            frametime: 0.0,
            key_bindings: vec![None; 256],
        }
    }
}
