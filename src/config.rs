use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

const SERVICE: &str = "smartycrank";

#[derive(Deserialize)]
struct FileConfig {
    /// Legacy single-TV config
    tv: Option<TvFileConfig>,
    /// Named TV profiles
    tvs: Option<HashMap<String, TvFileConfig>>,
    default: Option<DefaultConfig>,
    spotify: Option<SpotifyFileConfig>,
}

#[derive(Deserialize)]
struct DefaultConfig {
    tv: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub enum WakeMethod {
    Wol,
    Smartthings,
}

#[derive(Deserialize, Clone)]
struct TvFileConfig {
    host: String,
    mac: Option<String>,
    bind_addr: Option<String>,
    wake: Option<WakeMethod>,
    smartthings_device_id: Option<String>,
}

#[derive(Deserialize)]
struct SpotifyFileConfig {
    tv_device_name: String,
    tv_app_id: Option<String>,
}

pub struct TvConfig {
    pub host: String,
    pub token: Option<String>,
    pub mac: Option<String>,
    pub bind_addr: Option<String>,
    pub wake: Option<WakeMethod>,
    pub smartthings_device_id: Option<String>,
    pub smartthings_token: Option<String>,
}

pub struct SpotifyConfig {
    pub refresh_token: String,
    pub tv_device_name: String,
    pub tv_app_id: String,
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

pub fn load_tv_host(tv_arg: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    let file = load_file()?;
    let (tv_file, _) = resolve_tv_file_config(&file, tv_arg)?;
    Ok(tv_file.host)
}

/// Returns (host, Option<profile_name>) for pairing
pub fn load_tv_profile(tv_arg: Option<&str>) -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    let file = load_file()?;
    let (tv_file, profile_name) = resolve_tv_file_config(&file, tv_arg)?;
    Ok((tv_file.host, profile_name))
}

pub fn load_tv(tv_arg: Option<&str>) -> Result<TvConfig, Box<dyn std::error::Error>> {
    let file = load_file()?;
    let (tv_file, profile_name) = resolve_tv_file_config(&file, tv_arg)?;

    // Try per-TV token first, fall back to legacy "tv-token" key
    let token = if let Some(ref name) = profile_name {
        get_secret(&format!("tv-token:{}", name))
            .or_else(|_| get_secret("tv-token"))
            .ok()
    } else {
        get_secret("tv-token").ok()
    };

    let smartthings_token = if tv_file.smartthings_device_id.is_some() {
        get_secret("smartthings-token").ok()
    } else {
        None
    };

    Ok(TvConfig {
        host: tv_file.host,
        token,
        mac: tv_file.mac,
        bind_addr: tv_file.bind_addr,
        wake: tv_file.wake,
        smartthings_device_id: tv_file.smartthings_device_id,
        smartthings_token,
    })
}

/// Returns (TvFileConfig, Option<profile_name>)
fn resolve_tv_file_config(file: &FileConfig, tv_arg: Option<&str>) -> Result<(TvFileConfig, Option<String>), Box<dyn std::error::Error>> {
    let tvs = file.tvs.as_ref();

    // --tv flag provided: look up as profile name first, then treat as raw IP/host
    if let Some(arg) = tv_arg {
        if let Some(tvs) = tvs {
            if let Some(tv) = tvs.get(arg) {
                return Ok((tv.clone(), Some(arg.to_string())));
            }
        }
        return Ok((TvFileConfig { host: arg.to_string(), mac: None, bind_addr: None, wake: None, smartthings_device_id: None }, None));
    }

    // No --tv flag: check default.tv, then fall back to legacy [tv] section
    if let Some(default_name) = file.default.as_ref().and_then(|d| d.tv.as_deref()) {
        if let Some(tvs) = tvs {
            if let Some(tv) = tvs.get(default_name) {
                return Ok((tv.clone(), Some(default_name.to_string())));
            }
            return Err(format!("default tv '{}' not found in [tvs]", default_name).into());
        }
    }

    // Legacy [tv] section
    if let Some(tv) = &file.tv {
        return Ok((tv.clone(), None));
    }

    Err("No TV configured. Add [tv] or [tvs] section to config.toml".into())
}

pub fn load_spotify() -> Result<SpotifyConfig, Box<dyn std::error::Error>> {
    let file = load_file()?;
    let sp = file.spotify.ok_or("Missing [spotify] section in config")?;
    let refresh_token = get_secret("spotify-refresh-token")?;
    Ok(SpotifyConfig {
        refresh_token,
        tv_device_name: sp.tv_device_name,
        tv_app_id: sp.tv_app_id.unwrap_or_else(|| "3201606009684".to_string()),
    })
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("smartycrank")
        .join("config.toml")
}
