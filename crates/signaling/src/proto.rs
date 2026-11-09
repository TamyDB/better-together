//! Mensagens JSON trocadas com o servidor de sinalização.

use serde::{Deserialize, Serialize};

/// Identificador de uma conexão; vira o `peer_id` visto pelos outros.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnId(pub u64);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    CreateRoom,
    JoinRoom { code: String },
    Signal { to: ConnId, data: serde_json::Value },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    RoomCreated {
        code: String,
        peer_id: ConnId,
    },
    Joined {
        code: String,
        peer_id: ConnId,
        host_id: ConnId,
    },
    PeerJoined {
        peer_id: ConnId,
    },
    PeerLeft {
        peer_id: ConnId,
    },
    Signal {
        from: ConnId,
        data: serde_json::Value,
    },
    HostLeft,
    Error {
        reason: SignalError,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalError {
    RoomNotFound,
    RoomFull,
    RateLimited,
    BadMessage,
    AlreadyInRoom,
    BadTarget,
    ServerFull,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_json_shape() {
        let s = serde_json::to_string(&ServerMsg::Error {
            reason: SignalError::RoomFull,
        })
        .unwrap();
        assert_eq!(s, r#"{"type":"error","reason":"room_full"}"#);
        let m: ClientMsg =
            serde_json::from_str(r#"{"type":"join_room","code":"PATO-4821"}"#).unwrap();
        assert_eq!(
            m,
            ClientMsg::JoinRoom {
                code: "PATO-4821".into()
            }
        );
    }
}
