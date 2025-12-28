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
# Native build (current platform)
cargo build --release

# Or use build scripts:
./build.sh              # Linux/macOS
.\build.ps1             # Windows PowerShell
```

## Building for Multiple Platforms

### Prerequisites

1. **Rust toolchain** (stable)
2. **Target platforms** (install once):
   ```bash
   rustup target add x86_64-pc-windows-gnu      # Windows
   rustup target add x86_64-unknown-linux-gnu   # Linux
   ```

### Native Compilation (Recommended)

Build on each target platform for best compatibility:

| Platform | Command |
|----------|---------|
| Windows  | `cargo build --release` |
| Linux    | `cargo build --release` |
| macOS    | `cargo build --release` |

### Using Build Scripts

**PowerShell (Windows):**
```powershell
.\build.ps1                    # Build for current platform
.\build.ps1 -Target windows    # Build for Windows
.\build.ps1 -Target linux      # Build for Linux (requires cross-compiler)
.\build.ps1 -Target all        # Build all targets
.\build.ps1 -Setup             # Install Rust targets
.\build.ps1 -Clean             # Clean build artifacts
```

**Bash (Linux/macOS):**
```bash
./build.sh              # Build for current platform
./build.sh windows      # Build for Windows (requires cross-compiler)
./build.sh linux        # Build for Linux
./build.sh all          # Build all targets
./build.sh setup        # Install Rust targets
./build.sh clean        # Clean build artifacts
```

**Make:**
```bash
make release            # Build for current platform
make windows            # Build for Windows
make linux              # Build for Linux
make all                # Build all targets
make setup-targets      # Install Rust targets
```

### Cross-Compilation

Cross-compiling requires platform-specific linkers:

| From | To | Required Linker |
|------|-----|-----------------|
| Linux | Windows | `x86_64-w64-mingw32-gcc` (mingw-w64) |
| Windows | Linux | `x86_64-linux-gnu-gcc` (WSL or Docker) |

**Recommended approach:** Build natively on each platform or use CI/CD.

### Output

Built binaries are placed in:
- Native: `target/release/oc-bridge[.exe]`
- Cross: `dist/{windows,linux}/oc-bridge[.exe]`

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
