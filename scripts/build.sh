#!/usr/bin/env bash
# Build the bridge in release mode

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$(dirname "$SCRIPT_DIR")"

echo "Building open-control-bridge..."
cargo build --release

echo ""
echo "Build complete!"
if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "cygwin" || "$OSTYPE" == "win32" ]]; then
    ls -lh target/x86_64-pc-windows-gnu/release/oc-bridge.exe 2>/dev/null || \
    ls -lh target/release/oc-bridge.exe
else
    ls -lh target/release/oc-bridge
fi
