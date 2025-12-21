# Open Control Bridge

Transparent Serial-to-TCP bridge for the open-control framework.

Forwards COBS-framed messages between a USB Serial device (Teensy 4.1) and a TCP socket (Bitwig extension).

## Features

- **Auto-detection**: Automatically finds connected Teensy devices
- **Cross-platform**: Windows, macOS, Linux
- **Service mode**: Install as system service for hands-free operation
- **Transparent**: Zero protocol knowledge, pure byte forwarding
- **Minimal**: ~3MB binary, instant startup

## Installation

### From Release (Recommended)

Download the latest release for your platform:
- `oc-bridge-windows.exe`
- `oc-bridge-macos`
- `oc-bridge-linux`

### From Source

```bash
cargo build --release
```

## Usage

### Quick Start

```bash
# Auto-detect Teensy and start bridge
oc-bridge start

# Specify port manually
oc-bridge start --port COM3 --tcp-port 9000
```

### List Available Ports

```bash
oc-bridge list-ports
```

### Install as Service

```bash
# Install and start automatically on boot
oc-bridge install

# With specific port
oc-bridge install --port COM3

# Check status
oc-bridge status

# Uninstall
oc-bridge uninstall
```

## Architecture

```
┌──────────┐   USB Serial    ┌──────────┐    TCP       ┌─────────┐
│ Teensy   │ ←─────────────→ │oc-bridge │ ←──────────→ │ Bitwig  │
│   4.1    │   COBS frames   │          │  len-prefix  │  Java   │
└──────────┘                 └──────────┘              └─────────┘
```

### Framing

- **Serial side**: COBS encoding with 0x00 delimiter
- **TCP side**: 32-bit big-endian length prefix (Bitwig standard)

### Performance

| Metric | Value |
|--------|-------|
| Bandwidth | ~10 Mbit/s |
| Latency | ~2-3ms round-trip |
| Messages/sec | 1000+ |

## Configuration

Config file location:
- Windows: `%APPDATA%\open-control\bridge\config.toml`
- macOS: `~/Library/Application Support/com.open-control.bridge/config.toml`
- Linux: `~/.config/open-control/bridge/config.toml`

```toml
# Optional: specify serial port (auto-detect if omitted)
serial_port = "COM3"

# Serial baud rate (default: 2000000)
baud_rate = 2000000

# TCP port for Bitwig (default: 9000)
tcp_port = 9000

# Start minimized (for future GUI)
start_minimized = false

# Auto-start on system boot
auto_start = true
```

## Development

```bash
# Run in development
cargo run -- start --verbose

# Run tests
cargo test

# Build release
cargo build --release
```

## License

MIT
