use crate::state::{CEntity, ClientFrame, ClientState, ConnState};
use q2_common::net_msg::NetMsg;
use q2_shared::constants::*;
use q2_shared::protocol::*;
use q2_shared::types::*;

impl ClientState {
    /// Parse a complete server message buffer (may contain multiple opcodes).
    pub fn parse_server_message(&mut self, msg: &mut NetMsg) {
        while !msg.remaining_data().is_empty() {
            let cmd = msg.read_byte();
            let op = SvcOp::try_from(cmd as u8);

            match op {
                Ok(SvcOp::ServerData) => self.parse_server_data(msg),
                Ok(SvcOp::ConfigString) => self.parse_config_string(msg),
                Ok(SvcOp::SpawnBaseline) => self.parse_baseline(msg),
                Ok(SvcOp::Print) => self.parse_print(msg),
                Ok(SvcOp::StuffText) => self.parse_stufftext(msg),
                Ok(SvcOp::CenterPrint) => self.parse_centerprint(msg),
                Ok(SvcOp::Frame) => self.parse_frame(msg),
                Ok(SvcOp::Disconnect) => {
                    self.state = ConnState::Disconnected;
                    return;
                }
                Ok(SvcOp::Nop) => {}
                // PlayerInfo (17) and PacketEntities (18) are sub-commands of
                // svc_frame and MUST NOT appear as top-level opcodes.  If they
                // do, the stream is corrupt — stop parsing rather than silently
                // consuming zero bytes and corrupting everything that follows.
                Ok(SvcOp::PlayerInfo) | Ok(SvcOp::PacketEntities) => {
                    tracing::error!(
                        "Received opcode {} as a top-level message — protocol violation, \
                         aborting parse (expected only inside svc_frame)",
                        cmd
                    );
                    return;
                }
                _ => {
                    tracing::warn!("Unknown server command {} — stopping parse", cmd);
                    return;
                }
            }
        }
    }

    fn parse_server_data(&mut self, msg: &mut NetMsg) {
        let protocol = msg.read_long();
        self.server_count = msg.read_long();
        let _attractloop = msg.read_byte();
        self.gamedir = msg.read_string();
        self.playernum = msg.read_short();
        let _levelname = msg.read_string();

        tracing::info!(
            "Server data: protocol={}, player={}",
            protocol,
            self.playernum
        );

        self.entities.clear();
        self.entities.resize(MAX_EDICTS, CEntity::default());
        self.configstrings.clear();
        self.configstrings.resize(MAX_CONFIGSTRINGS, String::new());
    }

    fn parse_config_string(&mut self, msg: &mut NetMsg) {
        let index = msg.read_short();
        let value = msg.read_string();
        if index < 0 {
            tracing::warn!("Configstring: negative index {}", index);
            return;
        }
        let index = index as usize;
        if index < self.configstrings.len() {
            self.configstrings[index] = value;
        } else {
            tracing::warn!(
                "Configstring: index {} out of range (max {})",
                index,
                self.configstrings.len()
            );
        }
    }

    /// Parse a SpawnBaseline message: entity bits + full delta from null state.
    fn parse_baseline(&mut self, msg: &mut NetMsg) {
        let (num, bits) = parse_entity_bits(msg);
        let num = num as usize;
        if num > 0 && num < self.entities.len() {
            self.entities[num].baseline =
                read_delta_entity(msg, &EntityState::default(), num as i32, bits);
        } else if num != 0 {
            tracing::warn!("parse_baseline: entity number {} out of range", num);
            // Still must consume the delta bytes to keep stream aligned.
            read_delta_entity(msg, &EntityState::default(), num as i32, bits);
        }
    }

    fn parse_print(&mut self, msg: &mut NetMsg) {
        let _level = msg.read_byte();
        let text = msg.read_string();
        tracing::info!("[server] {}", text);
    }

    fn parse_stufftext(&mut self, msg: &mut NetMsg) {
        let _cmd = msg.read_string();
    }

    fn parse_centerprint(&mut self, msg: &mut NetMsg) {
        let _text = msg.read_string();
    }

