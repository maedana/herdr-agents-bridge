# herdr-agents-bridge

A [Herdr](https://herdr.dev) plugin that lets you monitor and interact with your coding agents from your phone.

It runs a local web server that provides a mobile-friendly UI showing agent output with status-colored tabs, text input, and key sending (e.g. Escape). Connect by scanning a QR code.

## Install

```sh
herdr plugin install maedana/herdr-agents-bridge
```

Or link a local clone for development:

```sh
git clone https://github.com/maedana/herdr-agents-bridge
herdr plugin link herdr-agents-bridge
```

## Usage

### Local network

Open the QR code popup:

```sh
herdr plugin pane open --plugin maedana.agents-bridge --entrypoint qr
```

Scan the QR code with your phone to open the web UI. Your phone must be on the same network.

### Remote access via Cloudflare Tunnel

To access from outside your local network:

```sh
herdr plugin pane open --plugin maedana.agents-bridge --entrypoint qr-tunnel
```

This starts a [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/), then shows a QR code with the tunnel URL. No port forwarding or certificates needed. Requires `cloudflared` to be installed.

Opening QR (either mode) automatically starts the server (or restarts with a fresh token if already running). To stop manually: `herdr plugin action invoke stop --plugin maedana.agents-bridge`

### Keybinding examples

Add to `~/.config/herdr/config.toml` for quick access:

```toml
[[keys.command]]
key = "prefix+q"
type = "shell"
command = "herdr plugin pane open --plugin maedana.agents-bridge --entrypoint qr"
description = "agents bridge qr"

[[keys.command]]
key = "prefix+shift+q"
type = "shell"
command = "herdr plugin pane open --plugin maedana.agents-bridge --entrypoint qr-tunnel"
description = "agents bridge qr (tunnel)"
```

## Security

- 128-bit session token (generated per server start, embedded in QR URL)
- Single-device IP lock (first device to connect is locked in)
- Token required for all endpoints that expose sensitive data
- Read-only endpoints (agent list, terminal output) allow loopback for tunnel proxy
- Constant-time token comparison
- PID/URL files restricted to owner (mode 0600/0700)

## Features

- Real-time agent output via WebSocket
- Agent tab switching with status indicators (idle / active / waiting / error)
- Text input with auto-Enter
- Escape key sending
- QR code popup for quick phone connection
- Secure remote access via Cloudflare Tunnel (optional)

## Requirements

- Linux (uses `xdotool` for input injection and `xclip` for clipboard)
- Rust toolchain (built automatically by Herdr on install)
- Herdr >= 0.7.0
- `cloudflared` (optional, for remote access)

## License

MIT
