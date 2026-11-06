use serde::{de::DeserializeOwned, Serialize};

use crate::{RejectReason, MAX_MESSAGE_BYTES, PROTOCOL_VERSION};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProtocolError {
    #[error("mensagem com {0} bytes excede o limite")]
    TooLarge(usize),
    #[error("mensagem malformada: {0}")]
    Malformed(String),
    #[error("bytes sobrando após a mensagem")]
    TrailingBytes,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    bincode::serde::encode_to_vec(msg, bincode::config::standard())
        .expect("tipos do protocolo sempre serializam")
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::TooLarge(bytes.len()));
    }
    let config = bincode::config::standard().with_limit::<MAX_MESSAGE_BYTES>();
    let (msg, read) = bincode::serde::decode_from_slice(bytes, config)
        .map_err(|e| ProtocolError::Malformed(e.to_string()))?;
    if read != bytes.len() {
        return Err(ProtocolError::TrailingBytes);
    }
    Ok(msg)
}

pub fn check_version(v: u16) -> Result<(), RejectReason> {
    if v == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(RejectReason::VersionMismatch {
            host: PROTOCOL_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::*;
    use proptest::prelude::*;

    fn pet(id: u8, animal: Animal) -> PetState {
        PetState {
            id: PlayerId(id),
            animal,
            pos: Vec2 {
                x: 0.25 * id as f32,
                y: 0.5,
            },
            facing: Facing::Left,
            anim: AnimState::Walk,
            last_input_seq: 42,
        }
    }

    #[test]
    fn roundtrip_guest_and_host_msgs() {
        let hello = GuestMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
            name: "Tamy".into(),
            desired_animal: Animal::Capybara,
        };
        assert_eq!(decode::<GuestMsg>(&encode(&hello)), Ok(hello));

        let input = GuestMsg::Input(InputFrame {
            seq: 7,
            move_x: 1,
            move_y: -1,
            run: true,
        });
        assert_eq!(decode::<GuestMsg>(&encode(&input)), Ok(input));

        let welcome = HostMsg::Welcome {
            player_id: PlayerId(2),
            animal: Animal::Horse,
            snapshot: Snapshot {
                tick: 99,
                pets: Animal::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, a)| pet(i as u8, *a))
                    .collect(),
            },
        };
        assert_eq!(decode::<HostMsg>(&encode(&welcome)), Ok(welcome));
    }

    proptest! {
        #[test]
        fn roundtrip_input_frame(seq: u32, move_x: i8, move_y: i8, run: bool) {
            let f = InputFrame { seq, move_x, move_y, run };
            prop_assert_eq!(decode::<InputFrame>(&encode(&f)), Ok(f));
        }
    }

    #[test]
    fn rejects_oversized() {
        let bytes = vec![0u8; MAX_MESSAGE_BYTES + 1];
        assert!(matches!(
            decode::<GuestMsg>(&bytes),
            Err(ProtocolError::TooLarge(_))
        ));
    }

    #[test]
    fn rejects_trailing_bytes() {
        let mut bytes = encode(&GuestMsg::Input(InputFrame {
            seq: 1,
            move_x: 0,
            move_y: 0,
            run: false,
        }));
        bytes.push(0xFF);
        assert_eq!(
            decode::<GuestMsg>(&bytes),
            Err(ProtocolError::TrailingBytes)
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            decode::<HostMsg>(&[0xFF; 8]),
            Err(ProtocolError::Malformed(_))
        ));
    }

    #[test]
    fn sanitized_clamps() {
        let f = InputFrame {
            seq: 1,
            move_x: -128,
            move_y: 7,
            run: true,
        }
        .sanitized();
        assert_eq!((f.move_x, f.move_y, f.run, f.seq), (-1, 1, true, 1));
    }

    #[test]
    fn version_check() {
        assert_eq!(check_version(PROTOCOL_VERSION), Ok(()));
        assert_eq!(
            check_version(PROTOCOL_VERSION + 1),
            Err(RejectReason::VersionMismatch {
                host: PROTOCOL_VERSION
            })
        );
    }
}
