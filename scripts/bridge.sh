#!/usr/bin/env bash
# Open Control Bridge - Main launcher script

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BRIDGE_DIR="$(dirname "$SCRIPT_DIR")"

# Find the binary
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]; then
    BINARY="$BRIDGE_DIR/target/x86_64-pc-windows-gnu/release/oc-bridge.exe"
    if [[ ! -f "$BINARY" ]]; then
        BINARY="$BRIDGE_DIR/target/release/oc-bridge.exe"
    fi
else
    BINARY="$BRIDGE_DIR/target/release/oc-bridge"
fi

# Check if binary exists
if [[ ! -f "$BINARY" ]]; then
    echo "Bridge not built. Run: ./scripts/build.sh"
    exit 1
fi

# Forward all arguments to the bridge
exec "$BINARY" "$@"
