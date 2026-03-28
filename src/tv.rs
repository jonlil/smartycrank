use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio_tungstenite::tungstenite::Message;

use crate::config::{TvConfig, WakeMethod};

const APP_NAME: &str = "smartycrank";

pub struct SamsungTv {
    host: String,
    token: Option<String>,
    mac: Option<String>,
    bind_addr: Option<String>,
    wake: Option<WakeMethod>,
    smartthings_device_id: Option<String>,
    smartthings_token: Option<String>,
}

impl SamsungTv {
    pub fn new(config: &TvConfig) -> Self {
        Self {
            host: config.host.clone(),
            token: config.token.clone(),
            mac: config.mac.clone(),
            bind_addr: config.bind_addr.clone(),
            wake: config.wake.clone(),
            smartthings_device_id: config.smartthings_device_id.clone(),
            smartthings_token: config.smartthings_token.clone(),
        }
    }

    fn require_token(&self) -> Result<&str, Box<dyn std::error::Error>> {
        self.token.as_deref().ok_or_else(|| {
            "No TV token found. Run 'smartycrank pair' first.".into()
        })
    }

    pub async fn power_on(&self) -> Result<(), Box<dyn std::error::Error>> {
        let method = self.wake.as_ref()
            .or(if self.mac.is_some() { Some(&WakeMethod::Wol) } else { None })
            .ok_or("No wake method configured. Add 'mac' or 'wake = \"smartthings\"' to config.")?;

        match method {
            WakeMethod::Wol => {
                let mac = self.mac.as_deref()
                    .ok_or("wake = \"wol\" requires a MAC address. Run 'smartycrank discover' to find it.")?;
                send_wol(mac, &self.host, self.bind_addr.as_deref())?;
                eprintln!("Wake-on-LAN sent to {}", mac);
            }
            WakeMethod::Smartthings => {
                let device_id = self.smartthings_device_id.as_deref()
                    .ok_or("wake = \"smartthings\" requires smartthings_device_id in config")?;
                let token = self.smartthings_token.as_deref()
                    .ok_or("No SmartThings token. Run: smartycrank store-secret smartthings-token <PAT>")?;
                smartthings_command(token, device_id, "on").await?;
                eprintln!("Power on sent via SmartThings");
            }
        }
        Ok(())
    }

    pub async fn power_off(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.send_key("KEY_POWER").await
    }

    pub fn discover(host: &str) -> Result<String, Box<dyn std::error::Error>> {
        // Ping the host to ensure an ARP entry exists
        let _ = std::process::Command::new("ping")
            .args(["-c", "1", "-W", "2", host])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        let arp_table = std::fs::read_to_string("/proc/net/arp")
            .map_err(|_| "Could not read /proc/net/arp")?;

        for line in arp_table.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() >= 4 && fields[0] == host {
                let mac = fields[3];
                if mac != "00:00:00:00:00:00" {
                    return Ok(mac.to_uppercase());
                }
            }
        }

        Err(format!("Could not find MAC address for {}. Is the TV on and reachable?", host).into())
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
        let token = self.require_token()?;
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/samsung.remote.control?name={}&token={}",
            self.host, name, token
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
        let token = self.require_token()?;
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/{}?name={}&token={}",
            self.host, channel, name, token
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
        let token = self.require_token()?;
        let name = STANDARD.encode(APP_NAME);
        let url = format!(
            "wss://{}:8002/api/v2/channels/samsung.remote.control?name={}&token={}",
            self.host, name, token
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

fn resolve_local_addr(target_host: &str) -> Result<std::net::IpAddr, Box<dyn std::error::Error>> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.connect(format!("{}:9", target_host))?;
    Ok(socket.local_addr()?.ip())
}

fn send_wol(mac_str: &str, host: &str, bind_addr: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let mac_bytes: Vec<u8> = mac_str
        .split(':')
        .map(|b| u8::from_str_radix(b, 16))
        .collect::<Result<Vec<_>, _>>()?;
    if mac_bytes.len() != 6 {
        return Err("Invalid MAC address, expected format AA:BB:CC:DD:EE:FF".into());
    }
    let mut packet = vec![0xFFu8; 6];
    for _ in 0..16 {
        packet.extend_from_slice(&mac_bytes);
    }

    let local_addr = match bind_addr {
        Some(addr) => addr.parse()?,
        None => resolve_local_addr(host)?,
    };

    let socket = std::net::UdpSocket::bind(std::net::SocketAddr::new(local_addr, 0))?;
    socket.set_broadcast(true)?;
    socket.send_to(&packet, "255.255.255.255:9")?;
    Ok(())
}

async fn smartthings_command(
    token: &str,
    device_id: &str,
    command: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!(
        "https://api.smartthings.com/v1/devices/{}/commands",
        device_id
    );
    let body = json!({
        "commands": [{
            "component": "main",
            "capability": "switch",
            "command": command
        }]
    });
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("SmartThings API error {}: {}", status, text).into());
    }
    Ok(())
}
