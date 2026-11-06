use serde::{Deserialize, Serialize};

use crate::types::{Animal, InputFrame, PlayerId, Snapshot};

/// Mensagens Convidado → Host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum GuestMsg {
    Hello {
        protocol_version: u16,
        name: String,
        desired_animal: Animal,
    },
    Input(InputFrame),
}

/// Mensagens Host → Convidado.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum HostMsg {
    Welcome {
        player_id: PlayerId,
        animal: Animal,
        snapshot: Snapshot,
    },
    Snapshot(Snapshot),
    Reject(RejectReason),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    VersionMismatch { host: u16 },
    RoomFull,
    AnimalTaken,
}
