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

    pub async fn pair(host: &str) -> Result<String, Box<dyn std::error::Error>> {
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/samsung.remote.control?name={}",
            host, name
        );

        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        let connector = tokio_tungstenite::Connector::NativeTls(connector);

        eprintln!("Connecting to TV at {}...", host);
        eprintln!("Please accept the connection on your TV when prompted.");

        let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(
            &url, None, false, Some(connector),
        )
        .await?;

        // The TV sends a response containing the token
        let msg = ws.next().await
            .ok_or("No response from TV")??;

        let response: serde_json::Value = serde_json::from_str(
            msg.to_text().map_err(|_| "Non-text response from TV")?
        )?;

        let token = response
            .get("data")
            .and_then(|d| d.get("token"))
            .and_then(|t| t.as_str())
            .ok_or("TV response did not contain a token")?;

        Ok(token.to_string())
    }

    pub async fn is_on(&self) -> bool {
        let url = format!("http://{}:8001/api/v2/", self.host);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        client.get(&url).send().await.is_ok()
    }

    pub async fn ensure_app_running(&self, app_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("http://{}:8001/api/v2/applications/{}", self.host, app_id);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()?;

        // Check if app is running and visible
        let resp = client.get(&url).send().await?;
        let info: serde_json::Value = resp.json().await?;
        let running = info.get("running").and_then(|v| v.as_bool()) == Some(true);
        let visible = info.get("visible").and_then(|v| v.as_bool()) == Some(true);
        if running && visible {
            return Ok(());
        }

        // Launch (or foreground) the app
        client.post(&url).send().await?;

        // Wait for it to be running + visible (poll up to 10s)
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Ok(resp) = client.get(&url).send().await {
                if let Ok(info) = resp.json::<serde_json::Value>().await {
                    let r = info.get("running").and_then(|v| v.as_bool()) == Some(true);
                    let v = info.get("visible").and_then(|v| v.as_bool()) == Some(true);
                    if r && v {
                        // Give the app time to set up WebSocket channel
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        return Ok(());
                    }
                }
            }
        }

        Err("App did not start within 10 seconds".into())
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

    pub async fn send_to_channel(&self, channel: &str, event: &str, data: serde_json::Value) -> Result<(), Box<dyn std::error::Error>> {
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/{}?name={}&token={}",
            self.host, channel, name, self.token
        );

        let connector = native_tls::TlsConnector::builder()
            .danger_accept_invalid_certs(true)
            .build()?;
        let connector = tokio_tungstenite::Connector::NativeTls(connector);

        let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(
            &url, None, false, Some(connector),
        )
        .await?;

        // Wait for connection + ready
        ws.next().await;

        let payload = json!({
            "method": "ms.channel.emit",
            "params": {
                "event": event,
                "to": "host",
                "data": data
            }
        });

        ws.send(Message::Text(payload.to_string().into())).await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(())
    }

    pub async fn launch_app(&self, app_id: &str, deep_link: &str) -> Result<(), Box<dyn std::error::Error>> {
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
            &url, None, false, Some(connector),
        )
        .await?;

        ws.next().await;

        let payload = json!({
            "method": "ms.channel.emit",
            "params": {
                "event": "ed.apps.launch",
                "to": "host",
                "data": {
                    "appId": app_id,
                    "action_type": "DEEP_LINK",
                    "metaTag": deep_link
                }
            }
        });

        ws.send(Message::Text(payload.to_string().into())).await?;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        Ok(())
    }
}
