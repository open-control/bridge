#!/usr/bin/env bash
# Install bridge as system service (requires admin on Windows)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/_admin.sh"
require_admin "$@"

exec "$SCRIPT_DIR/bridge.sh" service install "$@"
