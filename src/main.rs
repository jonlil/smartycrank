mod config;
mod spotify;
mod tv;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "smartycrank", about = "Control your Samsung TV volume locally via WebSocket")]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Skip Spotify check and always send command
    #[arg(long, short, global = true)]
    force: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Volume up
    Up,
    /// Volume down
    Down,
    /// Toggle mute
    Mute,
    /// Print config file path
    Config,
    /// Store a secret in the keyring
    StoreSecret {
        /// Key: spotify-refresh-token
        key: String,
        /// The secret value
        value: String,
    },
    /// Authorize with Spotify (opens browser for PKCE OAuth flow)
    Auth,
    /// Launch an app on TV with a deep link (e.g. spotify:track:xxx, spotify:album:xxx)
    Launch {
        /// Deep link URI (e.g. spotify:track:4cOdK2wGLETKBW3PvgPWqT)
        uri: String,
    },
    /// Control viska (TV4 Play)
    #[command(subcommand)]
    Viska(ViskaCommand),
}

#[derive(Subcommand)]
enum ViskaCommand {
    /// Play an asset on TV4 Play
    Play {
        /// TV4 Play asset ID
        asset_id: String,
    },
    /// Seek to a position in current playback (e.g. "1:30:00", "45:00", "+60", "-30")
    Seek {
        /// Time position: "H:MM:SS", "MM:SS", "+seconds", "-seconds"
        position: String,
    },
    /// Log out of TV4 Play
    Logout,
}

fn parse_timestamp(s: &str) -> Result<u64, Box<dyn std::error::Error>> {
    let parts: Vec<&str> = s.split(':').collect();
    let ms = match parts.len() {
        1 => parts[0].parse::<u64>()? * 1000,
        2 => {
            let m = parts[0].parse::<u64>()?;
            let s = parts[1].parse::<u64>()?;
            (m * 60 + s) * 1000
        }
        3 => {
            let h = parts[0].parse::<u64>()?;
            let m = parts[1].parse::<u64>()?;
            let s = parts[2].parse::<u64>()?;
            (h * 3600 + m * 60 + s) * 1000
        }
        _ => return Err("Invalid timestamp format, use H:MM:SS, MM:SS, or seconds".into()),
    };
    Ok(ms)
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Command::Config => {
            println!("{}", config::config_path().display());
            return Ok(());
        }
        Command::StoreSecret { key, value } => {
            config::store_secret(&key, &value)?;
            println!("Stored '{key}' in keyring");
            return Ok(());
        }
        Command::Auth => {
            spotify::auth().await?;
            return Ok(());
        }
        Command::Launch { ref uri } => {
            let tv_cfg = config::load_tv()?;
            let tv = tv::SamsungTv::new(&tv_cfg);
            let sp_cfg = config::load_spotify()?;
            tv.launch_app(&sp_cfg.tv_app_id, uri).await?;
            return Ok(());
        }
        Command::Viska(ref cmd) => {
            let tv_cfg = config::load_tv()?;
            let tv = tv::SamsungTv::new(&tv_cfg);
            if !tv.is_on().await {
                eprintln!("TV is off or unreachable");
                std::process::exit(1);
            }
            tv.ensure_app_running("tv4tizenap.App").await?;
            match cmd {
                ViskaCommand::Play { asset_id } => {
                    tv.send_to_channel(
                        "tv4tizenap.App",
                        "play",
                        serde_json::json!({"assetId": asset_id}),
                    ).await?;
                }
                ViskaCommand::Seek { position } => {
                    let data = if position.starts_with('+') || position.starts_with('-') {
                        let secs: i64 = position.parse().map_err(|_| "Invalid relative offset")?;
                        serde_json::json!({"relative": secs})
                    } else {
                        let ms = parse_timestamp(position)?;
                        serde_json::json!({"position": ms})
                    };
                    tv.send_to_channel("tv4tizenap.App", "seek", data).await?;
                }
                ViskaCommand::Logout => {
                    tv.send_to_channel(
                        "tv4tizenap.App",
                        "logout",
                        serde_json::json!({}),
                    ).await?;
                    eprintln!("Logged out");
                }
            }
            return Ok(());
        }
        _ => {}
    }

    let tv_cfg = config::load_tv()?;
    let tv = tv::SamsungTv::new(&tv_cfg);

    if !cli.force {
        let sp_cfg = config::load_spotify()?;
        let sp = spotify::Spotify::new(&sp_cfg);
        if !sp.is_playing_on_tv().await? {
            println!("Spotify is not playing on TV, use --force to send anyway");
            return Ok(());
        }
    }

    match cli.command {
        Command::Up => {
            tv.volume_up().await?;
        }
        Command::Down => {
            tv.volume_down().await?;
        }
        Command::Mute => {
            tv.mute().await?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
