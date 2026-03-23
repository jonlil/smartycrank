use serde::Deserialize;
use std::path::PathBuf;

const SERVICE: &str = "smartycrank";

#[derive(Deserialize)]
struct FileConfig {
    tv: TvFileConfig,
    spotify: Option<SpotifyFileConfig>,
}

#[derive(Deserialize)]
struct TvFileConfig {
    host: String,
}

#[derive(Deserialize)]
struct SpotifyFileConfig {
    tv_device_name: String,
}

pub struct TvConfig {
    pub host: String,
    pub token: String,
}

pub struct SpotifyConfig {
    pub refresh_token: String,
    pub tv_device_name: String,
}

fn get_secret(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    let entry = keyring::Entry::new(SERVICE, key)?;
    entry.get_password().map_err(|e| format!("keyring lookup failed for '{key}': {e}").into())
}

pub fn store_secret(key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let entry = keyring::Entry::new(SERVICE, key)?;
    entry.set_password(value)?;
    Ok(())
}

fn load_file() -> Result<FileConfig, Box<dyn std::error::Error>> {
    let path = config_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|_| format!("Config not found at {}", path.display()))?;
    Ok(toml::from_str(&content)?)
}

pub fn load_tv() -> Result<TvConfig, Box<dyn std::error::Error>> {
    let file = load_file()?;
    let token = get_secret("tv-token")?;
    Ok(TvConfig {
        host: file.tv.host,
        token,
    })
}

pub fn load_spotify() -> Result<SpotifyConfig, Box<dyn std::error::Error>> {
    let file = load_file()?;
    let sp = file.spotify.ok_or("Missing [spotify] section in config")?;
    let refresh_token = get_secret("spotify-refresh-token")?;
    Ok(SpotifyConfig {
        refresh_token,
        tv_device_name: sp.tv_device_name,
    })
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("smartycrank")
        .join("config.toml")
}
