//! Modo atual do app (Launcher, Host ou Convidado) e o que sobrevive entre sessões.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};

use dm_protocol::{sanitize_name, Animal, Channel, InputFrame, RosterEntry, MAX_MESSAGE_BYTES};
use serde::{Deserialize, Serialize};

use crate::input::keys::Key;
use crate::pet_mode::PetMode;
use crate::runtime::guest_loop::{GuestCommand, GuestHandle};
use crate::runtime::host_loop::{HostHandle, SimSink};
use crate::session::net::{NetIn, PeerId};

const SESSION_ACTIVE: &str = "já existe uma sessão ativa";

/// O que havia ao encerrar a sessão (`AppState::end_session`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    None,
    Host,
    Guest,
}

/// Última entrada bem-sucedida numa sala; sobrevive ao "Voltar" e morre com o app, como o token.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LastJoin {
    pub code: String,
    pub name: String,
    pub animal: Animal,
}

/// Código de sala já normalizado: 4 letras maiúsculas, hífen e 4 dígitos (`PATO-4821`).
pub fn valid_code(code: &str) -> bool {
    let b = code.as_bytes();
    b.len() == 9
        && b[..4].iter().all(u8::is_ascii_uppercase)
        && b[4] == b'-'
        && b[5..].iter().all(u8::is_ascii_digit)
}

pub enum Mode {
    Launcher,
    Host {
        handle: Arc<HostHandle>,
        pet_mode: Arc<PetMode>,
    },
    Guest {
        handle: GuestHandle,
    },
}

impl Mode {
    pub fn kind(&self) -> &'static str {
        match self {
            Mode::Launcher => "launcher",
            Mode::Host { .. } => "host",
            Mode::Guest { .. } => "guest",
        }
    }
}

/// Só se sai do `Launcher`; em qualquer outro modo já há uma sessão em andamento.
pub fn transition(current: &Mode, target: &str) -> Result<(), String> {
    match current {
        Mode::Launcher => {
            log::debug!("modo: {} → {target}", current.kind());
            Ok(())
        }
        Mode::Host { .. } | Mode::Guest { .. } => Err(SESSION_ACTIVE.to_string()),
    }
}

/// Token de reconexão: 0 significa "sem reconexão" para o Host, então nunca é sorteado.
pub fn non_zero_token(mut draw: impl FnMut() -> u64) -> u64 {
    loop {
        let token = draw();
        if token != 0 {
            return token;
        }
    }
}

/// Destino do `PetMode`, religado a cada sessão de Host. O hook e o keep-alive guardam o
/// `PetMode`; se ele guardasse o `HostHandle`, o canal da thread nunca fecharia.
#[derive(Default)]
pub struct PetSink(Mutex<Option<Arc<dyn SimSink>>>);

impl PetSink {
    fn bind(&self, sink: Arc<dyn SimSink>) {
        *lock(&self.0) = Some(sink);
    }

    fn clear(&self) {
        *lock(&self.0) = None;
    }
}

impl SimSink for PetSink {
    fn send_input(&self, frame: InputFrame) {
        if let Some(sink) = lock(&self.0).as_ref() {
            sink.send_input(frame);
        }
    }

    fn set_autonomous(&self, on: bool) {
        if let Some(sink) = lock(&self.0).as_ref() {
            sink.set_autonomous(on);
        }
    }
}

/// Estado gerenciado pelo Tauri. O lock de `Mode` nunca é segurado durante criação de
/// janela ou qualquer operação que dependa da thread principal: comandos síncronos rodam
/// nela e também pegam esse lock. Sob ele só há envios em canal sem limite e o spawn da
/// thread do convidado.
pub struct AppState {
    mode: Mutex<Mode>,
    /// Um `host_start` em andamento (cria janelas fora do lock). Escrito sob o lock de `Mode`.
    starting: AtomicBool,
    rejoin_token: u64,
    /// Lock próprio: nunca segurado junto com o de `Mode`.
    last_join: Mutex<Option<LastJoin>>,
    pet_sink: Arc<PetSink>,
    pet_mode: Arc<PetMode>,
    host_services: Once,
}

/// Reserva de um `host_start`; liberada ao sair de escopo, com sucesso ou erro.
pub struct StartGuard<'a>(&'a AppState);

