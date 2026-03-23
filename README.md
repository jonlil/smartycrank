# smartycrank

Control your Samsung TV volume from the command line via local WebSocket — no cloud, no latency, no rate limits.

Pairs with Spotify to automatically route volume controls to the TV only when music is playing there.

## Features

- Direct volume control over local network (WebSocket on port 8002)
- Spotify-aware: only controls TV when Spotify is playing on it
- Secrets stored in system keyring (libsecret/gnome-keyring)
- Token caching for fast repeated calls

## Install

```sh
cargo install --path .
```

## Setup

### 1. Find your TV

Your Samsung TV needs to be on the same network. Find its IP address in the TV's network settings.

Create the config file at `~/.config/smartycrank/config.toml`:

```toml
[tv]
host = "192.168.1.44"

[spotify]
tv_device_name = "Samsung TV QE55Q8DNA"
```

Set `tv_device_name` to whatever Spotify calls your TV (visible in Spotify's device picker).

### 2. Pair with your TV

The first time you connect, the TV will show a popup asking you to allow the connection. You may need to enable this in your TV's settings under **Settings > General > External Device Manager > Device Connection Manager**.

The pairing token is returned automatically and needs to be stored:

```sh
smartycrank store-secret tv-token <token>
```

### 3. Authorize Spotify

```sh
smartycrank auth
```

This opens your browser for Spotify login. The refresh token is stored in your system keyring and refreshed automatically.

## Usage

```sh
smartycrank up          # volume up (only if Spotify is playing on TV)
smartycrank down        # volume down
smartycrank mute        # toggle mute
smartycrank --force up  # skip Spotify check, always send to TV
```

### Example: volume key routing

Pair with a script that checks your audio setup to route volume keys intelligently:

```sh
#!/bin/sh
# If headset is active → system volume, if Spotify on TV → smartycrank, else → system volume
ACTION="$1"
SMARTYCRANK="smartycrank"

headset_state=$(pw-cli info $(pw-cli ls Node 2>/dev/null | grep -B5 "Jabra" | head -1 | tr -dc '0-9') 2>/dev/null | grep "state:" | awk '{print $2}' | tr -d '"')

if [ "$headset_state" != "running" ]; then
    "$SMARTYCRANK" "$ACTION" 2>/dev/null && exit 0
fi

case "$ACTION" in
    up)   wpctl set-volume -l 1.0 @DEFAULT_AUDIO_SINK@ 5%+ ;;
    down) wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%- ;;
    mute) wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle ;;
esac
```

## Requirements

- Samsung Smart TV (Tizen, 2016+) with WebSocket API on port 8002
- PipeWire/WirePlumber (for the volume routing example)
- gnome-keyring or another libsecret-compatible keyring
