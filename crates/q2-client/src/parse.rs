use crate::state::{CEntity, ClientFrame, ClientState, ConnState};
use q2_common::error::Q2Result;
use q2_common::net_msg::NetMsg;
use q2_common::q2_drop;
use q2_shared::constants::*;
use q2_shared::protocol::*;
use q2_shared::types::*;

// svc_sound flags (common.h SND_*).
const SND_VOLUME: i32 = 1 << 0;
const SND_ATTENUATION: i32 = 1 << 1;
const SND_POS: i32 = 1 << 2;
const SND_ENT: i32 = 1 << 3;
const SND_OFFSET: i32 = 1 << 4;

impl ClientState {
    /// Parse a complete server message buffer (may contain multiple opcodes).
    ///
    /// Returns `Q2Error::Drop` where C `CL_ParseServerMessage` would raise
    /// ERR_DROP: an illegible opcode, out-of-place frame data, a malformed
    /// frame, or reading past the end of the message. Parsing stops at the
    /// first error, since the rest of the stream can no longer be trusted.
    pub fn parse_server_message(&mut self, msg: &mut NetMsg) -> Q2Result<()> {
        loop {
            if msg.is_overflowed() {
                q2_drop!("parse_server_message: bad server message (read past end)");
            }
            if msg.remaining_data().is_empty() {
                return Ok(());
            }
            let cmd = msg.read_byte();
            let op = SvcOp::try_from(cmd as u8);

            match op {
                Ok(SvcOp::ServerData) => self.parse_server_data(msg)?,
                Ok(SvcOp::ConfigString) => self.parse_config_string(msg),
                Ok(SvcOp::SpawnBaseline) => self.parse_baseline(msg),
                Ok(SvcOp::Print) => self.parse_print(msg),
                Ok(SvcOp::StuffText) => self.parse_stufftext(msg),
                Ok(SvcOp::CenterPrint) => self.parse_centerprint(msg),
                Ok(SvcOp::Frame) => self.parse_frame(msg)?,
                Ok(SvcOp::Disconnect) => {
                    self.state = ConnState::Disconnected;
                    return Ok(());
                }
                Ok(SvcOp::Nop) => {}
                Ok(SvcOp::Reconnect) => self.state = ConnState::Connecting,
                // No client handler yet: consume the payload per its C layout
                // so the opcodes after it (often the frame) still parse.
                Ok(SvcOp::MuzzleFlash) | Ok(SvcOp::MuzzleFlash2) => {
                    let _entity = msg.read_short();
                    let _flash = msg.read_byte();
                }
                Ok(SvcOp::Layout) => {
                    let _layout = msg.read_string();
                }
                Ok(SvcOp::Inventory) => {
                    for _ in 0..MAX_ITEMS {
                        msg.read_short();
                    }
                }
                Ok(SvcOp::Sound) => skip_sound(msg),
                Ok(SvcOp::Download) => skip_download(msg),
                // The payload depends on a per-type table not ported yet
                // (opcode handlers, issue #19). Temp entities trail the frame
                // in the datagram, so stopping here does not lose the frame.
                Ok(SvcOp::TempEntity) => {
                    q2_drop!("parse_server_message: svc_temp_entity is not supported yet");
                }
                // PlayerInfo (17) and PacketEntities (18) are sub-commands of
                // svc_frame and MUST NOT appear as top-level opcodes.
                Ok(SvcOp::PlayerInfo)
                | Ok(SvcOp::PacketEntities)
                | Ok(SvcOp::DeltaPacketEntities) => {
                    q2_drop!("parse_server_message: out of place frame data (opcode {cmd})");
                }
                _ => {
                    q2_drop!("parse_server_message: illegible server message (opcode {cmd})");
                }
            }
        }
    }

