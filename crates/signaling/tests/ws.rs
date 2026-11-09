//! Testes de integração do servidor WebSocket de sinalização.

use std::{net::SocketAddr, time::Duration};

use dm_signaling::{
    proto::{ClientMsg, ServerMsg, SignalError},
    ConnId,
};
use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{tungstenite::Message, MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn start() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(dm_signaling::server::serve(listener));
    addr
}

const TIMEOUT: Duration = Duration::from_secs(2);

async fn connect(addr: SocketAddr) -> Ws {
    let (ws, _) = tokio::time::timeout(
        TIMEOUT,
        tokio_tungstenite::connect_async(format!("ws://{addr}/ws")),
    )
    .await
    .expect("timeout conectando")
    .unwrap();
    ws
}

async fn send_raw(ws: &mut Ws, text: &str) {
    tokio::time::timeout(TIMEOUT, ws.send(Message::text(text)))
        .await
        .expect("timeout enviando")
        .unwrap();
}

async fn close(ws: &mut Ws) {
    tokio::time::timeout(TIMEOUT, ws.close(None))
        .await
        .expect("timeout fechando")
        .unwrap();
}

async fn send(ws: &mut Ws, msg: &ClientMsg) {
    send_raw(ws, &serde_json::to_string(msg).unwrap()).await;
}

/// Próxima mensagem de texto do servidor (ignora ping/pong).
async fn next_json(ws: &mut Ws) -> ServerMsg {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return serde_json::from_str(t.as_str()).unwrap(),
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            other => panic!("esperava texto, veio {other:?}"),
        }
    }
}

/// Próxima mensagem de texto do servidor, com timeout de 2 s.
async fn recv_json(ws: &mut Ws) -> ServerMsg {
    tokio::time::timeout(TIMEOUT, next_json(ws))
        .await
        .expect("timeout esperando mensagem do servidor")
}

/// Confere que nenhuma mensagem chega em `wait`.
async fn assert_silent(ws: &mut Ws, wait: Duration) {
    if let Ok(msg) = tokio::time::timeout(wait, next_json(ws)).await {
        panic!("não esperava mensagem, veio {msg:?}");
    }
}

/// Cria uma sala e devolve (código, id do Host).
async fn create_room(ws: &mut Ws) -> (String, ConnId) {
    send(ws, &ClientMsg::CreateRoom).await;
    match recv_json(ws).await {
        ServerMsg::RoomCreated { code, peer_id } => (code, peer_id),
        other => panic!("esperava room_created, veio {other:?}"),
    }
}

/// Entra na sala e devolve o id do convidado (confere o `host_id`).
async fn join_room(ws: &mut Ws, code: &str, host: ConnId) -> ConnId {
    send(ws, &ClientMsg::JoinRoom { code: code.into() }).await;
    match recv_json(ws).await {
        ServerMsg::Joined {
            code: joined,
            peer_id,
            host_id,
        } => {
            assert_eq!(joined, code);
            assert_eq!(host_id, host);
            peer_id
        }
        other => panic!("esperava joined, veio {other:?}"),
    }
}

#[tokio::test]
async fn full_flow() {
    let addr = start().await;
    let mut host = connect(addr).await;
    let (code, host_id) = create_room(&mut host).await;

    let mut guest = connect(addr).await;
    let guest_id = join_room(&mut guest, &code, host_id).await;
    assert_ne!(guest_id, host_id);
    assert_eq!(
        recv_json(&mut host).await,
        ServerMsg::PeerJoined { peer_id: guest_id }
    );

    let data = serde_json::json!({ "sdp": { "type": "offer", "sdp": "v=0\r\n" } });
    send(
        &mut guest,
        &ClientMsg::Signal {
            to: host_id,
            data: data.clone(),
        },
    )
    .await;
    assert_eq!(
        recv_json(&mut host).await,
        ServerMsg::Signal {
            from: guest_id,
            data
        }
    );

    close(&mut guest).await;
    assert_eq!(
        recv_json(&mut host).await,
        ServerMsg::PeerLeft { peer_id: guest_id }
    );
    // Exatamente uma vez.
    assert_silent(&mut host, Duration::from_millis(200)).await;
}

#[tokio::test]
async fn host_leaving_notifies_guests() {
    let addr = start().await;
    let mut host = connect(addr).await;
    let (code, host_id) = create_room(&mut host).await;
    let mut guest = connect(addr).await;
    join_room(&mut guest, &code, host_id).await;

    close(&mut host).await;
    assert_eq!(recv_json(&mut guest).await, ServerMsg::HostLeft);
}

#[tokio::test]
async fn bad_json_and_bad_target() {
    let addr = start().await;
    let mut host = connect(addr).await;
    send_raw(&mut host, "oi").await;
    assert_eq!(
        recv_json(&mut host).await,
        ServerMsg::Error {
            reason: SignalError::BadMessage
        }
    );

    let (_, host_id) = create_room(&mut host).await;
    // Um id que não está na sala (nem existe).
    let outsider = ConnId(host_id.0 + 1000);
    send(
        &mut host,
        &ClientMsg::Signal {
            to: outsider,
            data: serde_json::json!({}),
        },
    )
    .await;
    assert_eq!(
        recv_json(&mut host).await,
        ServerMsg::Error {
            reason: SignalError::BadTarget
        }
    );
}

#[tokio::test]
async fn join_rate_limited() {
    let addr = start().await;
    let mut ws = connect(addr).await;
    for i in 0..11 {
        send(
            &mut ws,
            &ClientMsg::JoinRoom {
                code: "PATO-0000".into(),
            },
        )
        .await;
        let expected = if i < 10 {
            SignalError::RoomNotFound
        } else {
            SignalError::RateLimited
        };
        assert_eq!(
            recv_json(&mut ws).await,
            ServerMsg::Error { reason: expected },
            "tentativa {}",
            i + 1
        );
    }
}

#[tokio::test]
async fn create_rate_limited_after_joins() {
    let addr = start().await;
    let mut ws = connect(addr).await;
    for _ in 0..10 {
        send(
            &mut ws,
            &ClientMsg::JoinRoom {
                code: "PATO-0000".into(),
            },
        )
        .await;
        assert_eq!(
            recv_json(&mut ws).await,
            ServerMsg::Error {
                reason: SignalError::RoomNotFound
            }
        );
    }
    send(&mut ws, &ClientMsg::CreateRoom).await;
    assert_eq!(
        recv_json(&mut ws).await,
        ServerMsg::Error {
            reason: SignalError::RateLimited
        }
    );
}
