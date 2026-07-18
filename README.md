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

Open the QR code popup (starts the server automatically if needed):

```sh
herdr plugin pane open --plugin maedana.agents-bridge --entrypoint qr
```

Scan the QR code with your phone to open the web UI.

You can also start/stop the server independently:

```sh
# Start the server in the background
herdr plugin action invoke start --plugin maedana.agents-bridge

# Stop the server
herdr plugin action invoke stop --plugin maedana.agents-bridge
```

## Features

- Real-time agent output via WebSocket
- Agent tab switching with status indicators (idle / active / waiting / error)
- Text input with auto-Enter
- Escape key sending
- QR code popup for quick phone connection
- Single-device authentication (first device to connect is locked in)

## Requirements

- Linux (uses `xdotool` for input injection and `xclip` for clipboard)
- Rust toolchain (built automatically by Herdr on install)
- Herdr >= 0.7.0

## License

MIT
