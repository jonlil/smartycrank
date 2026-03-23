use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::config::TvConfig;

const APP_NAME: &str = "smartycrank";

pub struct SamsungTv {
    host: String,
    token: String,
}

impl SamsungTv {
    pub fn new(config: &TvConfig) -> Self {
        Self {
            host: config.host.clone(),
            token: config.token.clone(),
        }
    }

    async fn send_key(&self, key: &str) -> Result<(), Box<dyn std::error::Error>> {
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/samsung.remote.control?name={}&token={}",
            self.host, name, self.token
        );

        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        let connector = tokio_tungstenite::Connector::NativeTls(connector);

        let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(
            &url,
            None,
            false,
            Some(connector),
        )
        .await?;

        // Read the initial connection response
        ws.next().await;

        let payload = json!({
            "method": "ms.remote.control",
            "params": {
                "Cmd": "Click",
                "DataOfCmd": key,
                "Option": "false",
                "TypeOfRemote": "SendRemoteKey"
            }
        });

        ws.send(Message::Text(payload.to_string().into())).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }

    pub async fn volume_up(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_key("KEY_VOLUP").await
    }

    pub async fn volume_down(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_key("KEY_VOLDOWN").await
    }

    pub async fn mute(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_key("KEY_MUTE").await
    }
}