    /// Parse a complete svc_frame message including player state and entity list.
    ///
    /// The frame wire layout (after the svc_frame opcode byte) is:
    ///   serverframe (i32) | deltaframe (i32) | suppress (u8)
    ///   | areabits_len (u8) | areabits ([u8; len])
    ///   | svc_playerinfo (u8=17) | player_state_data
    ///   | svc_packetentities (u8=18) | entity_list | terminator (0, 0)
    fn parse_frame(&mut self, msg: &mut NetMsg) {
        let serverframe = msg.read_long();
        let deltaframe = msg.read_long();
        let _suppress = msg.read_byte();
        let servertime = serverframe * 100;

        // areabits: length-prefixed raw bytes
        let areabits_len = (msg.read_byte() as usize).min(MAX_MAP_AREAS / 8);
        let mut areabits = [0u8; MAX_MAP_AREAS / 8];
        msg.read_data(&mut areabits[..areabits_len]);

        // Locate the old frame for delta (or None for uncompressed frame).
        let old_frame: Option<ClientFrame> = if deltaframe > 0 {
            let old_idx = (deltaframe as usize) & UPDATE_MASK;
            let candidate = &self.frames[old_idx];
            if candidate.serverframe == deltaframe && candidate.valid {
                Some(candidate.clone())
            } else {
                tracing::warn!(
                    "parse_frame: delta from invalid frame {} (have {}, valid={})",
                    deltaframe,
                    candidate.serverframe,
                    candidate.valid
                );
                None
            }
        } else {
            None // uncompressed (fresh) frame
        };

        let frame_valid = deltaframe <= 0 || old_frame.is_some();

        // svc_playerinfo must follow immediately.
        let ps_cmd = msg.read_byte();
        if ps_cmd != SvcOp::PlayerInfo as i32 {
            tracing::error!(
                "parse_frame: expected PlayerInfo (17), got {} — aborting frame",
                ps_cmd
            );
            return;
        }
        let old_ps = old_frame.as_ref().map(|f| &f.playerstate);
        let playerstate = read_player_state(msg, old_ps);

        // svc_packetentities must follow immediately.
        let pe_cmd = msg.read_byte();
        if pe_cmd != SvcOp::PacketEntities as i32 {
            tracing::error!(
                "parse_frame: expected PacketEntities (18), got {} — aborting frame",
                pe_cmd
            );
            return;
        }
        let old_entities = old_frame
            .as_ref()
            .map(|f| f.entities.as_slice())
            .unwrap_or(&[]);
        let entities = read_packet_entities(msg, old_entities, &self.entities);

        let new_frame = ClientFrame {
            valid: frame_valid,
            serverframe,
            servertime,
            deltaframe,
            areabits,
            playerstate,
            entities,
        };

        // Store in ring buffer and update active frame.
        let idx = (serverframe as usize) & UPDATE_MASK;
        self.frames[idx] = new_frame.clone();
        self.frame = new_frame;
    }
}

// ---------------------------------------------------------------------------
// Delta parsing helpers (free functions, no self needed)
// ---------------------------------------------------------------------------

/// Read the variable-length entity bits header and entity number.
///
/// Returns `(entity_number, UpdateFlags)`. Entity number 0 is the terminator.
/// Mirrors `CL_ParseEntityBits` from cl_parse.c.
fn parse_entity_bits(msg: &mut NetMsg) -> (i32, UpdateFlags) {
    let mut bits = msg.read_byte() as u32;
    if bits & UpdateFlags::MOREBITS1.bits() != 0 {
        bits |= (msg.read_byte() as u32) << 8;
    }
    if bits & UpdateFlags::MOREBITS2.bits() != 0 {
        bits |= (msg.read_byte() as u32) << 16;
    }
    if bits & UpdateFlags::MOREBITS3.bits() != 0 {
        bits |= (msg.read_byte() as u32) << 24;
    }
    let flags = UpdateFlags::from_bits_truncate(bits);
    let number = if flags.contains(UpdateFlags::NUMBER16) {
        msg.read_short()
    } else {
        msg.read_byte()
    };
    (number, flags)
}

