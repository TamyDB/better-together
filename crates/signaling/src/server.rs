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
    sync::mpsc::{self, UnboundedSender},
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

type Shared = Arc<Mutex<Hub>>;

/// Estado compartilhado entre as conexões.
#[derive(Default)]
struct Hub {
    registry: Registry,
    limiter: RateLimiter,
    conns: HashMap<ConnId, UnboundedSender<ServerMsg>>,
    next_id: u64,
}

impl Hub {
    fn register(&mut self, tx: UnboundedSender<ServerMsg>) -> ConnId {
        self.next_id += 1;
        let id = ConnId(self.next_id);
        self.conns.insert(id, tx);
        id
    }

    /// Envio síncrono; se a conexão já saiu, a mensagem é descartada.
    fn send(&self, to: ConnId, msg: ServerMsg) {
        if let Some(tx) = self.conns.get(&to) {
            let _ = tx.send(msg);
        }
    }

    fn error(&self, to: ConnId, reason: SignalError) {
        self.send(to, ServerMsg::Error { reason });
    }

    fn handle_text(&mut self, id: ConnId, ip: IpAddr, text: &str) {
        let Ok(msg) = serde_json::from_str::<ClientMsg>(text) else {
            self.error(id, SignalError::BadMessage);
            return;
        };
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

    /// Caminho único de saída: fechamento, erro de socket ou keep-alive vencido.
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

/// Envia um frame; falha ou demora acima de [`IDLE_TIMEOUT`] encerram a conexão.
async fn send_frame(sink: &mut SplitSink<WebSocket, Message>, frame: Message) -> bool {
    matches!(
        tokio::time::timeout(IDLE_TIMEOUT, sink.send(frame)).await,
        Ok(Ok(()))
    )
}

async fn handle_socket(socket: WebSocket, ip: IpAddr, hub: Shared) {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = lock(&hub).register(tx);
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
                    Message::Text(text) => lock(&hub).handle_text(id, ip, text.as_str()),
                    Message::Binary(_) => lock(&hub).error(id, SignalError::BadMessage),
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) => {}
                }
            }
            outgoing = rx.recv() => {
                let Some(msg) = outgoing else { break };
                let Ok(json) = serde_json::to_string(&msg) else { continue };
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

    lock(&hub).disconnect(id);
    log::debug!("conexão {} encerrada", id.0);
}
