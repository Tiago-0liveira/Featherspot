#!/usr/bin/env bash
set -euo pipefail

CONFIG_FILE="/Users/tiago/Library/Application Support/spotifyd/spotifyd.conf"
CACHE_DIR="/Users/tiago/Library/Caches/spotifyd"

ensure_config() {
    mkdir -p "$(dirname "$CONFIG_FILE")" "$CACHE_DIR"
    if [ ! -f "$CONFIG_FILE" ]; then
        cat <<'EOF' > "$CONFIG_FILE"
[global]
device_name = "Mellowdeck"
device_type = "computer"
backend = "portaudio"
volume_controller = "softvol"
bitrate = 320
volume_normalisation = true
normalisation_pregain = -10
cache_path = "/Users/tiago/Library/Caches/spotifyd"
EOF
        echo "Created spotifyd config at $CONFIG_FILE"
    fi
}

cmd="${1:-status}"

case "$cmd" in
    start)
        ensure_config
        echo "Starting spotifyd via brew services..."
        brew services start spotifyd
        sleep 1
        $0 status
        ;;
    stop)
        echo "Stopping spotifyd..."
        brew services stop spotifyd || true
        pkill -f spotifyd 2>/dev/null || true
        echo "spotifyd stopped."
        ;;
    restart)
        ensure_config
        echo "Restarting spotifyd..."
        brew services restart spotifyd
        sleep 1
        $0 status
        ;;
    status)
        if pgrep -x spotifyd >/dev/null; then
            pid=$(pgrep -x spotifyd | head -n 1)
            echo "spotifyd is RUNNING (PID: $pid)"
            echo "Device name: Mellowdeck (CoreAudio / portaudio backend)"
            echo "Config: $CONFIG_FILE"
        else
            echo "spotifyd is NOT running."
            echo "Start it with: $0 start (or 'brew services start spotifyd')"
        fi
        ;;
    run)
        ensure_config
        echo "Running spotifyd in foreground (Ctrl+C to stop)..."
        spotifyd --no-daemon --config-path "$CONFIG_FILE" -v
        ;;
    *)
        echo "Usage: $0 {start|stop|restart|status|run}"
        exit 1
        ;;
esac