/// Apply bit-flagged entity delta fields from `msg` on top of `from`.
///
/// Entity angles are encoded with `write_angle16` (our convention, which
/// differs from the original 8-bit C protocol).
fn read_delta_entity(
    msg: &mut NetMsg,
    from: &EntityState,
    number: i32,
    bits: UpdateFlags,
) -> EntityState {
    let mut e = from.clone();
    e.number = number;
    e.old_origin = from.origin;

    if bits.contains(UpdateFlags::MODEL) {
        e.modelindex = msg.read_byte();
    }
    if bits.contains(UpdateFlags::MODEL2) {
        e.modelindex2 = msg.read_byte();
    }
    if bits.contains(UpdateFlags::MODEL3) {
        e.modelindex3 = msg.read_byte();
    }
    if bits.contains(UpdateFlags::MODEL4) {
        e.modelindex4 = msg.read_byte();
    }

    // FRAME: FRAME8+FRAME16 = short (our encoder), FRAME8 only = byte.
    if bits.contains(UpdateFlags::FRAME8) && bits.contains(UpdateFlags::FRAME16) {
        e.frame = msg.read_short();
    } else if bits.contains(UpdateFlags::FRAME8) {
        e.frame = msg.read_byte();
    } else if bits.contains(UpdateFlags::FRAME16) {
        e.frame = msg.read_short();
    }

    // SKIN: SKIN8+SKIN16 = long, SKIN8 = byte, SKIN16 = short.
    if bits.contains(UpdateFlags::SKIN8) && bits.contains(UpdateFlags::SKIN16) {
        e.skinnum = msg.read_long();
    } else if bits.contains(UpdateFlags::SKIN8) {
        e.skinnum = msg.read_byte();
    } else if bits.contains(UpdateFlags::SKIN16) {
        e.skinnum = msg.read_short();
    }

    // EFFECTS: EFFECTS8+EFFECTS16 = long, EFFECTS8 = byte, EFFECTS16 = short.
    if bits.contains(UpdateFlags::EFFECTS8) && bits.contains(UpdateFlags::EFFECTS16) {
        e.effects = msg.read_long() as u32;
    } else if bits.contains(UpdateFlags::EFFECTS8) {
        e.effects = msg.read_byte() as u32;
    } else if bits.contains(UpdateFlags::EFFECTS16) {
        e.effects = msg.read_short() as u32;
    }

    // RENDERFX: RENDERFX8+RENDERFX16 = long, RENDERFX8 = byte, RENDERFX16 = short.
    if bits.contains(UpdateFlags::RENDERFX8) && bits.contains(UpdateFlags::RENDERFX16) {
        e.renderfx = msg.read_long();
    } else if bits.contains(UpdateFlags::RENDERFX8) {
        e.renderfx = msg.read_byte();
    } else if bits.contains(UpdateFlags::RENDERFX16) {
        e.renderfx = msg.read_short();
    }

    if bits.contains(UpdateFlags::ORIGIN1) {
        e.origin.x = msg.read_coord();
    }
    if bits.contains(UpdateFlags::ORIGIN2) {
        e.origin.y = msg.read_coord();
    }
    if bits.contains(UpdateFlags::ORIGIN3) {
        e.origin.z = msg.read_coord();
    }

    // Angles encoded as 16-bit (write_angle16 convention in our encoder).
    if bits.contains(UpdateFlags::ANGLE1) {
        e.angles.x = msg.read_angle16();
    }
    if bits.contains(UpdateFlags::ANGLE2) {
        e.angles.y = msg.read_angle16();
    }
    if bits.contains(UpdateFlags::ANGLE3) {
        e.angles.z = msg.read_angle16();
    }

    if bits.contains(UpdateFlags::OLDORIGIN) {
        e.old_origin.x = msg.read_coord();
        e.old_origin.y = msg.read_coord();
        e.old_origin.z = msg.read_coord();
    }

    if bits.contains(UpdateFlags::SOUND) {
        e.sound = msg.read_byte();
    }

    if bits.contains(UpdateFlags::EVENT) {
        e.event = msg.read_byte();
    } else {
        e.event = 0; // events are cleared each frame
    }

    if bits.contains(UpdateFlags::SOLID) {
        e.solid = msg.read_short();
    }

    e
}