impl Drop for StartGuard<'_> {
    fn drop(&mut self) {
        self.0.starting.store(false, Ordering::Release);
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // O estado protegido continua válido mesmo se um comando entrou em pânico.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl AppState {
    pub fn new() -> AppState {
        let pet_sink = Arc::new(PetSink::default());
        AppState {
            mode: Mutex::new(Mode::Launcher),
            starting: AtomicBool::new(false),
            rejoin_token: non_zero_token(rand::random),
            last_join: Mutex::new(None),
            pet_mode: Arc::new(PetMode::new(pet_sink.clone())),
            pet_sink,
            host_services: Once::new(),
        }
    }

    /// Token único por execução: reentrar na sala depois de cair recupera o mesmo pet.
    pub fn rejoin_token(&self) -> u64 {
        self.rejoin_token
    }

    /// Guarda a escolha aceita pelo Host para a reconexão automática.
    pub fn remember_join(&self, code: String, name: &str, animal: Animal) -> Result<(), String> {
        if !valid_code(&code) {
            return Err("código inválido".to_string());
        }
        let name = sanitize_name(name, "Jogador");
        *lock(&self.last_join) = Some(LastJoin { code, name, animal });
        Ok(())
    }

    pub fn last_join(&self) -> Option<LastJoin> {
        lock(&self.last_join).clone()
    }

    /// Reserva o início de uma sessão de Host, recusando outra em andamento.
    pub fn begin_start(&self, target: &str) -> Result<StartGuard<'_>, String> {
        let mode = lock(&self.mode);
        transition(&mode, target)?;
        if self.starting.swap(true, Ordering::AcqRel) {
            return Err(SESSION_ACTIVE.to_string());
        }
        Ok(StartGuard(self))
    }

    /// Instala os serviços do Host uma única vez por execução: a bandeja, o hook e o
    /// keep-alive ficam ativos (inertes fora do modo Host).
    pub fn install_host_services_once(&self, install: impl FnOnce(Arc<PetMode>)) {
        let pet_mode = self.pet_mode.clone();
        self.host_services.call_once(|| install(pet_mode));
    }

    /// Liga o `PetMode` ao loop desta sessão.
    pub fn bind_pet_sink(&self, handle: Arc<HostHandle>) -> Arc<PetMode> {
        self.pet_sink.bind(handle);
        self.pet_mode.clone()
    }

    pub fn set_mode(&self, mode: Mode) {
        *lock(&self.mode) = mode;
    }

    /// Entra em modo Convidado, com a checagem de transição sob o lock.
    pub fn enter_guest(&self, spawn: impl FnOnce() -> GuestHandle) -> Result<(), String> {
        let mut mode = lock(&self.mode);
        transition(&mode, "guest")?;
        if self.starting.load(Ordering::Acquire) {
            return Err(SESSION_ACTIVE.to_string());
        }
        *mode = Mode::Guest { handle: spawn() };
        Ok(())
    }

    /// Volta ao `Launcher`. Ao retornar, a thread da sessão já não tem nenhum `Sender` e sai
    /// na próxima passagem; o retorno diz se havia sessão de Host, de Convidado ou nenhuma.
    pub fn end_session(&self) -> Result<Ended, String> {
        let previous = {
            let mut mode = lock(&self.mode);
            if self.starting.load(Ordering::Acquire) {
                return Err("a sessão ainda está iniciando".to_string());
            }
            std::mem::replace(&mut *mode, Mode::Launcher)
        };
        let ended = match previous {
            Mode::Launcher => Ended::None,
            Mode::Host { .. } => Ended::Host,
            Mode::Guest { .. } => Ended::Guest,
        };
        if let Mode::Host { pet_mode, .. } = &previous {
            // Depois do `Launcher` publicado: um toggle tardio já não encontra o modo Host.
            pet_mode.exit();
            self.pet_sink.clear();
        }
        Ok(ended)
    }

    /// Só no modo Host. Sob o lock de `Mode` para não religar o modo bichinho depois de um
    /// `end_session`, o que deixaria o hook engolindo o teclado fora da sessão.
    pub fn toggle_pet(&self) -> Result<(), String> {
        match &*lock(&self.mode) {
            Mode::Host { pet_mode, .. } => {
                pet_mode.toggle();
                Ok(())
            }
            Mode::Launcher | Mode::Guest { .. } => {
                Err("o modo bichinho só existe no modo Host".to_string())
            }
        }
    }

    pub fn key_event(&self, key: Key, down: bool) {
        match &*lock(&self.mode) {
            Mode::Host { pet_mode, .. } => pet_mode.on_key(key, down),
            Mode::Guest { handle } => handle.send(GuestCommand::Key(key, down)),
            Mode::Launcher => {}
        }
    }

    pub fn guest_choose(&self, name: String, animal: Animal) -> Result<(), String> {
        match &*lock(&self.mode) {
            Mode::Guest { handle } => {
                handle.send(GuestCommand::Choose { name, animal });
                Ok(())
            }
            Mode::Launcher | Mode::Host { .. } => Err("não há sessão de convidado".to_string()),
        }
    }

    /// Último roster do Host; vazio fora do modo Host. O lock de `Mode` não cobre a cópia.
    pub fn host_roster(&self) -> Vec<RosterEntry> {
        let handle = match &*lock(&self.mode) {
            Mode::Host { handle, .. } => handle.clone(),
            _ => return Vec::new(),
        };
        handle.roster()
    }

    /// Repassa um evento de rede ao loop do modo atual. Depois de um `session_reset` ainda
    /// podem chegar eventos atrasados; sem loop, são descartados.
    pub fn net(&self, ev: NetIn) {
        match &*lock(&self.mode) {
            Mode::Host { handle, .. } => handle.net(ev),
            Mode::Guest { handle } => handle.send(GuestCommand::Net(ev)),
            Mode::Launcher => log::debug!("evento de rede sem sessão ativa; ignorado"),
        }
    }
}

