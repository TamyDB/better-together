//! Configuração de rede que o WebView usa para sinalização e ICE.

use serde::Serialize;

const DEFAULT_SIGNALING_URL: &str = "ws://127.0.0.1:8787/ws";
const DEFAULT_STUN_URL: &str = "stun:stun.l.google.com:19302";

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct IceServer {
    pub urls: String,
    // Omitidos quando ausentes: no RTCIceServer, `null` viraria a string "null".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct NetConfig {
    pub signaling_url: String,
    pub ice_servers: Vec<IceServer>,
}

impl NetConfig {
    /// Lê a configuração de `get`; variável vazia vale como ausente.
    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> NetConfig {
        let var = |name: &str| get(name).filter(|v| !v.is_empty());
        let mut ice_servers = vec![IceServer {
            urls: DEFAULT_STUN_URL.to_string(),
            username: None,
            credential: None,
        }];
        if let (Some(urls), Some(username), Some(credential)) =
            (var("BT_TURN_URL"), var("BT_TURN_USER"), var("BT_TURN_PASS"))
        {
            ice_servers.push(IceServer {
                urls,
                username: Some(username),
                credential: Some(credential),
            });
        }
        NetConfig {
            signaling_url: var("BT_SIGNALING_URL")
                .unwrap_or_else(|| DEFAULT_SIGNALING_URL.to_string()),
            ice_servers,
        }
    }

    pub fn from_env() -> NetConfig {
        NetConfig::from_vars(|name| std::env::var(name).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn from_pairs(pairs: &[(&str, &str)]) -> NetConfig {
        NetConfig::from_vars(|k| {
            pairs
                .iter()
                .find(|(name, _)| *name == k)
                .map(|(_, v)| (*v).to_string())
        })
    }

    #[test]
    fn net_config_defaults() {
        let cfg = NetConfig::from_vars(|_| None);
        assert_eq!(cfg.signaling_url, "ws://127.0.0.1:8787/ws");
        assert_eq!(cfg.ice_servers.len(), 1);
        assert_eq!(cfg.ice_servers[0].urls, "stun:stun.l.google.com:19302");
        assert_eq!(cfg.ice_servers[0].username, None);
    }

    #[test]
    fn net_config_uses_signaling_var() {
        let cfg = from_pairs(&[("BT_SIGNALING_URL", "wss://sala.example/ws")]);
        assert_eq!(cfg.signaling_url, "wss://sala.example/ws");
    }

    #[test]
    fn net_config_turn_requires_all() {
        let partial = from_pairs(&[("BT_TURN_URL", "turn:t.example:3478")]);
        assert_eq!(partial.ice_servers.len(), 1);

        let full = from_pairs(&[
            ("BT_TURN_URL", "turn:t.example:3478"),
            ("BT_TURN_USER", "pato"),
            ("BT_TURN_PASS", "segredo"),
        ]);
        assert_eq!(full.ice_servers.len(), 2);
        let turn = &full.ice_servers[1];
        assert_eq!(turn.urls, "turn:t.example:3478");
        assert_eq!(turn.username.as_deref(), Some("pato"));
        assert_eq!(turn.credential.as_deref(), Some("segredo"));
    }

    #[test]
    fn net_config_empty_var_counts_as_missing() {
        let cfg = from_pairs(&[
            ("BT_SIGNALING_URL", ""),
            ("BT_TURN_URL", "turn:t.example:3478"),
            ("BT_TURN_USER", ""),
            ("BT_TURN_PASS", "segredo"),
        ]);
        assert_eq!(cfg.signaling_url, DEFAULT_SIGNALING_URL);
        assert_eq!(cfg.ice_servers.len(), 1);
    }

    #[test]
    fn net_config_json_omits_missing_credentials() {
        let json = serde_json::to_value(NetConfig::from_vars(|_| None)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "signaling_url": "ws://127.0.0.1:8787/ws",
                "ice_servers": [{ "urls": "stun:stun.l.google.com:19302" }],
            })
        );
    }
}