    /// Mirrors C `CL_ParseServerData`. The protocol and player number are
    /// validated before any state is cleared, so a rejected message leaves
    /// the client untouched (C clears first, then drops).
    fn parse_server_data(&mut self, msg: &mut NetMsg) -> Q2Result<()> {
        let protocol = msg.read_long();
        if protocol != PROTOCOL_VERSION {
            q2_drop!("Server returned version {protocol}, not {PROTOCOL_VERSION}");
        }
        let server_count = msg.read_long();
        let attractloop = msg.read_byte();
        let gamedir = msg.read_string();
        let playernum = msg.read_short();
        let _levelname = msg.read_string();
        if msg.is_overflowed() {
            q2_drop!("parse_server_data: truncated message");
        }
        // -1 means a cinematic or picture rather than a level.
        if !(-1..MAX_CLIENTS as i32).contains(&playernum) {
            q2_drop!("parse_server_data: bad player number {playernum}");
        }

        tracing::info!("Server data: protocol={}, player={}", protocol, playernum);

        // CL_ClearState: frames from the previous server must not be used
        // as delta bases.
        self.frames = (0..UPDATE_BACKUP).map(|_| ClientFrame::default()).collect();
        self.frame = ClientFrame::default();
        self.server_count = server_count;
        self.attractloop = attractloop != 0;
        self.gamedir = gamedir;
        self.playernum = playernum;

        self.entities.clear();
        self.entities.resize(MAX_EDICTS, CEntity::default());
        self.configstrings.clear();
        self.configstrings.resize(MAX_CONFIGSTRINGS, String::new());
        Ok(())
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
    ///
    /// A malformed or truncated frame is an error and is not stored, so it
    /// can never become a delta base.
    fn parse_frame(&mut self, msg: &mut NetMsg) -> Q2Result<()> {
        let serverframe = msg.read_long();
        let deltaframe = msg.read_long();
        let _suppress = msg.read_byte();
        let raw_areabits_len = msg.read_byte();
        if msg.is_overflowed() {
            q2_drop!("parse_frame: truncated frame header");
        }
        if serverframe < 0 {
            q2_drop!("parse_frame: negative serverframe {serverframe}");
        }
        // Wraps like the C int multiply instead of panicking.
        let servertime = serverframe.wrapping_mul(100);

        // areabits: length-prefixed raw bytes.  Cap to MAX_MAP_AREAS/8 but
        // consume the full wire length to keep the stream aligned. The
        // length is a byte (0..=255) now that truncation was rejected.
        let raw_areabits_len = raw_areabits_len as usize;
        let capped_areabits_len = raw_areabits_len.min(MAX_MAP_AREAS / 8);
        if raw_areabits_len > MAX_MAP_AREAS / 8 {
            tracing::warn!(
                "parse_frame: areabits_len {} > max {}, capping",
                raw_areabits_len,
                MAX_MAP_AREAS / 8
            );
        }
        let mut areabits = [0u8; MAX_MAP_AREAS / 8];
        msg.read_data(&mut areabits[..capped_areabits_len]);
        for _ in capped_areabits_len..raw_areabits_len {
            msg.read_byte();
        }

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
            q2_drop!("parse_frame: expected PlayerInfo (17), got {ps_cmd}");
        }
        let old_ps = old_frame.as_ref().map(|f| &f.playerstate);
        let playerstate = read_player_state(msg, old_ps);

        // svc_packetentities must follow immediately.
        let pe_cmd = msg.read_byte();
        if pe_cmd != SvcOp::PacketEntities as i32 {
            q2_drop!("parse_frame: expected PacketEntities (18), got {pe_cmd}");
        }
        let old_entities = old_frame
            .as_ref()
            .map(|f| f.entities.as_slice())
            .unwrap_or(&[]);
        let entities = read_packet_entities(msg, old_entities, &self.entities)?;
        if msg.is_overflowed() {
            q2_drop!("parse_frame: frame truncated");
        }

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
        Ok(())
    }
}

/// Consume an svc_sound payload (C `CL_ParseStartSoundPacket` layout).
fn skip_sound(msg: &mut NetMsg) {
    let flags = msg.read_byte();
    let _sound_index = msg.read_byte();
    for optional_byte in [SND_VOLUME, SND_ATTENUATION, SND_OFFSET] {
        if flags & optional_byte != 0 {
            msg.read_byte();
        }
    }
    if flags & SND_ENT != 0 {
        let _entity_channel = msg.read_short();
    }
    if flags & SND_POS != 0 {
        let mut _pos = Vec3f::ZERO;
        msg.read_position(&mut _pos);
    }
}

