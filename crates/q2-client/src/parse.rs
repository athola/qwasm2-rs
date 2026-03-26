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
                    tracing::warn!("Unknown server command: {}", cmd);
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
        self.playernum = msg.read_short() as i32;
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
        let index = msg.read_short() as usize;
        let value = msg.read_string();
        if index < self.configstrings.len() {
            self.configstrings[index] = value;
        }
    }

    fn parse_baseline(&mut self, msg: &mut NetMsg) {
        // Read entity number from the baseline message.
        // In the full protocol this uses delta-compressed entity bits;
        // for now we read a short entity number as a placeholder.
        let _entity_num = msg.read_short();
        // TODO: full delta parsing using net_msg delta functions
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
        let _ = msg;
    }

    fn parse_packet_entities(&mut self, msg: &mut NetMsg) {
        // TODO: parse delta-compressed entity states
        let _ = msg;
    }
}
