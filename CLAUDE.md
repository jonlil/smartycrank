# smartycrank

CLI tool for controlling Samsung TVs via WebSocket. Volume control, Spotify integration, and Viska (TV4 Play) playback.

## Build & install

```sh
cargo build              # dev build
cargo install --path .   # release build to ~/.cargo/bin (no sudo needed)
```

During development, `cargo install --path .` puts the binary in `~/.cargo/bin/` which takes priority over `/usr/bin/` (the PKGBUILD install location). Remove with `rm ~/.cargo/bin/smartycrank` or `cargo uninstall smartycrank` when a new release is installed system-wide.

## Config

Config lives at `~/.config/smartycrank/config.toml`. Supports named TV profiles:

```toml
[tvs.living_room]
host = "192.168.1.44"

[tvs.bedroom]
host = "192.168.1.207"

[default]
tv = "living_room"

[spotify]
tv_device_name = "Samsung TV QE55Q8DNA"
```

Legacy single `[tv]` section still works as fallback.

## TV selection

`--tv` flag selects which TV to target. Accepts a profile name or raw IP:

```sh
smartycrank --tv bedroom viska play <asset_id>
smartycrank --tv 192.168.1.207 viska play <asset_id>
smartycrank viska play <asset_id>  # uses default.tv
```

## Secrets

Stored in system keyring under service "smartycrank":

```sh
smartycrank store-secret tv-token <token>
smartycrank store-secret spotify-refresh-token <token>
```

## Project structure

- `src/main.rs` - CLI (clap derive) and command dispatch
- `src/config.rs` - TOML config loading, TV profile resolution, keyring access
- `src/tv.rs` - Samsung TV WebSocket/REST API (volume, app launch, channel messaging)
- `src/spotify.rs` - Spotify PKCE auth and playback status
