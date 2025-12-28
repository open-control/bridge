# Open Control Bridge

Serial-to-UDP bridge for high-bandwidth communication between hardware controllers and DAW extensions.

![Bridge TUI](docs/bridge-tui.png)

## Why Not MIDI SysEx?

| | MIDI SysEx | USB Serial + UDP |
|---|---|---|
| Bandwidth | ~31.25 kbit/s | Full USB speed (480 Mbit/s) |
| Encoding | 7-bit (overhead) | 8-bit native |
| Latency | ~10-50ms | ~2-3ms |
| Reliability | Lossy under load | Reliable |

MIDI SysEx was designed for patch dumps, not real-time bidirectional communication. The bridge bypasses MIDI entirely using direct USB serial, enabling features like live parameter feedback, waveform displays, and responsive UI updates.

## Architecture

```
┌──────────────┐    USB Serial     ┌────────────┐      UDP       ┌─────────────┐
│   Teensy     │◄─────────────────►│  oc-bridge │◄──────────────►│   Bitwig    │
│  Controller  │   COBS framing    │            │   :9000        │  Extension  │
└──────────────┘                   └────────────┘                └─────────────┘
```

Messages are defined using [protocol-codegen](https://github.com/open-control/protocol-codegen), which generates type-safe C++ (Teensy) and Java (Bitwig) code from Python definitions.

## Quick Start

### Download

Prebuilt binaries available in [Releases](https://github.com/open-control/bridge/releases):
- `oc-bridge-windows.exe`
- `oc-bridge-linux`

### Run

```bash
# Launch TUI (auto-detects Teensy)
oc-bridge

# Headless mode
oc-bridge --headless

# Specify port manually
oc-bridge --port COM3 --udp-port 9000
```

### TUI Controls

| Key | Action |
|-----|--------|
| `S` | Start/Stop bridge |
| `1` `2` `3` | Filter: Proto / Debug / All |
| `P` | Pause log |
| `C` | Copy log to clipboard |
| `O` | Export log to file |
| `F` | Edit config |
| `Q` | Quit |

### Windows Service

```bash
# Install (runs at startup, no window)
oc-bridge    # then press 'I' in TUI

# Or from command line (requires elevation)
oc-bridge --install-service
oc-bridge --uninstall-service
```

## Configuration

Config file: `config.toml` (next to executable)

```toml
[bridge]
serial_port = ""        # Empty = auto-detect Teensy
udp_port = 9000

[logs]
max_entries = 200
export_max = 2000

[ui]
default_filter = "All"  # "Proto", "Debug", or "All"
```

## Build from Source

### Prerequisites

- [Rust](https://rustup.rs/) (stable)

### Build

```bash
cargo build --release
```

Binary: `target/release/oc-bridge` (or `.exe` on Windows)

### Cross-compilation

```bash
# Linux → Windows (requires mingw-w64)
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

## Protocol Integration

The bridge is protocol-agnostic. Message encoding/decoding is handled by code generated from [protocol-codegen](https://github.com/open-control/protocol-codegen):

1. Define messages in Python
2. Generate C++ (Teensy) + Java (Bitwig)
3. Bridge transparently forwards COBS frames ↔ UDP datagrams

## License

MIT
