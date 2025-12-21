#!/usr/bin/env bash
# Start bridge service (requires admin on Windows)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_admin.sh"
require_admin "$@"

exec "$SCRIPT_DIR/bridge.sh" service start
