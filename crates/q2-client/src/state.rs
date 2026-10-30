use q2_shared::constants::*;
use q2_shared::types::*;

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
    /// The frame just received (C `cl.frame`), which the rest of the client
    /// reads. C keeps it apart from `frames` and copies the parsed frame into
    /// both (cl_parse.c `cl.frames[cl.frame.serverframe & UPDATE_MASK] =
    /// cl.frame`), so the two hold the same frame after a parse; this mirrors
    /// that rather than duplicating state C does not have.
    pub frame: ClientFrame,
    /// The delta bases (C `cl.frames[UPDATE_BACKUP]`): the last
    /// `UPDATE_BACKUP` frames by `serverframe & UPDATE_MASK`, which a later
    /// frame's `deltaframe` names.
    pub frames: Vec<ClientFrame>,
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
    /// svc_stufftext text not yet handed to the command buffer, in arrival
    /// order. C calls `Cbuf_AddText` as it parses; the host drains this with
    /// `take_stufftext` into `CmdSystem::cbuf_add_text` after each message.
    /// C runs `Cbuf_Execute` before parsing svc_serverdata, which this
    /// queue does not do: text stuffed ahead of a serverdata in the same
    /// message runs after it, and survives the state reset.
    pub stufftext: String,
    /// The centered message on screen (C `SCR_CenterPrint` state).
    pub centerprint: CenterPrint,
}

/// C `scr_centertime` default: seconds a center print stays up.
pub const SCR_CENTERTIME: f32 = 2.5;
/// C `scr_centerstring` is `char[1024]` filled by `Q_strlcpy`.
const CENTERSTRING_CHARS: usize = 1023;

/// C cl_screen.c center-print globals.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CenterPrint {
    /// C `scr_centerstring`, cut to 1023 chars.
    pub text: String,
    /// C `scr_centertime_off`: seconds left on screen.
    pub time_off: f32,
    /// C `scr_centertime_start`: `cl.time` when it arrived.
    pub time_start: f32,
    /// C `scr_center_lines`: 1 plus the newlines of the uncut string.
    pub lines: i32,
}

impl CenterPrint {
    /// C `SCR_CenterPrint` without its console echo.
    pub fn new(text: &str, time: f32) -> Self {
        Self {
            text: text.chars().take(CENTERSTRING_CHARS).collect(),
            time_off: SCR_CENTERTIME,
            time_start: time,
            lines: 1 + text.matches('\n').count() as i32,
        }
    }
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
            frames: (0..UPDATE_BACKUP).map(|_| ClientFrame::default()).collect(),
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
            stufftext: String::new(),
            centerprint: CenterPrint::default(),
        }
    }
}

impl ClientState {
    /// The queued svc_stufftext text, leaving the queue empty.
    pub fn take_stufftext(&mut self) -> String {
        std::mem::take(&mut self.stufftext)
    }
}

/// Frame state received from server.
///
/// `entities` is sorted by entity number; this replaces the C parse_entities
/// ring-buffer index with a per-frame Vec so there is no global shared state.
#[derive(Debug, Clone, Default)]
pub struct ClientFrame {
    pub valid: bool,
    pub serverframe: i32,
    pub servertime: i32,
    pub deltaframe: i32,
    pub areabits: [u8; MAX_MAP_AREAS / 8],
    pub playerstate: PlayerState,
    pub entities: Vec<EntityState>,
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
