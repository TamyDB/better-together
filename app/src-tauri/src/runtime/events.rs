//! Eventos que os loops emitem para o WebView, com os payloads JSON que a UI consome.

use dm_protocol::{RejectReason, Snapshot};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::session::guest::GuestEvent;
use crate::session::net::Out;

pub const SNAPSHOT_EVENT: &str = "sim://snapshot";
pub const NET_SEND_EVENT: &str = "net://send";
pub const NET_VIDEO_EVENT: &str = "net://video";
pub const ROSTER_EVENT: &str = "session://roster";
pub const LOBBY_EVENT: &str = "session://lobby";
pub const WELCOME_EVENT: &str = "session://welcome";
pub const REJECTED_EVENT: &str = "session://rejected";
pub const HOST_LOST_EVENT: &str = "session://host-lost";

/// Nome do evento e payload, sem depender de `AppHandle` (testável).
pub type UiEvent = (&'static str, Value);

pub fn snapshot_event(snapshot: &Snapshot) -> UiEvent {
    (SNAPSHOT_EVENT, json!(snapshot))
}

pub fn out_event(out: Out) -> UiEvent {
    match out {
        Out::Send {
            peer,
            channel,
            bytes,
        } => (
            NET_SEND_EVENT,
            json!({ "peer": peer, "channel": channel, "bytes": bytes }),
        ),
        Out::Roster(roster) => (ROSTER_EVENT, json!(roster)),
        Out::Video { peer, on } => (NET_VIDEO_EVENT, json!({ "peer": peer, "on": on })),
    }
}

pub fn guest_event(ev: GuestEvent) -> UiEvent {
    match ev {
        GuestEvent::Lobby(taken) => (LOBBY_EVENT, json!({ "taken": taken })),
        GuestEvent::Welcome { me, animal, aspect } => (
            WELCOME_EVENT,
            json!({ "me": me, "animal": animal, "aspect": aspect }),
        ),
        GuestEvent::Rejected(reason) => (REJECTED_EVENT, rejected_payload(&reason)),
        GuestEvent::Roster(roster) => (ROSTER_EVENT, json!(roster)),
        GuestEvent::HostLost => (HOST_LOST_EVENT, Value::Null),
    }
}

/// `RejectReason` serializa em bincode para a rede; o JSON da UI é definido aqui, à parte.
fn rejected_payload(reason: &RejectReason) -> Value {
    match reason {
        RejectReason::VersionMismatch { host } => {
            json!({ "reason": "version_mismatch", "host": host })
        }
        RejectReason::RoomFull => json!({ "reason": "room_full" }),
        RejectReason::AnimalTaken => json!({ "reason": "animal_taken" }),
    }
}

pub fn emit_to_ui(app: &AppHandle, (name, payload): UiEvent) {
    if let Err(e) = app.emit(name, payload) {
        log::warn!("falha ao emitir {name}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dm_protocol::{Animal, Channel, PlayerId, RosterEntry};

    #[test]
    fn net_send_payload() {
        let out = Out::Send {
            peer: 7,
            channel: Channel::Events,
            bytes: vec![1, 2, 255],
        };
        assert_eq!(
            out_event(out),
            (
                "net://send",
                json!({ "peer": 7, "channel": "events", "bytes": [1, 2, 255] })
            )
        );
    }

    #[test]
    fn net_video_payload() {
        assert_eq!(
            out_event(Out::Video { peer: 7, on: true }),
            ("net://video", json!({ "peer": 7, "on": true }))
        );
        assert_eq!(
            out_event(Out::Video { peer: 9, on: false }),
            ("net://video", json!({ "peer": 9, "on": false }))
        );
    }

    #[test]
    fn roster_payload() {
        let roster = vec![RosterEntry {
            id: PlayerId(1),
            name: "Ana".into(),
            animal: Animal::Horse,
            asleep: true,
        }];
        let expected = json!([{ "id": 1, "name": "Ana", "animal": "horse", "asleep": true }]);
        assert_eq!(
            out_event(Out::Roster(roster.clone())),
            ("session://roster", expected.clone())
        );
        assert_eq!(
            guest_event(GuestEvent::Roster(roster)),
            ("session://roster", expected)
        );
    }

    #[test]
    fn guest_event_payloads() {
        assert_eq!(
            guest_event(GuestEvent::Lobby(vec![Animal::Duck])),
            ("session://lobby", json!({ "taken": ["duck"] }))
        );
        assert_eq!(
            guest_event(GuestEvent::Welcome {
                me: PlayerId(2),
                animal: Animal::Capybara,
                aspect: 2.0,
            }),
            (
                "session://welcome",
                json!({ "me": 2, "animal": "capybara", "aspect": 2.0 })
            )
        );
        assert_eq!(
            guest_event(GuestEvent::HostLost),
            ("session://host-lost", Value::Null)
        );
    }

    #[test]
    fn rejected_payloads() {
        let payload = |r| guest_event(GuestEvent::Rejected(r));
        assert_eq!(
            payload(RejectReason::AnimalTaken),
            ("session://rejected", json!({ "reason": "animal_taken" }))
        );
        assert_eq!(
            payload(RejectReason::RoomFull),
            ("session://rejected", json!({ "reason": "room_full" }))
        );
        assert_eq!(
            payload(RejectReason::VersionMismatch { host: 3 }),
            (
                "session://rejected",
                json!({ "reason": "version_mismatch", "host": 3 })
            )
        );
    }
}
