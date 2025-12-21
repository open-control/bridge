#!/usr/bin/env bash
# Start the bridge in foreground (dev mode)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "$SCRIPT_DIR/bridge.sh" start -v "$@"