/// Valida o que vem do WebView para `net_recv`: canal conhecido e tamanho aceitável.
pub fn parse_recv(peer: PeerId, channel: &str, bytes: Vec<u8>) -> Result<NetIn, String> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err("mensagem excede o tamanho máximo".to_string());
    }
    let channel = Channel::parse(channel).ok_or_else(|| "canal inválido".to_string())?;
    Ok(NetIn::Recv {
        peer,
        channel,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::backlog::RecvBacklog;
    use crate::runtime::host_loop::HostCommand;
    use std::sync::mpsc::{self, Receiver, TryRecvError};

    fn guest_handle() -> (GuestHandle, Receiver<GuestCommand>) {
        let (tx, rx) = mpsc::channel();
        (GuestHandle::from_parts(tx, RecvBacklog::default()), rx)
    }

    fn guest_mode() -> Mode {
        Mode::Guest {
            handle: guest_handle().0,
        }
    }

    fn host_state() -> (AppState, Receiver<HostCommand>) {
        let state = AppState::new();
        let (tx, rx) = mpsc::channel();
        let handle = Arc::new(HostHandle::from_parts(tx, RecvBacklog::default()));
        let pet_mode = state.bind_pet_sink(handle.clone());
        state.set_mode(Mode::Host { handle, pet_mode });
        (state, rx)
    }

    #[test]
    fn mode_transition() {
        assert!(transition(&Mode::Launcher, "host").is_ok());
        assert_eq!(
            transition(&guest_mode(), "host"),
            Err("já existe uma sessão ativa".to_string())
        );
        assert_eq!(guest_mode().kind(), "guest");
    }

    #[test]
    fn app_state_token_never_zero() {
        let mut draws = [0, 0, 7].into_iter();
        assert_eq!(non_zero_token(|| draws.next().unwrap()), 7);
        assert_ne!(AppState::new().rejoin_token(), 0);
    }

    #[test]
    fn app_state_token_stable_across_sessions() {
        let state = AppState::new();
        let token = state.rejoin_token();
        state.enter_guest(|| guest_handle().0).unwrap();
        state.end_session().unwrap();
        state.enter_guest(|| guest_handle().0).unwrap();
        assert_eq!(state.rejoin_token(), token);
    }

    #[test]
    fn remember_join_rules() {
        let state = AppState::new();
        assert_eq!(state.last_join(), None);
        state
            .remember_join("PATO-4821".into(), "  Ana\n ", Animal::Duck)
            .unwrap();
        let expected = LastJoin {
            code: "PATO-4821".into(),
            name: "Ana".into(),
            animal: Animal::Duck,
        };
        assert_eq!(state.last_join(), Some(expected.clone()));
        for bad in [
            "pato-4821",
            "PATO4821",
            "PATO-482",
            "PATO-48211",
            "PAT0-4821",
        ] {
            assert_eq!(
                state.remember_join(bad.into(), "X", Animal::Horse),
                Err("código inválido".to_string())
            );
        }
        assert_eq!(state.last_join(), Some(expected.clone()));
        state.enter_guest(|| guest_handle().0).unwrap();
        state.end_session().unwrap();
        assert_eq!(state.last_join(), Some(expected));
    }

    #[test]
    fn valid_code_multibyte_is_false_without_panic() {
        // Fatiar por bytes não pode cair no meio de um caractere.
        for (code, len) in [
            ("PATÓ-4821", 10),
            ("PATÓ-482", 9),
            ("PATO-48²", 9),
            ("ÁBC-4821", 9),
        ] {
            assert_eq!(code.len(), len, "{code}");
            assert!(!valid_code(code), "{code}");
        }
    }

    #[test]
    fn remember_join_blank_name_becomes_default() {
        let state = AppState::new();
        for blank in ["   ", " \n\t "] {
            state
                .remember_join("PATO-4821".into(), blank, Animal::Chicken)
                .unwrap();
            assert_eq!(state.last_join().unwrap().name, "Jogador");
        }
    }

    #[test]
    fn app_state_rejects_second_session() {
        let state = AppState::new();
        state.enter_guest(|| guest_handle().0).unwrap();
        assert_eq!(
            state.enter_guest(|| guest_handle().0),
            Err("já existe uma sessão ativa".to_string())
        );
        assert!(state.begin_start("host").is_err());
    }

    #[test]
    fn app_state_start_reservation_is_exclusive() {
        let state = AppState::new();
        let guard = state.begin_start("host").unwrap();
        assert!(state.begin_start("host").is_err());
        assert!(state.enter_guest(|| guest_handle().0).is_err());
        assert!(state.end_session().is_err());
        drop(guard);
        assert!(state.begin_start("host").is_ok());
    }

    #[test]
    fn app_state_failed_start_stays_in_launcher() {
        let state = AppState::new();
        drop(state.begin_start("host").unwrap());
        assert_eq!(lock(&state.mode).kind(), "launcher");
        assert!(state.enter_guest(|| guest_handle().0).is_ok());
    }

    #[test]
    fn app_state_reset_closes_host_channel() {
        let (state, rx) = host_state();
        state.toggle_pet().unwrap();
        assert_eq!(state.end_session(), Ok(Ended::Host));
        assert_eq!(lock(&state.mode).kind(), "launcher");
        assert!(!state.pet_mode.is_on());
        // O `PetMode` segue vivo (hook/keep-alive), mas sem nenhum `Sender` da sessão.
        while let Ok(_cmd) = rx.try_recv() {}
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn app_state_reset_closes_guest_channel() {
        let state = AppState::new();
        let (handle, rx) = guest_handle();
        state.enter_guest(|| handle).unwrap();
        assert_eq!(state.end_session(), Ok(Ended::Guest));
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Disconnected)));
    }

    #[test]
    fn app_state_reset_from_launcher_is_none() {
        assert_eq!(AppState::new().end_session(), Ok(Ended::None));
    }

    #[test]
    fn app_state_toggle_only_in_host() {
        let state = AppState::new();
        assert!(state.toggle_pet().is_err());
        assert!(!state.pet_mode.is_on());

        let (state, _rx) = host_state();
        assert!(state.toggle_pet().is_ok());
        assert!(state.pet_mode.is_on());
    }

    #[test]
    fn app_state_toggle_after_reset_stays_off() {
        let (state, _rx) = host_state();
        state.end_session().unwrap();
        assert!(state.toggle_pet().is_err());
        assert!(!state.pet_mode.is_on());
    }

    #[test]
    fn app_state_second_host_session_rebinds_pet() {
        let (state, rx) = host_state();
        state.end_session().unwrap();
        drop(rx);

        let (tx, rx) = mpsc::channel();
        let handle = Arc::new(HostHandle::from_parts(tx, RecvBacklog::default()));
        let pet_mode = state.bind_pet_sink(handle.clone());
        state.set_mode(Mode::Host { handle, pet_mode });
        state.toggle_pet().unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            HostCommand::HostAutonomous(false)
        ));
    }

    #[test]
    fn app_state_routes_keys_and_net_to_guest() {
        let state = AppState::new();
        let (handle, rx) = guest_handle();
        state.enter_guest(|| handle).unwrap();
        state.key_event(Key::W, true);
        state.net(NetIn::PeerOpened(0));
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuestCommand::Key(Key::W, true)
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuestCommand::Net(NetIn::PeerOpened(0))
        ));
    }

    #[test]
    fn app_state_guest_choose_needs_guest_mode() {
        let state = AppState::new();
        assert!(state.guest_choose("Ana".into(), Animal::Duck).is_err());
        let (handle, rx) = guest_handle();
        state.enter_guest(|| handle).unwrap();
        state.guest_choose("Ana".into(), Animal::Duck).unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            GuestCommand::Choose { .. }
        ));
    }

    #[test]
    fn app_state_net_without_session_is_dropped() {
        AppState::new().net(NetIn::PeerOpened(0));
    }

    #[test]
    fn app_state_parse_recv_validates_channel_and_size() {
        assert_eq!(
            parse_recv(3, "input", vec![1]),
            Ok(NetIn::Recv {
                peer: 3,
                channel: Channel::Input,
                bytes: vec![1],
            })
        );
        assert_eq!(
            parse_recv(3, "lixo", vec![1]),
            Err("canal inválido".to_string())
        );
        assert!(parse_recv(3, "input", vec![0; MAX_MESSAGE_BYTES + 1]).is_err());
    }
}