/// Convert a byte read from the network to `PmType`. Out-of-range → `Normal`.
fn pm_type_from_byte(b: i32) -> PmType {
    match b {
        0 => PmType::Normal,
        1 => PmType::Spectator,
        2 => PmType::Dead,
        3 => PmType::Gib,
        4 => PmType::Freeze,
        _ => {
            tracing::warn!(
                "pm_type_from_byte: unknown pm_type {}, defaulting to Normal",
                b
            );
            PmType::Normal
        }
    }
}

/// Decode a delta-compressed player state.
///
/// `old` is `None` for an uncompressed (baseline) frame.
fn read_player_state(msg: &mut NetMsg, old: Option<&PlayerState>) -> PlayerState {
    let zero = PlayerState::default();
    let base = old.unwrap_or(&zero);
    let mut s = base.clone();

    let flags = PlayerStateFlags::from_bits_truncate(msg.read_short() as u16);

    if flags.contains(PlayerStateFlags::M_TYPE) {
        s.pmove.pm_type = pm_type_from_byte(msg.read_byte());
    }
    if flags.contains(PlayerStateFlags::M_ORIGIN) {
        s.pmove.origin[0] = msg.read_short() as i16;
        s.pmove.origin[1] = msg.read_short() as i16;
        s.pmove.origin[2] = msg.read_short() as i16;
    }
    if flags.contains(PlayerStateFlags::M_VELOCITY) {
        s.pmove.velocity[0] = msg.read_short() as i16;
        s.pmove.velocity[1] = msg.read_short() as i16;
        s.pmove.velocity[2] = msg.read_short() as i16;
    }
    if flags.contains(PlayerStateFlags::M_TIME) {
        s.pmove.pm_time = msg.read_byte() as u8;
    }
    if flags.contains(PlayerStateFlags::M_FLAGS) {
        s.pmove.pm_flags = msg.read_byte() as u8;
    }
    if flags.contains(PlayerStateFlags::M_GRAVITY) {
        s.pmove.gravity = msg.read_short() as i16;
    }
    if flags.contains(PlayerStateFlags::M_DELTA_ANGLES) {
        s.pmove.delta_angles[0] = msg.read_short() as i16;
        s.pmove.delta_angles[1] = msg.read_short() as i16;
        s.pmove.delta_angles[2] = msg.read_short() as i16;
    }
    if flags.contains(PlayerStateFlags::VIEWOFFSET) {
        s.viewoffset.x = msg.read_char() as f32 * 0.25;
        s.viewoffset.y = msg.read_char() as f32 * 0.25;
        s.viewoffset.z = msg.read_char() as f32 * 0.25;
    }
    if flags.contains(PlayerStateFlags::VIEWANGLES) {
        s.viewangles.x = msg.read_angle16();
        s.viewangles.y = msg.read_angle16();
        s.viewangles.z = msg.read_angle16();
    }
    if flags.contains(PlayerStateFlags::KICKANGLES) {
        s.kick_angles.x = msg.read_char() as f32 * 0.25;
        s.kick_angles.y = msg.read_char() as f32 * 0.25;
        s.kick_angles.z = msg.read_char() as f32 * 0.25;
    }
    if flags.contains(PlayerStateFlags::WEAPONINDEX) {
        s.gunindex = msg.read_byte();
    }
    if flags.contains(PlayerStateFlags::WEAPONFRAME) {
        s.gunframe = msg.read_byte();
        s.gunoffset.x = msg.read_char() as f32 * 0.25;
        s.gunoffset.y = msg.read_char() as f32 * 0.25;
        s.gunoffset.z = msg.read_char() as f32 * 0.25;
        s.gunangles.x = msg.read_char() as f32 * 0.25;
        s.gunangles.y = msg.read_char() as f32 * 0.25;
        s.gunangles.z = msg.read_char() as f32 * 0.25;
    }
    if flags.contains(PlayerStateFlags::BLEND) {
        s.blend[0] = msg.read_byte() as f32 / 255.0;
        s.blend[1] = msg.read_byte() as f32 / 255.0;
        s.blend[2] = msg.read_byte() as f32 / 255.0;
        s.blend[3] = msg.read_byte() as f32 / 255.0;
    }
    if flags.contains(PlayerStateFlags::FOV) {
        s.fov = msg.read_byte() as f32;
    }
    if flags.contains(PlayerStateFlags::RDFLAGS) {
        s.rdflags = msg.read_byte();
    }

    // Stats: always present; u32 to avoid signed shift-overflow on bit 31.
    let statbits = msg.read_long() as u32;
    for i in 0..MAX_STATS {
        if statbits & (1u32 << i) != 0 {
            s.stats[i] = msg.read_short() as i16;
        }
    }

    s
}

