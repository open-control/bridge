#!/usr/bin/env bash
# List available serial ports

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/bridge.sh" list-ports
