//! Registro de salas: o código vale enquanto o Host estiver conectado (sem expiração por tempo).

use std::collections::HashMap;

use rand::Rng;

use crate::{code, proto::SignalError, ConnId};

pub const MAX_GUESTS: usize = 3;
pub const MAX_ROOMS: usize = 1000;
/// Tentativas de sortear um código sem colisão antes de desistir.
const MAX_CODE_ATTEMPTS: usize = 32;

struct Room {
    host: ConnId,
    guests: Vec<ConnId>,
}

/// O que mudou quando uma conexão saiu.
#[derive(Debug, PartialEq, Eq)]
pub enum Leave {
    NotInRoom,
    Guest {
        host: ConnId,
    },
    /// A sala foi apagada; estes convidados ficaram sem Host.
    Host {
        guests: Vec<ConnId>,
    },
}

#[derive(Default)]
pub struct Registry {
    rooms: HashMap<String, Room>,
    /// Conexão -> código da sala em que está (Host ou convidado).
    members: HashMap<ConnId, String>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&mut self, host: ConnId, rng: &mut impl Rng) -> Result<String, SignalError> {
        if self.members.contains_key(&host) {
            return Err(SignalError::AlreadyInRoom);
        }
        if self.rooms.len() >= MAX_ROOMS {
            return Err(SignalError::ServerFull);
        }
        let code = (0..MAX_CODE_ATTEMPTS)
            .map(|_| code::generate(rng))
            .find(|c| !self.rooms.contains_key(c))
            .ok_or(SignalError::ServerFull)?;
        self.rooms.insert(
            code.clone(),
            Room {
                host,
                guests: Vec::new(),
            },
        );
        self.members.insert(host, code.clone());
        Ok(code)
    }

    /// Devolve o código normalizado e o Host da sala.
    pub fn join(&mut self, guest: ConnId, code: &str) -> Result<(String, ConnId), SignalError> {
        if self.members.contains_key(&guest) {
            return Err(SignalError::AlreadyInRoom);
        }
        let code = code::normalize(code).ok_or(SignalError::RoomNotFound)?;
        let room = self.rooms.get_mut(&code).ok_or(SignalError::RoomNotFound)?;
        if room.guests.len() >= MAX_GUESTS {
            return Err(SignalError::RoomFull);
        }
        room.guests.push(guest);
        let host = room.host;
        self.members.insert(guest, code.clone());
        Ok((code, host))
    }

    pub fn leave(&mut self, conn: ConnId) -> Leave {
        let Some(code) = self.members.remove(&conn) else {
            return Leave::NotInRoom;
        };
        let Some(room) = self.rooms.get_mut(&code) else {
            return Leave::NotInRoom;
        };
        if room.host == conn {
            let room = self.rooms.remove(&code).expect("sala existe");
            for g in &room.guests {
                self.members.remove(g);
            }
            Leave::Host {
                guests: room.guests,
            }
        } else {
            room.guests.retain(|g| *g != conn);
            Leave::Guest { host: room.host }
        }
    }

    /// Só Host <-> convidado da mesma sala.
    pub fn can_signal(&self, from: ConnId, to: ConnId) -> bool {
        let (Some(a), Some(b)) = (self.members.get(&from), self.members.get(&to)) else {
            return false;
        };
        if a != b {
            return false;
        }
        self.rooms
            .get(a)
            .is_some_and(|r| (r.host == from) != (r.host == to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    fn rng() -> StdRng {
        StdRng::seed_from_u64(7)
    }

    #[test]
    fn create_join_leave() {
        let mut r = Registry::new();
        let mut rng = rng();
        let host = ConnId(1);
        let code = r.create(host, &mut rng).unwrap();
        let lower = code.to_lowercase();
        assert_eq!(r.join(ConnId(2), &lower), Ok((code.clone(), host)));
        assert_eq!(r.join(ConnId(3), &code), Ok((code.clone(), host)));
        assert_eq!(r.join(ConnId(4), &code), Ok((code.clone(), host)));
        assert_eq!(r.join(ConnId(5), &code), Err(SignalError::RoomFull));
        assert_eq!(r.join(ConnId(2), &code), Err(SignalError::AlreadyInRoom));
        assert_eq!(r.leave(ConnId(2)), Leave::Guest { host });
        assert_eq!(r.leave(ConnId(2)), Leave::NotInRoom);
        assert_eq!(
            r.leave(host),
            Leave::Host {
                guests: vec![ConnId(3), ConnId(4)]
            }
        );
        assert_eq!(r.join(ConnId(6), &code), Err(SignalError::RoomNotFound));
    }

    #[test]
    fn code_valid_while_session_lives() {
        let mut r = Registry::new();
        let host = ConnId(1);
        let code = r.create(host, &mut rng()).unwrap();
        assert!(r.join(ConnId(2), &code).is_ok());
        assert_eq!(r.leave(ConnId(2)), Leave::Guest { host });
        assert!(r.join(ConnId(2), &code).is_ok());
        r.leave(host);
        assert_eq!(r.join(ConnId(3), &code), Err(SignalError::RoomNotFound));
    }

    #[test]
    fn signal_scope() {
        let mut r = Registry::new();
        let mut rng = rng();
        let (h1, h2) = (ConnId(1), ConnId(10));
        let c1 = r.create(h1, &mut rng).unwrap();
        let c2 = r.create(h2, &mut rng).unwrap();
        let (a, b, x) = (ConnId(2), ConnId(3), ConnId(11));
        r.join(a, &c1).unwrap();
        r.join(b, &c1).unwrap();
        r.join(x, &c2).unwrap();
        assert!(r.can_signal(h1, a) && r.can_signal(a, h1));
        assert!(!r.can_signal(a, b));
        assert!(!r.can_signal(a, x));
        assert!(!r.can_signal(h1, h2));
        assert!(!r.can_signal(h1, ConnId(99)));
    }

    #[test]
    fn max_rooms() {
        let mut r = Registry::new();
        let mut rng = rng();
        for i in 0..MAX_ROOMS as u64 {
            r.create(ConnId(i), &mut rng).unwrap();
        }
        assert_eq!(
            r.create(ConnId(5000), &mut rng),
            Err(SignalError::ServerFull)
        );
    }

    #[test]
    fn leave_cleans_up() {
        let mut r = Registry::new();
        let mut rng = rng();
        let host = ConnId(1);
        let code = r.create(host, &mut rng).unwrap();
        for g in 2..=4 {
            r.join(ConnId(g), &code).unwrap();
        }
        r.leave(ConnId(2));
        assert!(r.join(ConnId(5), &code).is_ok());
        r.leave(host);
        assert!(!r.can_signal(host, ConnId(3)));
        assert!(!r.can_signal(ConnId(3), host));
        let other = r.create(ConnId(10), &mut rng).unwrap();
        assert!(r.join(ConnId(3), &other).is_ok());
        assert!(r.create(ConnId(4), &mut rng).is_ok());
        assert!(r.create(ConnId(5), &mut rng).is_ok());
    }

    #[test]
    fn leave_host_frees_room_slot() {
        let mut r = Registry::new();
        let mut rng = rng();
        for i in 0..MAX_ROOMS as u64 {
            r.create(ConnId(i), &mut rng).unwrap();
        }
        r.leave(ConnId(0));
        assert!(r.create(ConnId(5000), &mut rng).is_ok());
    }

    #[test]
    fn rejects_double_create_and_garbage() {
        let mut r = Registry::new();
        let mut rng = rng();
        r.create(ConnId(1), &mut rng).unwrap();
        assert_eq!(
            r.create(ConnId(1), &mut rng),
            Err(SignalError::AlreadyInRoom)
        );
        assert_eq!(r.join(ConnId(2), "lixo"), Err(SignalError::RoomNotFound));
    }

    #[test]
    fn collision_picks_another_code() {
        let mut r = Registry::new();
        let a = r.create(ConnId(1), &mut StdRng::seed_from_u64(1)).unwrap();
        let b = r.create(ConnId(2), &mut StdRng::seed_from_u64(1)).unwrap();
        assert_ne!(a, b);
    }
}