/// Decode a delta-compressed entity list from `msg`.
///
/// `old_entities` — sorted entity list from the delta frame (may be empty for
/// uncompressed frames).  `baselines` — per-entity baseline storage for new
/// entities that are not in the old frame.
///
/// Returns a new entity list sorted by entity number, ready to store in
/// `ClientFrame::entities`.
fn read_packet_entities(
    msg: &mut NetMsg,
    old_entities: &[EntityState],
    baselines: &[CEntity],
) -> Vec<EntityState> {
    let mut result: Vec<EntityState> = Vec::new();
    let mut old_idx = 0usize;

    loop {
        let (newnum, bits) = parse_entity_bits(msg);

        if newnum >= MAX_EDICTS as i32 {
            tracing::error!(
                "read_packet_entities: entity number {} >= MAX_EDICTS",
                newnum
            );
            break;
        }

        if newnum == 0 {
            break; // terminator
        }

        // Copy unchanged old entities whose number is less than this new one.
        while old_idx < old_entities.len() && old_entities[old_idx].number < newnum {
            result.push(old_entities[old_idx].clone());
            old_idx += 1;
        }

        if bits.contains(UpdateFlags::REMOVE) {
            // Entity is gone from this frame; skip the matching old entry.
            if old_idx < old_entities.len() && old_entities[old_idx].number == newnum {
                old_idx += 1;
            }
            continue;
        }

        // Determine delta base.
        let from: EntityState =
            if old_idx < old_entities.len() && old_entities[old_idx].number == newnum {
                let f = old_entities[old_idx].clone();
                old_idx += 1;
                f
            } else {
                // New entity — delta from its stored baseline.
                let baseline_idx = newnum as usize;
                if baseline_idx < baselines.len() {
                    baselines[baseline_idx].baseline.clone()
                } else {
                    EntityState::default()
                }
            };

        result.push(read_delta_entity(msg, &from, newnum, bits));
    }

    // Copy any remaining unchanged old entities.
    result.extend_from_slice(&old_entities[old_idx..]);

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ClientState;
    use q2_common::net_msg::NetMsg;

    /// Build a complete frame message that parses without error.
    /// deltaframe=-1 means uncompressed (no delta base required).
    fn write_empty_frame(
        msg: &mut NetMsg,
        serverframe: i32,
        deltaframe: i32,
        ps_new: Option<&PlayerState>,
        entities: &[EntityState],
    ) {
        let zero_ps = PlayerState::default();
        let ps = ps_new.unwrap_or(&zero_ps);
        let old_ps = PlayerState::default();

        msg.write_byte(SvcOp::Frame as i32);
        msg.write_long(serverframe);
        msg.write_long(deltaframe);
        msg.write_byte(0); // suppress
        msg.write_byte(0); // areabits len = 0
        msg.write_player_state(&old_ps, ps);
        msg.write_packet_entities_list(&[], entities);
    }

    fn make_frame_msg(serverframe: i32, deltaframe: i32) -> NetMsg {
        let mut msg = NetMsg::new();
        write_empty_frame(&mut msg, serverframe, deltaframe, None, &[]);
        msg.begin_reading();
        msg
    }

    // -------------------------------------------------------------------------
    // Existing tests (updated to write full frame payload)
    // -------------------------------------------------------------------------

    fn make_server_data_msg(
        protocol: i32,
        count: i32,
        playernum: i16,
        gamedir: &str,
        level: &str,
    ) -> NetMsg {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ServerData as i32);
        msg.write_long(protocol);
        msg.write_long(count);
        msg.write_byte(0);
        msg.write_string(gamedir);
        msg.write_short(playernum as i32);
        msg.write_string(level);
        msg.begin_reading();
        msg
    }

    #[test]
    fn parse_server_data_sets_fields() {
        let mut cs = ClientState::default();
        let mut msg = make_server_data_msg(34, 7, 2, "baseq2", "base1");
        cs.parse_server_message(&mut msg);

        assert_eq!(cs.server_count, 7);
        assert_eq!(cs.playernum, 2);
        assert_eq!(cs.gamedir, "baseq2");
        assert_eq!(cs.entities.len(), MAX_EDICTS);
        assert_eq!(cs.configstrings.len(), MAX_CONFIGSTRINGS);
    }

    #[test]
    fn parse_server_data_resets_entities() {
        let mut cs = ClientState::default();
        cs.entities.clear();
        cs.entities.resize(10, CEntity::default());

        let mut msg = make_server_data_msg(34, 1, 0, "baseq2", "q2dm1");
        cs.parse_server_message(&mut msg);

        assert_eq!(cs.entities.len(), MAX_EDICTS);
        assert_eq!(cs.configstrings.len(), MAX_CONFIGSTRINGS);
    }

    #[test]
    fn parse_config_string_stores_at_index() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(5);
        msg.write_string("models/world.bsp");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.configstrings[5], "models/world.bsp");
    }

    #[test]
    fn parse_config_string_oob_index_ignored() {
        let mut cs = ClientState::default();
        let original_len = cs.configstrings.len();

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(MAX_CONFIGSTRINGS as i32 + 100);
        msg.write_string("should_be_ignored");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.configstrings.len(), original_len);
    }

    #[test]
    fn parse_print_consumes_message() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Print as i32);
        msg.write_byte(0);
        msg.write_string("Hello world");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_frame_sets_serverframe() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(42, -1);
        cs.parse_server_message(&mut msg);
        assert_eq!(cs.frame.serverframe, 42);
        assert_eq!(cs.frame.deltaframe, -1);
    }

    #[test]
    fn parse_disconnect_sets_state() {
        let mut cs = ClientState::default();
        cs.state = ConnState::Active;

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.state, ConnState::Disconnected);
    }

    #[test]
    fn parse_nop_is_harmless() {
        let mut cs = ClientState::default();
        cs.state = ConnState::Active;

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Nop as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.state, ConnState::Active);
    }

    #[test]
    fn unknown_opcode_stops_parsing() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(255);
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.state, ConnState::Disconnected); // default
    }

    #[test]
    fn parse_multiple_messages_in_sequence() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();

        msg.write_byte(SvcOp::Nop as i32);

        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(10);
        msg.write_string("test_value");

        write_empty_frame(&mut msg, 100, -1, None, &[]);

        msg.begin_reading();
        cs.parse_server_message(&mut msg);

        assert_eq!(cs.configstrings[10], "test_value");
        assert_eq!(cs.frame.serverframe, 100);
        assert_eq!(cs.frame.deltaframe, -1);
    }

    #[test]
    fn parse_stufftext_consumes_string() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::StuffText as i32);
        msg.write_string("cmd connect");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_centerprint_consumes_string() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::CenterPrint as i32);
        msg.write_string("You found a secret!");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_baseline_consumes_entity_number() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::SpawnBaseline as i32);
        // Write entity 42 with no changed fields (empty delta from null).
        // parse_entity_bits: flags byte = 0 (no morebits, no NUMBER16), entity byte = 42.
        msg.write_byte(0); // flags = 0
        msg.write_byte(42); // entity number
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_config_string_negative_index_ignored() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(-1);
        msg.write_string("should_be_ignored");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(cs.configstrings.iter().all(|s| s.is_empty()));
    }

    // -------------------------------------------------------------------------
    // New round-trip tests for delta machinery
    // -------------------------------------------------------------------------

    #[test]
    fn parse_frame_stores_in_ring_buffer() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(5, -1);
        cs.parse_server_message(&mut msg);

        let idx = 5usize & UPDATE_MASK;
        assert_eq!(cs.frames[idx].serverframe, 5);
        assert!(cs.frames[idx].valid);
    }

    #[test]
    fn parse_frame_areabits_roundtrip() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        let zero_ps = PlayerState::default();

        msg.write_byte(SvcOp::Frame as i32);
        msg.write_long(1); // serverframe
        msg.write_long(-1); // deltaframe (uncompressed)
        msg.write_byte(0); // suppress

        // Write 4 bytes of areabits
        msg.write_byte(4);
        msg.write_byte(0xAB);
        msg.write_byte(0xCD);
        msg.write_byte(0xEF);
        msg.write_byte(0x12);

        msg.write_player_state(&zero_ps, &zero_ps);
        msg.write_packet_entities_list(&[], &[]);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.frame.areabits[0], 0xAB);
        assert_eq!(cs.frame.areabits[1], 0xCD);
        assert_eq!(cs.frame.areabits[2], 0xEF);
        assert_eq!(cs.frame.areabits[3], 0x12);
        assert_eq!(cs.frame.areabits[4], 0x00);
    }

    #[test]
    fn player_state_delta_roundtrip() {
        let old_ps = PlayerState::default();
        let mut new_ps = PlayerState::default();
        new_ps.viewangles = Vec3f::new(10.0, 45.0, 0.0);
        new_ps.fov = 90.0;
        new_ps.pmove.origin = [100, 200, 300];
        new_ps.pmove.gravity = -800;
        new_ps.stats[STAT_HEALTH] = 100;

        let mut msg = NetMsg::new();
        msg.write_player_state(&old_ps, &new_ps);
        msg.begin_reading();
        // read_player_state expects the svc_playerinfo byte consumed already
        let _ = msg.read_byte(); // consume PlayerInfo opcode
        let decoded = read_player_state(&mut msg, None);

        assert_eq!(decoded.pmove.origin, new_ps.pmove.origin);
        assert_eq!(decoded.pmove.gravity, new_ps.pmove.gravity);
        assert_eq!(decoded.stats[STAT_HEALTH], 100);
        assert_eq!(decoded.fov, 90.0);
        // Viewangles are angle16-compressed (lossy); check within quantization error.
        assert!((decoded.viewangles.x - new_ps.viewangles.x).abs() < 0.1);
        assert!((decoded.viewangles.y - new_ps.viewangles.y).abs() < 0.1);
    }

    #[test]
    fn player_state_delta_stat_bit31_no_overflow() {
        let mut old_ps = PlayerState::default();
        let mut new_ps = PlayerState::default();
        // Change stat at slot 31 to exercise the u32 << 31 path.
        old_ps.stats[31] = 0;
        new_ps.stats[31] = 42;

        let mut msg = NetMsg::new();
        msg.write_player_state(&old_ps, &new_ps);
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PlayerInfo opcode
        let decoded = read_player_state(&mut msg, None);
        assert_eq!(decoded.stats[31], 42);
    }

    #[test]
    fn entity_delta_roundtrip() {
        let old_ent = EntityState {
            number: 3,
            origin: Vec3f::new(100.0, 200.0, 300.0),
            modelindex: 5,
            ..Default::default()
        };
        let new_ent = EntityState {
            number: 3,
            origin: Vec3f::new(110.0, 200.0, 300.0),
            modelindex: 7,
            frame: 2,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_delta_entity(&old_ent, &new_ent, true, false);
        msg.begin_reading();
        let (num, bits) = parse_entity_bits(&mut msg);
        let decoded = read_delta_entity(&mut msg, &old_ent, num, bits);

        assert_eq!(decoded.number, 3);
        assert!((decoded.origin.x - 110.0).abs() < 0.5); // coord quantization
        assert_eq!(decoded.origin.y, 200.0);
        assert_eq!(decoded.modelindex, 7);
        assert_eq!(decoded.frame, 2);
    }

    #[test]
    fn packet_entities_roundtrip() {
        let ent1 = EntityState {
            number: 1,
            origin: Vec3f::new(10.0, 20.0, 30.0),
            modelindex: 2,
            ..Default::default()
        };
        let ent2 = EntityState {
            number: 5,
            origin: Vec3f::new(50.0, 60.0, 70.0),
            modelindex: 3,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_packet_entities_list(&[], &[ent1.clone(), ent2.clone()]);
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PacketEntities opcode
        let decoded = read_packet_entities(&mut msg, &[], &vec![CEntity::default(); MAX_EDICTS]);

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].number, 1);
        assert_eq!(decoded[1].number, 5);
        assert_eq!(decoded[0].modelindex, 2);
        assert_eq!(decoded[1].modelindex, 3);
    }

    #[test]
    fn packet_entities_remove_roundtrip() {
        let ent1 = EntityState {
            number: 1,
            modelindex: 2,
            ..Default::default()
        };
        let ent2 = EntityState {
            number: 3,
            modelindex: 4,
            ..Default::default()
        };
        let ent3 = EntityState {
            number: 5,
            modelindex: 6,
            ..Default::default()
        };

        // Old frame has 1, 3, 5. New frame has only 1 and 5 (3 removed).
        let old = vec![ent1.clone(), ent2.clone(), ent3.clone()];
        let new = vec![ent1.clone(), ent3.clone()];

        let mut msg = NetMsg::new();
        msg.write_packet_entities_list(&old, &new);
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PacketEntities opcode
        let decoded = read_packet_entities(&mut msg, &old, &vec![CEntity::default(); MAX_EDICTS]);

        // Entity 3 must be absent from result.
        assert_eq!(decoded.len(), 2);
        assert!(decoded.iter().all(|e| e.number != 3));
        assert!(decoded.iter().any(|e| e.number == 1));
        assert!(decoded.iter().any(|e| e.number == 5));
    }

    #[test]
    fn delta_frame_uses_old_playerstate() {
        let mut cs = ClientState::default();

        // Frame 1: uncompressed, set fov=75.
        let mut ps1 = PlayerState::default();
        ps1.fov = 75.0;
        let mut msg = NetMsg::new();
        write_empty_frame(&mut msg, 1, -1, Some(&ps1), &[]);
        msg.begin_reading();
        cs.parse_server_message(&mut msg);
        assert_eq!(cs.frame.playerstate.fov, 75.0);

        // Frame 2: delta from frame 1, only gravity changed.
        let mut ps2 = ps1.clone();
        ps2.pmove.gravity = -500;
        let old_ps = cs.frame.playerstate.clone();
        let mut msg2 = NetMsg::new();
        msg2.write_byte(SvcOp::Frame as i32);
        msg2.write_long(2);
        msg2.write_long(1); // delta from frame 1
        msg2.write_byte(0);
        msg2.write_byte(0); // areabits len = 0
        msg2.write_player_state(&old_ps, &ps2);
        msg2.write_packet_entities_list(&[], &[]);
        msg2.begin_reading();
        cs.parse_server_message(&mut msg2);

        // fov should be carried forward (delta); gravity should update.
        assert_eq!(cs.frame.playerstate.fov, 75.0);
        assert_eq!(cs.frame.playerstate.pmove.gravity, -500);
    }

    #[test]
    fn player_info_as_standalone_opcode_is_rejected() {
        let mut cs = ClientState::default();
        cs.state = ConnState::Active;

        // Inject PlayerInfo (17) as top-level opcode — protocol violation.
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::PlayerInfo as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        // Parser must have stopped (not panicked, not consumed further data).
        assert_eq!(cs.state, ConnState::Active); // unchanged
    }

    #[test]
    fn packet_entities_as_standalone_opcode_is_rejected() {
        let mut cs = ClientState::default();
        cs.state = ConnState::Active;

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::PacketEntities as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.state, ConnState::Active);
    }
}
