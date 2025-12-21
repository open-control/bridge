#!/usr/bin/env bash
# Helper for admin elevation on Windows

require_admin() {
    # Only needed on Windows
    if [[ "$OSTYPE" != "msys" && "$OSTYPE" != "cygwin" ]]; then
        return 0
    fi

    # Check if already admin
    if net session &>/dev/null; then
        return 0
    fi

    # Get paths
    local caller="${BASH_SOURCE[1]}"
    local script_name=$(basename "$caller")
    local script_dir="$(cd "$(dirname "$caller")" && pwd)"
    local bridge_dir="$(dirname "$script_dir")"

    local binary="$bridge_dir/target/x86_64-pc-windows-gnu/release/oc-bridge.exe"
    if [[ ! -f "$binary" ]]; then
        binary="$bridge_dir/target/release/oc-bridge.exe"
    fi
    local win_binary=$(cygpath -w "$binary" 2>/dev/null || echo "$binary")

    # Determine the command based on script name
    local cmd=""
    case "$script_name" in
        install.sh)       cmd="service install $*" ;;
        uninstall.sh)     cmd="service uninstall" ;;
        service-start.sh) cmd="service start" ;;
        stop.sh)          cmd="service stop" ;;
        restart.sh)       cmd="service restart" ;;
        *)                cmd="$*" ;;
    esac

    echo "Requesting administrator privileges..."
    echo "Command: oc-bridge $cmd"
    echo ""

    # Create a temp script that runs the command and pauses
    local temp_script=$(mktemp --suffix=.bat)
    local win_temp=$(cygpath -w "$temp_script" 2>/dev/null || echo "$temp_script")

    cat > "$temp_script" << EOF
@echo off
echo ========================================
echo Open Control Bridge - Admin Command
echo ========================================
echo.
"$win_binary" $cmd
echo.
echo ========================================
echo Command completed. Press any key to close...
pause > nul
EOF

    # Run the batch file as admin
    powershell -Command "Start-Process -Verb RunAs -FilePath 'cmd.exe' -ArgumentList '/c', '\"$win_temp\"' -Wait"
    local result=$?

    # Cleanup
    rm -f "$temp_script"

    # Show status
    echo ""
    echo "Checking status..."
    "$binary" service status

    exit $result
}
