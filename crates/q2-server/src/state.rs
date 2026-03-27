//! Server and per-client state.
//!
//! Rust equivalents of `server_t`, `server_static_t`, and `client_t` from
//! `server.h` (C reference: Qwasm2/src/server/header/server.h).

use q2_shared::types::*;

/// Number of frames tracked for rate limiting (from C: RATE_MESSAGES).
const RATE_MESSAGES: usize = 10;

// ---------------------------------------------------------------------------
// Server state enum -- replaces server_state_t (ss_dead, ss_loading, ss_game)
// ---------------------------------------------------------------------------

/// High-level server state.
///
/// Mirrors `server_state_t` from the C codebase, omitting the legacy
/// `ss_cinematic`, `ss_demo`, and `ss_pic` variants which are not relevant
/// for the core game loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServerState {
    /// No map loaded.
    #[default]
    Dead,
    /// Spawning level edicts.
    Loading,
    /// Actively running.
    Game,
}

// ---------------------------------------------------------------------------
// Client connection state -- replaces client_state_t
// ---------------------------------------------------------------------------

/// Per-client connection state as tracked by the server.
///
/// Mirrors `client_state_t` (`cs_free`, `cs_connected`, `cs_spawned`).
/// The `cs_zombie` state from C is intentionally omitted; disconnected
/// clients are immediately moved to `Free` in the Rust implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClientState {
    /// Slot is available for a new connection.
    #[default]
    Free,
    /// Assigned to a client, but not yet in the game.
    Connected,
    /// Client is fully in the game.
    Spawned,
}

// ---------------------------------------------------------------------------
// Per-client server-side data -- replaces client_t
// ---------------------------------------------------------------------------

/// Server-side data for a single connected (or potentially connected) client.
///
/// Corresponds to `client_t` in the C reference. Fields that relate to
/// low-level networking (`netchan`, `datagram`, `download`) are omitted for
/// now and will be added when the networking layer is implemented.
#[derive(Debug)]
pub struct ServerClient {
    /// Connection state for this slot.
    pub state: ClientState,
    /// Player name extracted from userinfo, high-bit masked.
    pub name: String,
    /// Full userinfo string ("name\value\..." format).
    pub userinfo: String,
    /// Last movement command received from this client.
    pub last_cmd: UserCmd,
    /// Last frame acknowledged by the client (for delta compression).
    pub last_frame: i32,
    /// Millisecond budget for commands; reset periodically. Used to detect
    /// speed-hack cheating (C field: `commandMsec`).
    pub command_msec: i32,
    /// Player state sent to this client.
    pub ps: PlayerState,
    /// Entity index for this client's player entity (1-based).
    pub edict_num: usize,
    /// Current average ping in milliseconds.
    pub ping: i32,
    /// Network rate limit (bytes/s).
    pub rate: i32,
    /// Per-frame message sizes for rate limiting.
    pub message_size: [i32; RATE_MESSAGES],
    /// Number of messages suppressed by rate limiting.
    pub suppress_count: i32,
}

