//! Servidor WebSocket de sinalização (axum): salas e troca de SDP/ICE entre Host e convidados.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::Instant,
};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    response::Response,
    routing::get,
    Router,
};
use futures_util::{stream::SplitSink, SinkExt, StreamExt};
use tokio::{
    net::TcpListener,
    sync::mpsc::{self, error::TrySendError},
    time::Duration,
};

use crate::{
    limiter::RateLimiter,
    proto::{ClientMsg, ServerMsg, SignalError},
    registry::{Leave, Registry},
    ConnId,
};

/// Tamanho máximo de uma mensagem (e de um frame) vinda do cliente.
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024;
/// Intervalo entre os Pings WS enviados pelo servidor.
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
/// Sem nenhum frame do cliente nesse tempo, a conexão é dada como morta.
/// Também é o limite para um envio ao cliente sair.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(45);
/// Intervalo da limpeza dos baldes do rate limit.
pub const PRUNE_INTERVAL: Duration = Duration::from_secs(60);
/// Mensagens pendentes por conexão; fila cheia derruba o destino.
pub const QUEUE_CAPACITY: usize = 32;

type Shared = Arc<Mutex<Hub>>;

/// Estado compartilhado entre as conexões.
#[derive(Default)]
struct Hub {
    registry: Registry,
    limiter: RateLimiter,
    /// Fila de saída de cada conexão, já serializada em JSON.
    conns: HashMap<ConnId, mpsc::Sender<String>>,
    next_id: u64,
}

impl Hub {
    fn register(&mut self, tx: mpsc::Sender<String>) -> ConnId {
        self.next_id += 1;
        let id = ConnId(self.next_id);
        self.conns.insert(id, tx);
        id
    }

    /// Envio síncrono; se a conexão já saiu, a mensagem é descartada.
    /// Fila cheia: o destino não está lendo e é derrubado. Sem o `Sender`, o `recv` da task dele
    /// devolve `None` e ela sai pelo mesmo caminho de [`Hub::disconnect`].
    fn send(&mut self, to: ConnId, msg: ServerMsg) {
        let Some(tx) = self.conns.get(&to) else {
            return;
        };
        let Ok(json) = serde_json::to_string(&msg) else {
            return;
        };
        if let Err(TrySendError::Full(_)) = tx.try_send(json) {
            log::debug!("conexão {} não lê a fila; derrubando", to.0);
            self.conns.remove(&to);
        }
    }

    fn error(&mut self, to: ConnId, reason: SignalError) {
        self.send(to, ServerMsg::Error { reason });
    }

    fn handle_msg(&mut self, id: ConnId, ip: IpAddr, msg: ClientMsg) {
        match msg {
            ClientMsg::CreateRoom => {
                if !self.limiter.allow(ip, Instant::now()) {
                    self.error(id, SignalError::RateLimited);
                    return;
                }
                match self.registry.create(id, &mut rand::rng()) {
                    Ok(code) => self.send(id, ServerMsg::RoomCreated { code, peer_id: id }),
                    Err(reason) => self.error(id, reason),
                }
            }
            ClientMsg::JoinRoom { code } => {
                if !self.limiter.allow(ip, Instant::now()) {
                    self.error(id, SignalError::RateLimited);
                    return;
                }
                match self.registry.join(id, &code) {
                    Ok((code, host_id)) => {
                        self.send(
                            id,
                            ServerMsg::Joined {
                                code,
                                peer_id: id,
                                host_id,
                            },
                        );
                        self.send(host_id, ServerMsg::PeerJoined { peer_id: id });
                    }
                    Err(reason) => self.error(id, reason),
                }
            }
            ClientMsg::Signal { to, data } => {
                if self.registry.can_signal(id, to) {
                    self.send(to, ServerMsg::Signal { from: id, data });
                } else {
                    self.error(id, SignalError::BadTarget);
                }
            }
        }
    }

    /// Caminho único de saída: fechamento, erro de socket, keep-alive vencido, fila cheia ou
    /// pânico na task. Idempotente.
    fn disconnect(&mut self, id: ConnId) {
        self.conns.remove(&id);
        match self.registry.leave(id) {
            Leave::NotInRoom => {}
            Leave::Guest { host } => self.send(host, ServerMsg::PeerLeft { peer_id: id }),
            Leave::Host { guests } => {
                for g in guests {
                    self.send(g, ServerMsg::HostLeft);
                }
            }
        }
    }
}

/// O lock nunca atravessa um `.await`; um pânico sob o lock não derruba o servidor.
fn lock(hub: &Mutex<Hub>) -> MutexGuard<'_, Hub> {
    hub.lock().unwrap_or_else(PoisonError::into_inner)
}

fn build() -> (Router, Shared) {
    let hub = Shared::default();
    let router = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(hub.clone());
    (router, hub)
}

/// Rota `GET /ws`. Precisa de `ConnectInfo<SocketAddr>` (ver [`serve`]).
pub fn router() -> Router {
    build().0
}

pub async fn serve(listener: TcpListener) -> std::io::Result<()> {
    let (router, hub) = build();
    tokio::spawn(prune_loop(Arc::downgrade(&hub)));
    drop(hub);
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
}

/// Limpa os baldes cheios do limiter; termina quando o servidor some.
async fn prune_loop(hub: Weak<Mutex<Hub>>) {
    let start = tokio::time::Instant::now() + PRUNE_INTERVAL;
    let mut tick = tokio::time::interval_at(start, PRUNE_INTERVAL);
    loop {
        tick.tick().await;
        let Some(hub) = hub.upgrade() else { break };
        lock(&hub).limiter.prune(Instant::now());
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(hub): State<Shared>,
) -> Response {
    ws.max_message_size(MAX_MESSAGE_SIZE)
        .max_frame_size(MAX_MESSAGE_SIZE)
        .on_upgrade(move |socket| handle_socket(socket, addr.ip(), hub))
}

/// Garante o [`Hub::disconnect`] uma única vez, mesmo se a task da conexão entrar em pânico.
struct ConnGuard {
    hub: Shared,
    id: ConnId,
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        lock(&self.hub).disconnect(self.id);
        log::debug!("conexão {} encerrada", self.id.0);
    }
}

