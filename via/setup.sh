#!/usr/bin/env bash
# Sets up ViaProxy + the PumpkinForward plugin in front of the Pumpkin server:
#   1. checks Java 17+
#   2. downloads the pinned ViaProxy jar into via/run (sha256-checked)
#   3. installs the PumpkinForward plugin (prebuilt jar, or builds it with javac)
#   4. generates a forwarding secret shared by ViaProxy and Pumpkin
#   5. moves Pumpkin to 127.0.0.1:25566 (offline mode + Velocity forwarding) in server/pumpkin.toml
# Afterwards: start server/start.sh, then via/start-via.sh. Players connect to port 25565 as before.
#
# Usage: ./setup.sh [--port 25565] [--backend-port 25566]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
source "$HERE/viaproxy.env"
PORT=25565; BACKEND_PORT=25566
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    --backend-port) BACKEND_PORT="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

for tool in java curl python3; do
  command -v "$tool" >/dev/null || { echo "$tool not found. ViaProxy needs Java 17+ (e.g. apt install openjdk-21-jre-headless)." >&2; exit 1; }
done
JAVA_MAJOR=$(java -version 2>&1 | sed -n 's/.*version "\([0-9]*\).*/\1/p' | head -n1)
[[ "${JAVA_MAJOR:-0}" -ge 17 ]] || { echo "Java 17 or newer is required (found: ${JAVA_MAJOR:-unknown})." >&2; exit 1; }

RUN="$HERE/run"
mkdir -p "$RUN/plugins/PumpkinForward"

# 2. ViaProxy
JAR="$RUN/ViaProxy-$VIAPROXY_VERSION.jar"
if [[ ! -f "$JAR" ]] || [[ "$(sha256sum "$JAR" | cut -d' ' -f1)" != "$VIAPROXY_SHA256" ]]; then
  echo "Downloading ViaProxy $VIAPROXY_VERSION..."
  curl -L --retry 5 --fail -o "$JAR" "$VIAPROXY_URL"
  [[ "$(sha256sum "$JAR" | cut -d' ' -f1)" == "$VIAPROXY_SHA256" ]] || { echo "ViaProxy jar hash mismatch" >&2; rm -f "$JAR"; exit 1; }
fi
ln -sfn "$(basename "$JAR")" "$RUN/ViaProxy.jar"   # stable name for the systemd unit
for old in "$RUN"/ViaProxy-*.jar; do [[ "$old" == "$JAR" ]] || rm -f "$old"; done

# 3. PumpkinForward plugin
PLUGIN=""
for candidate in "$HERE/PumpkinForward.jar" "$HERE/plugin/build/PumpkinForward.jar"; do
  [[ -f "$candidate" ]] && { PLUGIN="$candidate"; break; }
done
if [[ -z "$PLUGIN" ]]; then
  if command -v javac >/dev/null; then
    echo "Building the PumpkinForward plugin..."
    "$HERE/plugin/build.sh" "$JAR"
    PLUGIN="$HERE/plugin/build/PumpkinForward.jar"
  else
    echo "Downloading the PumpkinForward plugin from the latest ServerJar release..."
    curl -L --retry 5 --fail -o "$HERE/PumpkinForward.jar" \
      "https://github.com/jayclrk358/ServerJar/releases/latest/download/PumpkinForward.jar"
    PLUGIN="$HERE/PumpkinForward.jar"
  fi
fi
cp "$PLUGIN" "$RUN/plugins/PumpkinForward.jar"

# 4. Shared secret (kept if one exists so re-running setup doesn't break a working install)
SECRET_FILE="$RUN/plugins/PumpkinForward/secret.txt"
if [[ ! -s "$SECRET_FILE" ]]; then
  python3 -c 'import secrets; print(secrets.token_urlsafe(24))' > "$SECRET_FILE"
  chmod 600 "$SECRET_FILE"
fi
SECRET="$(tr -d '\r\n' < "$SECRET_FILE")"

# ViaProxy config (only written if missing, so hand edits survive)
if [[ ! -f "$RUN/viaproxy.yml" ]]; then
  sed -e "s|^bind-address:.*|bind-address: \"0.0.0.0:$PORT\"|" \
      -e "s|^target-address:.*|target-address: \"127.0.0.1:$BACKEND_PORT\"|" \
      "$HERE/viaproxy.yml" > "$RUN/viaproxy.yml"
fi

# 5. Pumpkin config
TOML="$ROOT/server/pumpkin.toml"
[[ -f "$TOML" ]] || { echo "server/pumpkin.toml not found at $TOML" >&2; exit 1; }
cp -n "$TOML" "$TOML.before-via" 2>/dev/null || true
python3 "$HERE/patch-pumpkin-toml.py" "$TOML" "$SECRET" "$BACKEND_PORT"

cat <<MSG

Done.
  ViaProxy:  via/run/ViaProxy.jar  (config: via/run/viaproxy.yml, listens on :$PORT)
  Plugin:    via/run/plugins/PumpkinForward.jar  (secret: via/run/plugins/PumpkinForward/secret.txt)
  Pumpkin:   server/pumpkin.toml now listens on 127.0.0.1:$BACKEND_PORT (backup: pumpkin.toml.before-via)

Start Pumpkin (server/start.sh), then ViaProxy (via/start-via.sh). Keep port $BACKEND_PORT closed in the firewall;
only $PORT needs to be open. Clients from Minecraft 1.7.2 up to 26.3 can now join.
MSG
