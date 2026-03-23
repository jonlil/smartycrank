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