impl Default for ServerClient {
    fn default() -> Self {
        Self {
            state: ClientState::Free,
            name: String::new(),
            userinfo: String::new(),
            last_cmd: UserCmd::default(),
            last_frame: -1,
            command_msec: 0,
            ps: PlayerState::default(),
            edict_num: 0,
            ping: 0,
            rate: 5000, // default rate from C: SV_UserinfoChanged
            message_size: [0; RATE_MESSAGES],
            suppress_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Per-map server state -- replaces server_t
// ---------------------------------------------------------------------------

/// Server-level state that is reset on every map change.
///
/// Corresponds to `server_t` in the C reference. The multicast buffer and
/// demo-related fields are omitted for now.
#[derive(Debug)]
pub struct Server {
    /// Current server state (Dead / Loading / Game).
    pub state: ServerState,
    /// Map name (e.g. `"base1"`).
    pub name: String,
    /// Precached model paths.
    pub models: Vec<String>,
    /// Configstrings indexed by the CS_* offsets from `q2-shared`.
    pub configstrings: Vec<String>,
    /// Entity baselines for delta compression.
    pub baselines: Vec<EntityState>,
    /// Frame counter, incremented each server frame.
    pub framenum: i32,
    /// Current server time in seconds.
    pub time: f32,
    /// Duration of one server frame in seconds.
    pub frametime: f32,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            state: ServerState::Dead,
            name: String::new(),
            models: Vec::new(),
            configstrings: Vec::new(),
            baselines: Vec::new(),
            framenum: 0,
            time: 0.0,
            frametime: 0.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistent server data -- replaces server_static_t
// ---------------------------------------------------------------------------

/// Persistent server data that survives map changes.
///
/// Corresponds to `server_static_t` in the C reference. Challenge tracking,
/// demo recording, and network entity ring-buffer fields are omitted for now.
#[derive(Debug)]
pub struct ServerStatic {
    /// `true` once `SV_InitGame` has completed.
    pub initialized: bool,
    /// Wall-clock time in seconds (always increasing).
    pub realtime: f32,
    /// The map command string (e.g. `"*intro.cin+base1"`).
    pub mapcmd: String,
    /// Per-slot client data; length == `max_clients`.
    pub clients: Vec<ServerClient>,
    /// Maximum number of client slots.
    pub max_clients: usize,
}

impl Default for ServerStatic {
    fn default() -> Self {
        Self {
            initialized: false,
            realtime: 0.0,
            mapcmd: String::new(),
            clients: Vec::new(),
            max_clients: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_state_field_assignment() {
        let mut sv = Server::default();
        assert_eq!(sv.state, ServerState::Dead);

        sv.state = ServerState::Loading;
        sv.name = "base1".to_string();
        assert_eq!(sv.state, ServerState::Loading);
        assert_eq!(sv.name, "base1");

        sv.state = ServerState::Game;
        sv.framenum = 1;
        sv.time = 0.1;
        assert_eq!(sv.state, ServerState::Game);
        assert_eq!(sv.framenum, 1);
    }

    #[test]
    fn client_state_lifecycle() {
        let mut cl = ServerClient::default();
        assert_eq!(cl.state, ClientState::Free);

        // Client connects
        cl.state = ClientState::Connected;
        cl.name = "Player1".into();
        cl.userinfo = r"name\Player1\skin\male/grunt".into();
        assert_eq!(cl.state, ClientState::Connected);

        // Client spawns into game
        cl.state = ClientState::Spawned;
        cl.edict_num = 1;
        cl.command_msec = 1800;
        assert_eq!(cl.state, ClientState::Spawned);
        assert_eq!(cl.edict_num, 1);

        // Client disconnects
        cl.state = ClientState::Free;
        assert_eq!(cl.state, ClientState::Free);
    }

    #[test]
    fn server_static_client_allocation() {
        let mut svs = ServerStatic::default();
        assert!(!svs.initialized);

        svs.max_clients = 4;
        svs.clients = (0..4).map(|_| ServerClient::default()).collect();
        svs.initialized = true;

        assert!(svs.initialized);
        assert_eq!(svs.clients.len(), 4);
        assert!(svs.clients.iter().all(|c| c.state == ClientState::Free));
    }

    #[test]
    fn server_client_default_rate() {
        let cl = ServerClient::default();
        assert_eq!(cl.rate, 5000);
        assert_eq!(cl.last_frame, -1);
    }

    #[test]
    fn server_frame_accumulation() {
        let mut sv = Server::default();
        sv.state = ServerState::Game;
        sv.frametime = 0.1;

        for _ in 0..10 {
            sv.framenum += 1;
            sv.time += sv.frametime;
        }

        assert_eq!(sv.framenum, 10);
        assert!((sv.time - 1.0).abs() < 1e-5);
    }

    #[test]
    fn server_configstrings_and_baselines() {
        let mut sv = Server::default();
        sv.configstrings = vec![String::new(); 64];
        sv.configstrings[0] = "base1".into();
        sv.configstrings[1] = "models/world.bsp".into();

        assert_eq!(sv.configstrings[0], "base1");
        assert_eq!(sv.configstrings[1], "models/world.bsp");
        assert!(sv.configstrings[2].is_empty());
    }
}
