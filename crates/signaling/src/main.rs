//! Binário do servidor de sinalização. Endereço em `BT_SIGNALING_ADDR` (padrão `127.0.0.1:8787`).

use tokio::net::TcpListener;

const DEFAULT_ADDR: &str = "127.0.0.1:8787";

#[tokio::main]
async fn main() -> std::io::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let addr = std::env::var("BT_SIGNALING_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
    let listener = TcpListener::bind(&addr).await?;
    log::info!("ouvindo em {}", listener.local_addr()?);
    dm_signaling::server::serve(listener).await
}
