use bitflags::bitflags;
use serde::{Deserialize, Serialize};

/// Server-to-client ops. Replaces svc_ops_e from common.h:192-219
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SvcOp {
    Bad = 0,
    MuzzleFlash = 1,
    MuzzleFlash2 = 2,
    TempEntity = 3,
    Layout = 4,
    Inventory = 5,
    Nop = 6,
    Disconnect = 7,
    Reconnect = 8,
    Sound = 9,
    Print = 10,
    StuffText = 11,
    ServerData = 12,
    ConfigString = 13,
    SpawnBaseline = 14,
    CenterPrint = 15,
    Download = 16,
    PlayerInfo = 17,
    PacketEntities = 18,
    DeltaPacketEntities = 19,
    Frame = 20,
}

/// Client-to-server ops. Replaces clc_ops_e from common.h:224-231
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClcOp {
    Bad = 0,
    Nop = 1,
    Move = 2,
    UserInfo = 3,
    StringCmd = 4,
}

bitflags! {
    /// Entity update flags. Replaces U_* defines from common.h:284-316
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct UpdateFlags: u32 {
        const ORIGIN1    = 1 << 0;
        const ORIGIN2    = 1 << 1;
        const ANGLE2     = 1 << 2;
        const ANGLE3     = 1 << 3;
        const FRAME8     = 1 << 4;
        const EVENT      = 1 << 5;
        const REMOVE     = 1 << 6;
        const MOREBITS1  = 1 << 7;
        const NUMBER16   = 1 << 8;
        const ORIGIN3    = 1 << 9;
        const ANGLE1     = 1 << 10;
        const MODEL      = 1 << 11;
        const RENDERFX8  = 1 << 12;
        // bit 13 is unused in the protocol
        const EFFECTS8   = 1 << 14;
        const MOREBITS2  = 1 << 15;
        const SKIN8      = 1 << 16;
        const FRAME16    = 1 << 17;
        const RENDERFX16 = 1 << 18;
        const EFFECTS16  = 1 << 19;
        const MODEL2     = 1 << 20;
        const MODEL3     = 1 << 21;
        const MODEL4     = 1 << 22;
        const MOREBITS3  = 1 << 23;
        const OLDORIGIN  = 1 << 24;
        const SKIN16     = 1 << 25;
        const SOUND      = 1 << 26;
        const SOLID      = 1 << 27;
    }

    /// Player state communication flags. Replaces PS_* defines from common.h:236-252
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct PlayerStateFlags: u16 {
        const M_TYPE         = 1 << 0;
        const M_ORIGIN       = 1 << 1;
        const M_VELOCITY     = 1 << 2;
        const M_TIME         = 1 << 3;
        const M_FLAGS        = 1 << 4;
        const M_GRAVITY      = 1 << 5;
        const M_DELTA_ANGLES = 1 << 6;
        const VIEWOFFSET     = 1 << 7;
        const VIEWANGLES     = 1 << 8;
        const KICKANGLES     = 1 << 9;
        const BLEND          = 1 << 10;
        const FOV            = 1 << 11;
        const WEAPONINDEX    = 1 << 12;
        const WEAPONFRAME    = 1 << 13;
        const RDFLAGS        = 1 << 14;
    }
}

/// TryFrom<u8> for SvcOp — needed for reading ops from network messages
impl TryFrom<u8> for SvcOp {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Bad),
            1 => Ok(Self::MuzzleFlash),
            2 => Ok(Self::MuzzleFlash2),
            3 => Ok(Self::TempEntity),
            4 => Ok(Self::Layout),
            5 => Ok(Self::Inventory),
            6 => Ok(Self::Nop),
            7 => Ok(Self::Disconnect),
            8 => Ok(Self::Reconnect),
            9 => Ok(Self::Sound),
            10 => Ok(Self::Print),
            11 => Ok(Self::StuffText),
            12 => Ok(Self::ServerData),
            13 => Ok(Self::ConfigString),
            14 => Ok(Self::SpawnBaseline),
            15 => Ok(Self::CenterPrint),
            16 => Ok(Self::Download),
            17 => Ok(Self::PlayerInfo),
            18 => Ok(Self::PacketEntities),
            19 => Ok(Self::DeltaPacketEntities),
            20 => Ok(Self::Frame),
            other => Err(other),
        }
    }
}

impl TryFrom<u8> for ClcOp {
    type Error = u8;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Bad),
            1 => Ok(Self::Nop),
            2 => Ok(Self::Move),
            3 => Ok(Self::UserInfo),
            4 => Ok(Self::StringCmd),
            other => Err(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn svc_op_values() {
        assert_eq!(SvcOp::Bad as u8, 0);
        assert_eq!(SvcOp::Frame as u8, 20);
    }

    #[test]
    fn clc_op_values() {
        assert_eq!(ClcOp::Bad as u8, 0);
        assert_eq!(ClcOp::StringCmd as u8, 4);
    }

    #[test]
    fn update_flags_bitfield() {
        let flags = UpdateFlags::ORIGIN1 | UpdateFlags::ORIGIN2;
        assert!(flags.contains(UpdateFlags::ORIGIN1));
        assert!(!flags.contains(UpdateFlags::ANGLE1));
    }

    #[test]
    fn player_state_flags() {
        let flags = PlayerStateFlags::M_TYPE | PlayerStateFlags::VIEWANGLES;
        assert!(flags.contains(PlayerStateFlags::M_TYPE));
        assert!(!flags.contains(PlayerStateFlags::FOV));
    }

    #[test]
    fn svc_op_try_from() {
        assert_eq!(SvcOp::try_from(0u8), Ok(SvcOp::Bad));
        assert_eq!(SvcOp::try_from(20u8), Ok(SvcOp::Frame));
        assert_eq!(SvcOp::try_from(255u8), Err(255u8));
    }

    #[test]
    fn clc_op_try_from() {
        assert_eq!(ClcOp::try_from(2u8), Ok(ClcOp::Move));
        assert_eq!(ClcOp::try_from(99u8), Err(99u8));
    }
}
