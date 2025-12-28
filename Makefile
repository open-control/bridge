# Open Control Bridge - Cross-Platform Build System
# Usage:
#   make build              # Build for current platform (native)
#   make release            # Release build for current platform
#   make windows            # Build for Windows (x86_64)
#   make linux              # Build for Linux (x86_64)
#   make all                # Build for all targets
#   make clean              # Clean build artifacts

BINARY_NAME := oc-bridge
CARGO := cargo

# Target triples
TARGET_WINDOWS := x86_64-pc-windows-gnu
TARGET_LINUX := x86_64-unknown-linux-gnu

# Output directories
DIST_DIR := dist
DIST_WINDOWS := $(DIST_DIR)/windows
DIST_LINUX := $(DIST_DIR)/linux

.PHONY: all build release windows linux clean setup-targets help

# Default: build for current platform
build:
	$(CARGO) build

# Release build for current platform
release:
	$(CARGO) build --release

# Windows release build
windows:
	@echo "Building for Windows ($(TARGET_WINDOWS))..."
	$(CARGO) build --release --target $(TARGET_WINDOWS)
	@mkdir -p $(DIST_WINDOWS)
	@cp target/$(TARGET_WINDOWS)/release/$(BINARY_NAME).exe $(DIST_WINDOWS)/ 2>/dev/null || \
		copy target\$(TARGET_WINDOWS)\release\$(BINARY_NAME).exe $(DIST_WINDOWS)\ 2>nul || true
	@echo "Output: $(DIST_WINDOWS)/$(BINARY_NAME).exe"

# Linux release build
linux:
	@echo "Building for Linux ($(TARGET_LINUX))..."
	$(CARGO) build --release --target $(TARGET_LINUX)
	@mkdir -p $(DIST_LINUX)
	@cp target/$(TARGET_LINUX)/release/$(BINARY_NAME) $(DIST_LINUX)/ 2>/dev/null || \
		copy target\$(TARGET_LINUX)\release\$(BINARY_NAME) $(DIST_LINUX)\ 2>nul || true
	@echo "Output: $(DIST_LINUX)/$(BINARY_NAME)"

# Build all targets
all: windows linux
	@echo "All builds complete. Outputs in $(DIST_DIR)/"

# Install required Rust targets
setup-targets:
	rustup target add $(TARGET_WINDOWS)
	rustup target add $(TARGET_LINUX)

# Clean build artifacts
clean:
	$(CARGO) clean
	rm -rf $(DIST_DIR) 2>/dev/null || rmdir /s /q $(DIST_DIR) 2>nul || true

# Show help
help:
	@echo "Open Control Bridge - Build Targets"
	@echo ""
	@echo "  make build         - Debug build (current platform)"
	@echo "  make release       - Release build (current platform)"
	@echo "  make windows       - Release build for Windows x86_64"
	@echo "  make linux         - Release build for Linux x86_64"
	@echo "  make all           - Build for all platforms"
	@echo "  make setup-targets - Install required Rust targets"
	@echo "  make clean         - Remove build artifacts"