/// Envia um frame; falha ou demora acima de [`IDLE_TIMEOUT`] encerram a conexão.
async fn send_frame(sink: &mut SplitSink<WebSocket, Message>, frame: Message) -> bool {
    matches!(
        tokio::time::timeout(IDLE_TIMEOUT, sink.send(frame)).await,
        Ok(Ok(()))
    )
}

async fn handle_socket(socket: WebSocket, ip: IpAddr, hub: Shared) {
    let (tx, mut rx) = mpsc::channel(QUEUE_CAPACITY);
    let id = lock(&hub).register(tx);
    let guard = ConnGuard {
        hub: hub.clone(),
        id,
    };
    log::debug!("conexão {} de {ip}", id.0);
    let (mut sink, mut stream) = socket.split();

    let mut ping =
        tokio::time::interval_at(tokio::time::Instant::now() + PING_INTERVAL, PING_INTERVAL);
    let idle = tokio::time::sleep(IDLE_TIMEOUT);
    tokio::pin!(idle);

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(frame)) = incoming else { break };
                idle.as_mut().reset(tokio::time::Instant::now() + IDLE_TIMEOUT);
                match frame {
                    // O parse (até 64 KiB) fica fora do lock global.
                    Message::Text(text) => match serde_json::from_str::<ClientMsg>(&text) {
                        Ok(msg) => lock(&hub).handle_msg(id, ip, msg),
                        Err(_) => lock(&hub).error(id, SignalError::BadMessage),
                    },
                    Message::Binary(_) => lock(&hub).error(id, SignalError::BadMessage),
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            outgoing = rx.recv() => {
                let Some(json) = outgoing else { break };
                if !send_frame(&mut sink, Message::Text(json.into())).await {
                    break;
                }
            }
            _ = ping.tick() => {
                if !send_frame(&mut sink, Message::Ping(Bytes::new())).await {
                    break;
                }
            }
            _ = &mut idle => {
                log::debug!("conexão {} sem resposta; fechando", id.0);
                break;
            }
        }
    }

    drop(guard);
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const IP: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

    type Rx = mpsc::Receiver<String>;

    fn conn(hub: &mut Hub) -> (ConnId, Rx) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        (hub.register(tx), rx)
    }

    fn next(rx: &mut Rx) -> Option<ServerMsg> {
        rx.try_recv()
            .ok()
            .map(|json| serde_json::from_str(&json).unwrap())
    }

    fn closed(rx: &mut Rx) -> bool {
        matches!(rx.try_recv(), Err(mpsc::error::TryRecvError::Disconnected))
    }

    fn req(hub: &mut Hub, id: ConnId, msg: &ClientMsg) {
        hub.handle_msg(id, IP, msg.clone());
    }

    /// Sala com Host e um convidado; as filas já foram lidas até aqui.
    fn room(hub: &mut Hub) -> [(ConnId, Rx); 2] {
        let (host, mut host_rx) = conn(hub);
        let (guest, mut guest_rx) = conn(hub);
        req(hub, host, &ClientMsg::CreateRoom);
        let Some(ServerMsg::RoomCreated { code, .. }) = next(&mut host_rx) else {
            panic!("esperava room_created");
        };
        req(hub, guest, &ClientMsg::JoinRoom { code });
        assert!(matches!(
            next(&mut guest_rx),
            Some(ServerMsg::Joined { .. })
        ));
        assert_eq!(
            next(&mut host_rx),
            Some(ServerMsg::PeerJoined { peer_id: guest })
        );
        [(host, host_rx), (guest, guest_rx)]
    }

    #[test]
    fn flood_drops_target_that_does_not_read() {
        let mut hub = Hub::default();
        let [(host, mut host_rx), (guest, mut guest_rx)] = room(&mut hub);
        // O Host para de ler; o convidado inunda.
        for i in 0..(QUEUE_CAPACITY + 8) {
            let data = serde_json::json!({ "n": i });
            req(&mut hub, guest, &ClientMsg::Signal { to: host, data });
        }
        assert!(!hub.conns.contains_key(&host), "Host deveria ser derrubado");
        // A fila do Host esvazia o que já tinha e fecha.
        let mut queued = 0;
        while next(&mut host_rx).is_some() {
            queued += 1;
        }
        assert_eq!(queued, QUEUE_CAPACITY);
        assert!(closed(&mut host_rx));
        // O convidado não recebeu nada até a task do Host encerrar.
        assert_eq!(next(&mut guest_rx), None);
        hub.disconnect(host);
        assert_eq!(next(&mut guest_rx), Some(ServerMsg::HostLeft));
        // E segue funcionando.
        req(&mut hub, guest, &ClientMsg::CreateRoom);
        assert!(matches!(
            next(&mut guest_rx),
            Some(ServerMsg::RoomCreated { .. })
        ));
    }

    #[test]
    fn disconnect_is_idempotent() {
        let mut hub = Hub::default();
        let [(_, mut host_rx), (guest, _)] = room(&mut hub);
        hub.disconnect(guest);
        hub.disconnect(guest);
        assert_eq!(
            next(&mut host_rx),
            Some(ServerMsg::PeerLeft { peer_id: guest })
        );
        assert_eq!(next(&mut host_rx), None);
    }
}
