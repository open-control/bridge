#!/bin/bash
# Open Control Bridge - Cross-Platform Build Script (Bash)
# Usage:
#   ./build.sh                # Build for current platform
#   ./build.sh windows        # Build for Windows
#   ./build.sh linux          # Build for Linux
#   ./build.sh all            # Build for all platforms
#   ./build.sh setup          # Install Rust targets
#   ./build.sh clean          # Clean build artifacts

set -e

BINARY_NAME="oc-bridge"
TARGET_WINDOWS="x86_64-pc-windows-gnu"
TARGET_LINUX="x86_64-unknown-linux-gnu"
DIST_DIR="dist"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

print_header() {
    echo -e "\n${CYAN}=== $1 ===${NC}"
}

build_target() {
    local target=$1
    local output_dir=$2
    local ext=$3

    print_header "Building for $target"
    echo -e "${YELLOW}cargo build --release --target $target${NC}"

    cargo build --release --target "$target"

    mkdir -p "$output_dir"
    cp "target/$target/release/$BINARY_NAME$ext" "$output_dir/"
    echo -e "${GREEN}Output: $output_dir/$BINARY_NAME$ext${NC}"
}

case "${1:-native}" in
    native)
        print_header "Building for current platform"
        cargo build --release
        ;;
    windows)
        build_target "$TARGET_WINDOWS" "$DIST_DIR/windows" ".exe"
        ;;
    linux)
        build_target "$TARGET_LINUX" "$DIST_DIR/linux" ""
        ;;
    all)
        build_target "$TARGET_WINDOWS" "$DIST_DIR/windows" ".exe"
        build_target "$TARGET_LINUX" "$DIST_DIR/linux" ""
        print_header "All builds complete"
        echo -e "${GREEN}Outputs in $DIST_DIR/${NC}"
        ;;
    setup)
        print_header "Installing Rust targets"
        rustup target add "$TARGET_WINDOWS"
        rustup target add "$TARGET_LINUX"
        echo -e "${GREEN}Targets installed.${NC}"
        ;;
    clean)
        print_header "Cleaning build artifacts"
        cargo clean
        rm -rf "$DIST_DIR"
        echo -e "${GREEN}Clean complete.${NC}"
        ;;
    *)
        echo "Usage: $0 {native|windows|linux|all|setup|clean}"
        exit 1
        ;;
esac

echo -e "\n${GREEN}Done!${NC}"