/// Consume an svc_download payload (C `CL_ParseDownload` layout): size,
/// percent, then `size` data bytes unless size is -1 (file not found).
fn skip_download(msg: &mut NetMsg) {
    let size = msg.read_short();
    let _percent = msg.read_byte();
    if let Ok(size) = usize::try_from(size) {
        let mut data = vec![0u8; size];
        msg.read_data(&mut data);
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
/// Mirrors C `CL_ParseDelta` (protocol 34). With empty `bits` nothing is
/// read: the result is `from` with `event` cleared and `old_origin` set to
/// the previous origin, which is how C carries unchanged entities forward.
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

    // FRAME8 and FRAME16 are independent fields, read in that order.
    if bits.contains(UpdateFlags::FRAME8) {
        e.frame = msg.read_byte();
    }
    if bits.contains(UpdateFlags::FRAME16) {
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

    if bits.contains(UpdateFlags::ANGLE1) {
        e.angles.x = msg.read_angle();
    }
    if bits.contains(UpdateFlags::ANGLE2) {
        e.angles.y = msg.read_angle();
    }
    if bits.contains(UpdateFlags::ANGLE3) {
        e.angles.z = msg.read_angle();
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
/// `ClientFrame::entities`. Errors (C ERR_DROP) on a truncated list or a bad
/// entity number; numbers must also ascend, which the merge with
/// `old_entities` relies on.
fn read_packet_entities(
    msg: &mut NetMsg,
    old_entities: &[EntityState],
    baselines: &[CEntity],
) -> Q2Result<Vec<EntityState>> {
    let mut result: Vec<EntityState> = Vec::new();
    let mut old_idx = 0usize;
    let mut last_num = 0;

    loop {
        let (newnum, bits) = parse_entity_bits(msg);

        if msg.is_overflowed() {
            q2_drop!("read_packet_entities: end of message");
        }
        // C reads the number as unsigned, so a negative NUMBER16 also fails
        // this check there.
        if !(0..MAX_EDICTS as i32).contains(&newnum) {
            q2_drop!("read_packet_entities: bad number {newnum}");
        }

        if newnum == 0 {
            break; // terminator
        }
        if newnum <= last_num {
            q2_drop!("read_packet_entities: entity {newnum} after {last_num}");
        }
        last_num = newnum;

        // Copy unchanged old entities whose number is less than this new one.
        while old_idx < old_entities.len() && old_entities[old_idx].number < newnum {
            result.push(copy_forward(&old_entities[old_idx]));
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
    result.extend(old_entities[old_idx..].iter().map(copy_forward));

    Ok(result)
}

/// An entity absent from the packet is unchanged: C runs it through
/// `CL_ParseDelta` with no bits, so its event is cleared and its old_origin
/// becomes the previous origin.
fn copy_forward(old: &EntityState) -> EntityState {
    EntityState {
        old_origin: old.origin,
        event: 0,
        ..old.clone()
    }
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
        msg.write_packet_entities_list(&[], entities, &[], 0)
            .expect("valid entities");
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
        cs.parse_server_message(&mut msg).expect("valid message");

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
        cs.parse_server_message(&mut msg).expect("valid message");

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

        cs.parse_server_message(&mut msg).expect("valid message");
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

        cs.parse_server_message(&mut msg).expect("valid message");
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

        cs.parse_server_message(&mut msg).expect("valid message");
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_frame_sets_serverframe() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(42, -1);
        cs.parse_server_message(&mut msg).expect("valid message");
        assert_eq!(cs.frame.serverframe, 42);
        assert_eq!(cs.frame.deltaframe, -1);
    }

    #[test]
    fn parse_disconnect_sets_state() {
        let mut cs = ClientState {
            state: ConnState::Active,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg).expect("valid message");
        assert_eq!(cs.state, ConnState::Disconnected);
    }

    #[test]
    fn parse_nop_is_harmless() {
        let mut cs = ClientState {
            state: ConnState::Active,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Nop as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg).expect("valid message");
        assert_eq!(cs.state, ConnState::Active);
    }

    // C CL_ParseServerMessage: an unknown opcode is ERR_DROP ("Illegible
    // server message").
    #[test]
    fn unknown_opcode_is_error_and_stops_parsing() {
        let mut cs = ClientState {
            state: ConnState::Active,
            ..Default::default()
        };
        let mut msg = NetMsg::new();
        msg.write_byte(255);
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.state, ConnState::Active);
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
        cs.parse_server_message(&mut msg).expect("valid message");

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

        cs.parse_server_message(&mut msg).expect("valid message");
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_centerprint_consumes_string() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::CenterPrint as i32);
        msg.write_string("You found a secret!");
        msg.begin_reading();

        cs.parse_server_message(&mut msg).expect("valid message");
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

        cs.parse_server_message(&mut msg).expect("valid message");
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

        cs.parse_server_message(&mut msg).expect("valid message");
        assert!(cs.configstrings.iter().all(|s| s.is_empty()));
    }

    // -------------------------------------------------------------------------
    // New round-trip tests for delta machinery
    // -------------------------------------------------------------------------

    #[test]
    fn parse_frame_stores_in_ring_buffer() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(5, -1);
        cs.parse_server_message(&mut msg).expect("valid message");

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
        msg.write_packet_entities_list(&[], &[], &[], 0)
            .expect("valid entities");
        msg.begin_reading();

        cs.parse_server_message(&mut msg).expect("valid message");
        assert_eq!(cs.frame.areabits[0], 0xAB);
        assert_eq!(cs.frame.areabits[1], 0xCD);
        assert_eq!(cs.frame.areabits[2], 0xEF);
        assert_eq!(cs.frame.areabits[3], 0x12);
        assert_eq!(cs.frame.areabits[4], 0x00);
    }

    #[test]
    fn player_state_delta_roundtrip() {
        let old_ps = PlayerState::default();
        let mut new_ps = PlayerState {
            viewangles: Vec3f::new(10.0, 45.0, 0.0),
            fov: 90.0,
            ..Default::default()
        };
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
        msg.write_delta_entity(&old_ent, &new_ent, true, false)
            .expect("valid entity");
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
        msg.write_packet_entities_list(&[], &[ent1.clone(), ent2.clone()], &[], 0)
            .expect("valid entities");
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PacketEntities opcode
        let decoded = read_packet_entities(&mut msg, &[], &vec![CEntity::default(); MAX_EDICTS])
            .expect("valid entity list");

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
        msg.write_packet_entities_list(&old, &new, &[], 0)
            .expect("valid entities");
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PacketEntities opcode
        let decoded = read_packet_entities(&mut msg, &old, &vec![CEntity::default(); MAX_EDICTS])
            .expect("valid entity list");

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
        let ps1 = PlayerState {
            fov: 75.0,
            ..Default::default()
        };
        let mut msg = NetMsg::new();
        write_empty_frame(&mut msg, 1, -1, Some(&ps1), &[]);
        msg.begin_reading();
        cs.parse_server_message(&mut msg).expect("valid message");
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
        msg2.write_packet_entities_list(&[], &[], &[], 0)
            .expect("valid entities");
        msg2.begin_reading();
        cs.parse_server_message(&mut msg2).expect("valid message");

        // fov should be carried forward (delta); gravity should update.
        assert_eq!(cs.frame.playerstate.fov, 75.0);
        assert_eq!(cs.frame.playerstate.pmove.gravity, -500);
    }

    #[test]
    fn player_info_as_standalone_opcode_is_rejected() {
        let mut cs = ClientState {
            state: ConnState::Active,
            ..Default::default()
        };

        // Inject PlayerInfo (17) as top-level opcode followed by Disconnect.
        // If the parser correctly aborts on the violation it must NOT reach
        // the Disconnect opcode — state stays Active.
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::PlayerInfo as i32);
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.state, ConnState::Active);
    }

    #[test]
    fn packet_entities_as_standalone_opcode_is_rejected() {
        let mut cs = ClientState {
            state: ConnState::Active,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::PacketEntities as i32);
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.state, ConnState::Active);
    }

    // I3: entity number >= 256 exercises the NUMBER16 path (MOREBITS1 + read_short).
    #[test]
    fn entity_number16_roundtrip() {
        let from = EntityState::default();
        let to = EntityState {
            number: 300,
            modelindex: 9,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_delta_entity(&from, &to, true, false)
            .expect("valid entity");
        msg.begin_reading();
        let (num, bits) = parse_entity_bits(&mut msg);

        assert_eq!(num, 300);
        assert!(bits.contains(UpdateFlags::NUMBER16));
        let decoded = read_delta_entity(&mut msg, &from, num, bits);
        assert_eq!(decoded.number, 300);
        assert_eq!(decoded.modelindex, 9);
    }

    // I4: WEAPONFRAME group encodes gunframe + 6 packed gunoffset/gunangle bytes.
    #[test]
    fn player_state_weaponframe_roundtrip() {
        let old_ps = PlayerState::default();
        let new_ps = PlayerState {
            gunframe: 7,
            gunoffset: Vec3f::new(2.0, -1.0, 0.5),
            gunangles: Vec3f::new(15.0, -10.0, 5.0),
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_player_state(&old_ps, &new_ps);
        msg.begin_reading();
        let _ = msg.read_byte(); // consume PlayerInfo opcode
        let decoded = read_player_state(&mut msg, None);

        assert_eq!(decoded.gunframe, 7);
        // gunoffset/gunangles stored as (val * 4) as i8 → 0.25 resolution
        assert!((decoded.gunoffset.x - 2.0).abs() < 0.26);
        assert!((decoded.gunoffset.y - (-1.0)).abs() < 0.26);
        assert!((decoded.gunangles.x - 15.0).abs() < 0.26);
        assert!((decoded.gunangles.y - (-10.0)).abs() < 0.26);
    }

    // S1: SOLID flag lives in bits 24-31, forcing the MOREBITS3 expansion (4-byte header).
    #[test]
    fn entity_morebits3_solid_roundtrip() {
        let from = EntityState::default();
        let to = EntityState {
            number: 1,
            solid: 31744,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_delta_entity(&from, &to, true, false)
            .expect("valid entity");
        msg.begin_reading();
        let (_, bits) = parse_entity_bits(&mut msg);

        assert!(bits.contains(UpdateFlags::SOLID));
        let decoded = read_delta_entity(&mut msg, &from, 1, bits);
        assert_eq!(decoded.solid, 31744);
    }

    // 256..0x8000 skinnum/effects use the 16-bit form (SKIN16 / EFFECTS16
    // alone, short), as C MSG_WriteDeltaEntity does.
    #[test]
    fn entity_skin_effects_16bit_roundtrip() {
        let from = EntityState::default();
        let to = EntityState {
            number: 2,
            skinnum: 300,
            effects: 300,
            ..Default::default()
        };

        let mut msg = NetMsg::new();
        msg.write_delta_entity(&from, &to, true, false)
            .expect("valid entity");
        msg.begin_reading();
        let (num, bits) = parse_entity_bits(&mut msg);

        assert!(!bits.contains(UpdateFlags::SKIN8));
        assert!(bits.contains(UpdateFlags::SKIN16));
        assert!(!bits.contains(UpdateFlags::EFFECTS8));
        assert!(bits.contains(UpdateFlags::EFFECTS16));
        let decoded = read_delta_entity(&mut msg, &from, num, bits);
        assert_eq!(decoded.skinnum, 300);
        assert_eq!(decoded.effects, 300);
        assert!(msg.remaining_data().is_empty());
    }

    // Laser colour skins are negative as i32 and need the 32-bit form.
    #[test]
    fn entity_negative_skinnum_roundtrips() {
        let from = EntityState::default();
        let to = EntityState {
            number: 2,
            skinnum: 0xd0d1_d2d3_u32 as i32,
            ..Default::default()
        };
        let mut msg = NetMsg::new();
        msg.write_delta_entity(&from, &to, true, false)
            .expect("valid entity");
        msg.begin_reading();
        let (num, bits) = parse_entity_bits(&mut msg);
        let decoded = read_delta_entity(&mut msg, &from, num, bits);
        assert_eq!(decoded.skinnum, to.skinnum);
    }

    // C writer bytes for {number 1, frame 300, angles (10,20,30)} (see the
    // golden test in net_msg.rs). C CL_ParseDelta reads angles with
    // MSG_ReadAngle: 7, 14, 21 * 1.40625.
    #[test]
    fn read_delta_entity_decodes_c_frame16_and_byte_angles() {
        let mut msg = NetMsg::from_bytes(&[0x8C, 0x84, 0x02, 1, 0x2C, 0x01, 7, 14, 21]);
        let (num, bits) = parse_entity_bits(&mut msg);
        let decoded = read_delta_entity(&mut msg, &EntityState::default(), num, bits);
        assert_eq!(decoded.number, 1);
        assert_eq!(decoded.frame, 300);
        assert_eq!(decoded.angles, Vec3f::new(9.84375, 19.6875, 29.53125));
        assert!(msg.remaining_data().is_empty());
    }

    // C reads U_FRAME8 (byte) and U_FRAME16 (short) independently; with both
    // set it consumes 3 bytes and the short wins.
    // bits = FRAME8|FRAME16|MOREBITS2|MOREBITS1 -> bytes 0x90, 0x80, 0x02.
    #[test]
    fn read_delta_entity_reads_frame8_then_frame16() {
        let mut msg = NetMsg::from_bytes(&[0x90, 0x80, 0x02, 1, 5, 0x2C, 0x01]);
        let (num, bits) = parse_entity_bits(&mut msg);
        let decoded = read_delta_entity(&mut msg, &EntityState::default(), num, bits);
        assert_eq!(decoded.frame, 300);
        assert!(msg.remaining_data().is_empty());
    }

    // C SV_EmitPacketEntities deltas a new entity from its baseline and the
    // client decodes it from the same baseline. A field that is zero in the
    // entity but nonzero in the baseline must still arrive as zero.
    #[test]
    fn new_entity_roundtrips_against_nonzero_baseline() {
        let baseline = EntityState {
            number: 7,
            modelindex: 9,
            solid: 31,
            ..Default::default()
        };
        let mut server_baselines = vec![EntityState::default(); MAX_EDICTS];
        server_baselines[7] = baseline.clone();
        let mut client_baselines = vec![CEntity::default(); MAX_EDICTS];
        client_baselines[7].baseline = baseline;

        let ent = EntityState {
            number: 7,
            modelindex: 0,
            solid: 31,
            ..Default::default()
        };
        let mut msg = NetMsg::new();
        msg.write_packet_entities_list(&[], std::slice::from_ref(&ent), &server_baselines, 0)
            .expect("valid entities");
        msg.begin_reading();
        let _ = msg.read_byte(); // PacketEntities opcode
        let decoded =
            read_packet_entities(&mut msg, &[], &client_baselines).expect("valid entity list");
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].modelindex, 0);
        assert_eq!(decoded[0].solid, 31);
    }

    // C copies unchanged entities forward through CL_ParseDelta with bits 0,
    // which clears `event` and sets old_origin to the previous origin.
    #[test]
    fn copied_forward_entity_clears_event_and_sets_old_origin() {
        let with_event = EntityState {
            number: 4,
            origin: Vec3f::new(8.0, 0.0, 0.0),
            event: 3,
            ..Default::default()
        };
        let baselines = vec![CEntity::default(); MAX_EDICTS];

        let mut msg = NetMsg::new();
        msg.write_packet_entities_list(&[], std::slice::from_ref(&with_event), &[], 0)
            .expect("valid entities");
        msg.begin_reading();
        let _ = msg.read_byte(); // PacketEntities opcode
        let frame1 = read_packet_entities(&mut msg, &[], &baselines).expect("valid entity list");
        assert_eq!(frame1[0].event, 3);
        assert_eq!(frame1[0].old_origin, Vec3f::ZERO);

        // Same entity, event gone: nothing changed, so nothing is written
        // for it and the client copies it forward.
        let without_event = EntityState {
            event: 0,
            ..with_event.clone()
        };
        let mut msg2 = NetMsg::new();
        msg2.write_packet_entities_list(&[with_event], &[without_event], &[], 0)
            .expect("valid entities");
        msg2.begin_reading();
        let _ = msg2.read_byte();
        let frame2 =
            read_packet_entities(&mut msg2, &frame1, &baselines).expect("valid entity list");
        assert_eq!(frame2.len(), 1);
        assert_eq!(frame2[0].event, 0);
        assert_eq!(frame2[0].old_origin, Vec3f::new(8.0, 0.0, 0.0));
    }

    // -------------------------------------------------------------------------
    // Malformed / truncated input (review S-1, S-9, S-10, S-11, S-13, S-31)
    // -------------------------------------------------------------------------

    /// svc_frame header: serverframe, deltaframe, suppress, areabits len 0.
    fn write_frame_header(msg: &mut NetMsg, serverframe: i32, deltaframe: i32) {
        msg.write_byte(SvcOp::Frame as i32);
        msg.write_long(serverframe);
        msg.write_long(deltaframe);
        msg.write_byte(0); // suppress
        msg.write_byte(0); // areabits len
    }

    /// Frame header + empty playerinfo + PacketEntities opcode; the caller
    /// appends the entity list.
    fn frame_up_to_entities(serverframe: i32) -> NetMsg {
        let mut msg = NetMsg::new();
        write_frame_header(&mut msg, serverframe, -1);
        msg.write_player_state(&PlayerState::default(), &PlayerState::default());
        msg.write_byte(SvcOp::PacketEntities as i32);
        msg
    }

    fn assert_frame_not_stored(cs: &ClientState, serverframe: i32) {
        let idx = (serverframe as usize) & UPDATE_MASK;
        assert!(!cs.frames[idx].valid, "frame {serverframe} stored");
        assert_ne!(cs.frame.serverframe, serverframe);
    }

    // S-1: a packet ending after `suppress` made read_byte return -1 for the
    // areabits length, and `-1 as usize` drove a ~2^64-iteration drain loop.
    #[test]
    fn truncated_frame_header_returns_promptly() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut cs = ClientState::default();
            let mut msg = NetMsg::new();
            msg.write_byte(SvcOp::Frame as i32);
            msg.write_long(5);
            msg.write_long(-1);
            msg.write_byte(0); // suppress, then nothing
            msg.begin_reading();
            let parsed = cs.parse_server_message(&mut msg);
            let _ = tx.send((parsed.is_err(), cs.frame.serverframe));
        });
        let (is_err, serverframe) = rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("parse must not hang on a truncated frame");
        assert!(is_err);
        assert_eq!(serverframe, 0);
    }

    // S-10: entity 4 announces ORIGIN1 but the coordinate is missing.
    #[test]
    fn truncated_entity_list_does_not_store_frame() {
        let mut cs = ClientState::default();
        let mut msg = frame_up_to_entities(9);
        msg.write_byte(0x01); // ORIGIN1
        msg.write_byte(4); // entity 4, payload missing
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_frame_not_stored(&cs, 9);
    }

    // S-10: cut a valid multi-opcode message at every offset inside the
    // frame; no prefix may panic or store the frame.
    #[test]
    fn frame_truncated_at_any_offset_is_never_stored() {
        let ents = [
            EntityState {
                number: 3,
                origin: Vec3f::new(8.0, 16.0, 24.0),
                modelindex: 2,
                ..Default::default()
            },
            EntityState {
                number: 300,
                frame: 5,
                ..Default::default()
            },
        ];
        let ps = PlayerState {
            fov: 90.0,
            ..Default::default()
        };
        let mut full = NetMsg::new();
        full.write_byte(SvcOp::ConfigString as i32);
        full.write_short(1);
        full.write_string("x");
        let frame_start = full.len();
        write_empty_frame(&mut full, 9, -1, Some(&ps), &ents);
        let frame_end = full.len();

        // cut == frame_start is a complete message without the frame.
        for cut in frame_start + 1..frame_end {
            let mut cs = ClientState::default();
            let mut msg = NetMsg::from_bytes(&full.data()[..cut]);
            assert!(cs.parse_server_message(&mut msg).is_err(), "cut {cut}");
            assert_frame_not_stored(&cs, 9);
        }

        let mut cs = ClientState::default();
        let mut msg = NetMsg::from_bytes(full.data());
        cs.parse_server_message(&mut msg).expect("full message");
        assert_eq!(cs.frame.serverframe, 9);
        assert_eq!(cs.frame.entities.len(), 2);
    }

    // S-9: NUMBER16 with a short >= 0x8000 reads negative. C reads the
    // number as unsigned, so it fails the MAX_EDICTS check (ERR_DROP).
    // bits = NUMBER16|MOREBITS1 -> 0x80, 0x01; number -5 -> 0xFB, 0xFF.
    #[test]
    fn negative_entity_number_rejects_frame() {
        let mut cs = ClientState::default();
        let mut msg = frame_up_to_entities(9);
        msg.write_data(&[0x80, 0x01, 0xFB, 0xFF]);
        msg.write_data(&[0, 0]);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_frame_not_stored(&cs, 9);
    }

    #[test]
    fn entity_number_max_edicts_rejects_frame() {
        let mut cs = ClientState::default();
        let mut msg = frame_up_to_entities(9);
        msg.write_data(&[0x80, 0x01]);
        msg.write_short(MAX_EDICTS as i32);
        msg.write_data(&[0, 0]);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_frame_not_stored(&cs, 9);
    }

    // S-9: the merge with the old frame relies on ascending numbers.
    #[test]
    fn descending_entity_numbers_reject_frame() {
        let mut cs = ClientState::default();
        let mut msg = frame_up_to_entities(9);
        msg.write_data(&[0x00, 5]); // entity 5, no fields
        msg.write_data(&[0x00, 3]); // entity 3 after 5
        msg.write_data(&[0, 0]);
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_frame_not_stored(&cs, 9);
    }

    // S-11: after a bad playerinfo opcode the rest of the frame is garbage;
    // a following byte 12 must not be run as svc_serverdata.
    #[test]
    fn aborted_frame_stops_parsing_the_message() {
        let mut cs = ClientState::default();
        cs.entities[3].baseline.modelindex = 7;

        let mut msg = NetMsg::new();
        write_frame_header(&mut msg, 1, -1);
        msg.write_byte(99); // not svc_playerinfo
        msg.write_byte(SvcOp::ServerData as i32);
        msg.write_long(PROTOCOL_VERSION);
        msg.write_long(1);
        msg.write_byte(0);
        msg.write_string("baseq2");
        msg.write_short(0);
        msg.write_string("base1");
        msg.begin_reading();

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.entities[3].baseline.modelindex, 7);
    }

    // R-25 / S-13: C CL_ParseServerData drops on a protocol mismatch.
    #[test]
    fn server_data_with_wrong_protocol_is_rejected() {
        let mut cs = ClientState {
            server_count: 3,
            ..Default::default()
        };
        cs.entities[3].baseline.modelindex = 7;
        let mut msg = make_server_data_msg(33, 9, 0, "baseq2", "base1");

        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.server_count, 3);
        assert_eq!(cs.entities[3].baseline.modelindex, 7);
    }

    #[test]
    fn server_data_with_out_of_range_playernum_is_rejected() {
        let mut cs = ClientState::default();
        let mut msg = make_server_data_msg(34, 9, 9999, "baseq2", "base1");
        assert!(cs.parse_server_message(&mut msg).is_err());
    }

    // playernum -1 means a cinematic or picture, not a level.
    #[test]
    fn server_data_accepts_cinematic_playernum() {
        let mut cs = ClientState::default();
        let mut msg = make_server_data_msg(34, 9, -1, "baseq2", "intro.cin");
        cs.parse_server_message(&mut msg)
            .expect("cinematic serverdata");
        assert_eq!(cs.playernum, -1);
    }

    #[test]
    fn server_data_records_attractloop() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ServerData as i32);
        msg.write_long(PROTOCOL_VERSION);
        msg.write_long(1);
        msg.write_byte(1); // attractloop
        msg.write_string("baseq2");
        msg.write_short(0);
        msg.write_string("demo");
        msg.begin_reading();
        cs.parse_server_message(&mut msg).expect("valid serverdata");
        assert!(cs.attractloop);
    }

    // S-13: like CL_ClearState, a new serverdata forgets old frames so a
    // frame from the previous map cannot be a delta base.
    #[test]
    fn server_data_resets_frame_ring() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(5, -1);
        cs.parse_server_message(&mut msg).expect("valid frame");
        assert!(cs.frames[5].valid);

        let mut msg = make_server_data_msg(34, 2, 0, "baseq2", "base2");
        cs.parse_server_message(&mut msg).expect("valid serverdata");
        assert!(cs.frames.iter().all(|f| !f.valid));
        assert_eq!(cs.frame.serverframe, 0);
    }

    // S-31: serverframe * 100 overflowed i32 (debug panic).
    #[test]
    fn huge_serverframe_does_not_panic() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(i32::MAX, -1);
        cs.parse_server_message(&mut msg).expect("valid frame");
        assert_eq!(cs.frame.serverframe, i32::MAX);
        assert_eq!(cs.frame.servertime, i32::MAX.wrapping_mul(100));
    }

    #[test]
    fn negative_serverframe_is_rejected() {
        let mut cs = ClientState::default();
        let mut msg = make_frame_msg(-7, -1);
        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_frame_not_stored(&cs, -7);
    }

    // C CL_ParseServerMessage: reading past the end of the message is
    // ERR_DROP ("Bad server message").
    #[test]
    fn unterminated_configstring_is_error() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::from_bytes(&[SvcOp::ConfigString as u8, 5, 0, b'a']);
        assert!(cs.parse_server_message(&mut msg).is_err());
    }

    // -------------------------------------------------------------------------
    // Opcodes without a client handler are consumed per their C layout
    // (review S-12) so the frame behind them is still parsed.
    // -------------------------------------------------------------------------

    /// Parse `prefix` followed by a valid frame 42; assert the frame landed.
    fn assert_frame_parsed_after(prefix: &NetMsg) -> ClientState {
        let mut msg = NetMsg::new();
        msg.write_data(prefix.data());
        write_empty_frame(&mut msg, 42, -1, None, &[]);
        msg.begin_reading();
        let mut cs = ClientState::default();
        cs.parse_server_message(&mut msg).expect("valid message");
        assert_eq!(cs.frame.serverframe, 42);
        assert!(msg.remaining_data().is_empty());
        cs
    }

    // svc_layout: one string (C copies it to cl.layout).
    #[test]
    fn layout_before_frame_is_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Layout as i32);
        msg.write_string("xv 0 yv 0 string \"hi\"");
        assert_frame_parsed_after(&msg);
    }

    // svc_inventory: MAX_ITEMS (256) shorts (C CL_ParseInventory).
    #[test]
    fn inventory_before_frame_is_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Inventory as i32);
        for i in 0..MAX_ITEMS as i32 {
            msg.write_short(i);
        }
        assert_frame_parsed_after(&msg);
    }

    // svc_muzzleflash / svc_muzzleflash2: short entity, byte weapon/flash.
    #[test]
    fn muzzleflashes_before_frame_are_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::MuzzleFlash as i32);
        msg.write_short(1);
        msg.write_byte(SvcOp::Frame as i32); // weapon byte that looks like an opcode
        msg.write_byte(SvcOp::MuzzleFlash2 as i32);
        msg.write_short(2);
        msg.write_byte(3);
        assert_frame_parsed_after(&msg);
    }

    // svc_sound (C CL_ParseStartSoundPacket): flags, soundnum, then volume,
    // attenuation, offset bytes, entity/channel short and position as
    // flagged. All flags set: 1 + 1 + 3 + 2 + 6 bytes after the opcode.
    #[test]
    fn sound_with_all_fields_before_frame_is_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Sound as i32);
        msg.write_byte(0x1F); // VOLUME|ATTENUATION|POS|ENT|OFFSET
        msg.write_byte(4); // sound index
        msg.write_byte(255); // volume
        msg.write_byte(64); // attenuation
        msg.write_byte(0); // offset
        msg.write_short((5 << 3) | 2); // entity 5, channel 2
        msg.write_position(Vec3f::new(1.0, 2.0, 3.0));
        assert_frame_parsed_after(&msg);
    }

    #[test]
    fn sound_with_no_optional_fields_before_frame_is_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Sound as i32);
        msg.write_byte(0);
        msg.write_byte(4);
        assert_frame_parsed_after(&msg);
    }

    // svc_download: short size, byte percent, then `size` bytes when
    // size > 0 (C CL_ParseDownload); size -1 means "no such file".
    #[test]
    fn download_chunks_before_frame_are_skipped() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Download as i32);
        msg.write_short(3);
        msg.write_byte(50);
        msg.write_data(&[SvcOp::Disconnect as u8, 0xAA, 0xBB]);
        msg.write_byte(SvcOp::Download as i32);
        msg.write_short(-1);
        msg.write_byte(0);
        // A misread would hit the svc_disconnect byte and miss the frame.
        assert_frame_parsed_after(&msg);
    }

    // svc_reconnect: C sets ca_connecting and keeps parsing.
    #[test]
    fn reconnect_sets_connecting_and_keeps_parsing() {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Reconnect as i32);
        let cs = assert_frame_parsed_after(&msg);
        assert_eq!(cs.state, ConnState::Connecting);
    }

    // svc_temp_entity needs the per-type payload table; until that handler
    // exists it is an error rather than a silent stop. Temp entities follow
    // the frame in the datagram, so the frame is not lost.
    #[test]
    fn temp_entity_is_error() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        write_empty_frame(&mut msg, 42, -1, None, &[]);
        msg.write_byte(SvcOp::TempEntity as i32);
        msg.write_byte(0);
        msg.begin_reading();
        assert!(cs.parse_server_message(&mut msg).is_err());
        assert_eq!(cs.frame.serverframe, 42);
    }
}
