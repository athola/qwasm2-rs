use q2_shared::protocol::*;
use q2_shared::constants::*;
use q2_common::net_msg::NetMsg;
use crate::state::{ClientState, ConnState, CEntity};

impl ClientState {
    /// Parse a server message.
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
                Ok(SvcOp::PlayerInfo) => self.parse_playerinfo(msg),
                Ok(SvcOp::PacketEntities) => self.parse_packet_entities(msg),
                Ok(SvcOp::Disconnect) => {
                    self.state = ConnState::Disconnected;
                    return;
                }
                Ok(SvcOp::Nop) => {}
                _ => {
                    tracing::warn!("Unknown server command {} — stopping parse (remaining bytes lost)", cmd);
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

        // Reset state for new level
        self.entities.clear();
        self.entities
            .resize(MAX_EDICTS, CEntity::default());
        self.configstrings.clear();
        self.configstrings
            .resize(MAX_CONFIGSTRINGS, String::new());
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
            tracing::warn!("Configstring: index {} out of range (max {})", index, self.configstrings.len());
        }
    }

    fn parse_baseline(&mut self, msg: &mut NetMsg) {
        // Minimal stub — reads entity number only.
        // A real server sends delta-compressed entity bits after this;
        // connecting to a full server will desync here.
        let _entity_num = msg.read_short();
        tracing::debug!("parse_baseline: stub (entity {})", _entity_num);
    }

    fn parse_print(&mut self, msg: &mut NetMsg) {
        let _level = msg.read_byte();
        let text = msg.read_string();
        tracing::info!("[server] {}", text);
    }

    fn parse_stufftext(&mut self, msg: &mut NetMsg) {
        let _cmd = msg.read_string();
        // TODO: execute stuffed command
    }

    fn parse_centerprint(&mut self, msg: &mut NetMsg) {
        let _text = msg.read_string();
        // TODO: display centered text
    }

    fn parse_frame(&mut self, msg: &mut NetMsg) {
        self.frame.serverframe = msg.read_long();
        self.frame.deltaframe = msg.read_long();
        // Suppress count for bandwidth
        let _suppress = msg.read_byte();
        // TODO: parse areabits
    }

    fn parse_playerinfo(&mut self, msg: &mut NetMsg) {
        // TODO: parse delta-compressed player state
        // WARNING: this stub does not consume message bytes — remaining data will be corrupted
        tracing::warn!("parse_playerinfo: stub — message stream may be corrupted");
        let _ = msg;
    }

    fn parse_packet_entities(&mut self, msg: &mut NetMsg) {
        // TODO: parse delta-compressed entity states
        // WARNING: this stub does not consume message bytes — remaining data will be corrupted
        tracing::warn!("parse_packet_entities: stub — message stream may be corrupted");
        let _ = msg;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ClientState;

    /// Build a NetMsg with ServerData payload and hand it to the parser.
    fn make_server_data_msg(protocol: i32, count: i32, playernum: i16, gamedir: &str, level: &str) -> NetMsg {
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ServerData as i32);
        msg.write_long(protocol);      // protocol version
        msg.write_long(count);          // server count
        msg.write_byte(0);              // attract loop
        msg.write_string(gamedir);      // gamedir
        msg.write_short(playernum as i32); // player number
        msg.write_string(level);        // level name
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
        // Dirty the entities vec
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
        // Index beyond configstrings capacity — should be silently ignored
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
        msg.write_byte(0); // print level
        msg.write_string("Hello world");
        msg.begin_reading();

        // Should not panic and should consume the entire message
        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_frame_sets_serverframe() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::Frame as i32);
        msg.write_long(42);  // serverframe
        msg.write_long(40);  // deltaframe
        msg.write_byte(0);   // suppress count
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert_eq!(cs.frame.serverframe, 42);
        assert_eq!(cs.frame.deltaframe, 40);
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
        msg.write_byte(255); // unknown opcode
        // This data should NOT be parsed
        msg.write_byte(SvcOp::Disconnect as i32);
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        // Disconnect should NOT have been processed
        assert_eq!(cs.state, ConnState::Disconnected); // default
        // But the message was not fully consumed
    }

    #[test]
    fn parse_multiple_messages_in_sequence() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();

        // Nop + ConfigString + Frame
        msg.write_byte(SvcOp::Nop as i32);

        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(10);
        msg.write_string("test_value");

        msg.write_byte(SvcOp::Frame as i32);
        msg.write_long(100);
        msg.write_long(98);
        msg.write_byte(0);

        msg.begin_reading();
        cs.parse_server_message(&mut msg);

        assert_eq!(cs.configstrings[10], "test_value");
        assert_eq!(cs.frame.serverframe, 100);
        assert_eq!(cs.frame.deltaframe, 98);
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
        msg.write_short(42); // entity number
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        assert!(msg.remaining_data().is_empty());
    }

    #[test]
    fn parse_config_string_negative_index_ignored() {
        let mut cs = ClientState::default();
        let mut msg = NetMsg::new();
        msg.write_byte(SvcOp::ConfigString as i32);
        msg.write_short(-1); // negative index
        msg.write_string("should_be_ignored");
        msg.begin_reading();

        cs.parse_server_message(&mut msg);
        // No configstring should be modified
        assert!(cs.configstrings.iter().all(|s| s.is_empty()));
    }
}
