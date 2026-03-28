use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;

use crate::config::{self, SpotifyConfig};

const CLIENT_ID: &str = "40c148a5aa614c38b6032a73ba2f030f";
const REDIRECT_URI: &str = "http://127.0.0.1:8913/callback";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const PLAYER_URL: &str = "https://api.spotify.com/v1/me/player";
const SCOPES: &str = "user-read-playback-state user-modify-playback-state";
const TOKEN_MARGIN_SECS: u64 = 300; // refresh 5 min before expiry

#[derive(Serialize, Deserialize)]
struct CachedToken {
    access_token: String,
    expires_at: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct PlayerState {
    device: Device,
}

#[derive(Deserialize)]
struct Device {
    name: String,
}

fn generate_code_verifier() -> String {
    let mut bytes = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

pub struct Spotify {
    client: Client,
    refresh_token: String,
    tv_device_name: String,
}

impl Spotify {
    pub fn new(config: &SpotifyConfig) -> Self {
        Self {
            client: Client::new(),
            refresh_token: config.refresh_token.clone(),
            tv_device_name: config.tv_device_name.clone(),
        }
    }

    fn cache_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("smartycrank_spotify_token.json")
    }

    fn load_cached_token() -> Option<String> {
        let data = std::fs::read_to_string(Self::cache_path()).ok()?;
        let cached: CachedToken = serde_json::from_str(&data).ok()?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        if now < cached.expires_at - TOKEN_MARGIN_SECS {
            Some(cached.access_token)
        } else {
            None
        }
    }

    fn save_cached_token(access_token: &str, expires_in: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cached = CachedToken {
            access_token: access_token.to_string(),
            expires_at: now + expires_in,
        };
        let _ = std::fs::write(Self::cache_path(), serde_json::to_string(&cached).unwrap_or_default());
    }

    async fn get_access_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(token) = Self::load_cached_token() {
            return Ok(token);
        }

        let resp = self
            .client
            .post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", self.refresh_token.as_str()),
                ("client_id", CLIENT_ID),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Spotify token error: {text}").into());
        }

        let token: TokenResponse = resp.json().await?;

        if let Some(new_refresh) = &token.refresh_token {
            let _ = config::store_secret("spotify-refresh-token", new_refresh);
        }

        let expires_in = token.expires_in.unwrap_or(3600);
        Self::save_cached_token(&token.access_token, expires_in);

        Ok(token.access_token)
    }

    pub async fn transfer_to_tv(&self) -> Result<(), Box<dyn std::error::Error>> {
        let token = self.get_access_token().await?;

        // Get available devices
        let resp = self.client
            .get(format!("{}/devices", PLAYER_URL))
            .bearer_auth(&token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Spotify API error: {text}").into());
        }

        let body: serde_json::Value = resp.json().await?;
        let devices = body["devices"].as_array()
            .ok_or("No devices array in response")?;

        let tv_device = devices.iter()
            .find(|d| d["name"].as_str() == Some(&self.tv_device_name))
            .ok_or_else(|| format!("TV '{}' not found among Spotify devices", self.tv_device_name))?;

        let device_id = tv_device["id"].as_str()
            .ok_or("Device has no ID")?;

        // Transfer playback
        let resp = self.client
            .put(PLAYER_URL)
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "device_ids": [device_id],
                "play": true
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("Failed to transfer playback: {text}").into());
        }

        eprintln!("Playback transferred to {}", self.tv_device_name);
        Ok(())
    }

    pub async fn is_playing_on_tv(&self) -> Result<bool, Box<dyn std::error::Error>> {
        let token = self.get_access_token().await?;
        let resp = self
            .client
            .get(PLAYER_URL)
            .bearer_auth(&token)
            .send()
            .await?;

        if resp.status().as_u16() == 204 {
            return Ok(false);
        }
        if !resp.status().is_success() {
            return Ok(false);
        }

        let state: PlayerState = resp.json().await?;
        Ok(state.device.name == self.tv_device_name)
    }
}

pub async fn auth() -> Result<(), Box<dyn std::error::Error>> {
    let verifier = generate_code_verifier();
    let challenge = code_challenge(&verifier);

    let url = format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge_method=S256&code_challenge={}",
        CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPES),
        challenge,
    );

    println!("Opening browser for Spotify authorization...");
    open::that(&url)?;

    let listener = TcpListener::bind("127.0.0.1:8913")?;
    println!("Waiting for callback on {REDIRECT_URI}");

    let (mut stream, _) = listener.accept()?;
    let reader = BufReader::new(&stream);
    let request_line = reader.lines().next().ok_or("No request received")??;

    let code = request_line
        .split_whitespace()
        .nth(1)
        .and_then(|path| path.split("code=").nth(1))
        .and_then(|c| c.split('&').next())
        .ok_or("No authorization code in callback")?
        .to_string();

    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h2>smartycrank authorized!</h2><p>You can close this tab.</p></body></html>";
    stream.write_all(response.as_bytes())?;
    drop(stream);

    println!("Exchanging code for tokens...");

    let client = Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", REDIRECT_URI),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {text}").into());
    }

    let token: TokenResponse = resp.json().await?;

    if let Some(refresh) = &token.refresh_token {
        config::store_secret("spotify-refresh-token", refresh)?;
        println!("Refresh token stored in keyring");
    } else {
        return Err("No refresh token in response".into());
    }

    println!("Spotify auth complete!");
    Ok(())
}
