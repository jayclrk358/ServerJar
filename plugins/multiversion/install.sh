#!/usr/bin/env bash
# Installs the multiversion plugin into the server folder next to this repo's server/ directory:
#   1. copies multiversion.wasm into server/plugins/
#   2. writes server/plugins/data/multiversion/config.toml with a fresh forwarding secret
#   3. edits server/pumpkin.toml: plugins on, Pumpkin on 127.0.0.1:25566, offline mode, velocity forwarding
# Usage: ./install.sh [--port 25565] [--backend-port 25566] [--server DIR] [--wasm FILE]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SERVER="$(cd "$HERE/../../server" 2>/dev/null && pwd || echo "$HERE/../../server")"
WASM=""; PORT=25565; BACKEND_PORT=25566
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    --backend-port) BACKEND_PORT="$2"; shift 2 ;;
    --server) SERVER="$2"; shift 2 ;;
    --wasm) WASM="$2"; shift 2 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done
command -v python3 >/dev/null || { echo "python3 is needed to edit pumpkin.toml" >&2; exit 1; }
[[ -f "$SERVER/pumpkin.toml" ]] || { echo "No pumpkin.toml in $SERVER (use --server DIR)" >&2; exit 1; }

if [[ -z "$WASM" ]]; then
  for c in "$HERE/multiversion.wasm" "$HERE/target/wasm32-wasip2/release/multiversion.wasm"; do
    [[ -f "$c" ]] && { WASM="$c"; break; }
  done
fi
if [[ -z "$WASM" ]]; then
  if command -v cargo >/dev/null; then
    echo "Building the plugin..."
    "$HERE/build.sh"
    WASM="$HERE/target/wasm32-wasip2/release/multiversion.wasm"
  else
    echo "Downloading multiversion.wasm from the latest ServerJar release..."
    curl -L --retry 5 --fail -o "$HERE/multiversion.wasm" "https://github.com/jayclrk358/ServerJar/releases/latest/download/multiversion.wasm"
    WASM="$HERE/multiversion.wasm"
  fi
fi

mkdir -p "$SERVER/plugins/data/multiversion"
cp "$WASM" "$SERVER/plugins/multiversion.wasm"

CONFIG="$SERVER/plugins/data/multiversion/config.toml"
if [[ -f "$CONFIG" ]]; then
  SECRET="$(python3 - "$CONFIG" <<'PY'
import re, sys
m = re.search(r'^forwarding_secret\s*=\s*"([^"]*)"', open(sys.argv[1]).read(), re.M)
print(m.group(1) if m else "")
PY
)"
fi
if [[ -z "${SECRET:-}" ]]; then
  SECRET="$(python3 -c 'import secrets; print(secrets.token_urlsafe(24))')"
  cat > "$CONFIG" <<CFG
# multiversion plugin configuration. Players connect to [listen].address; the plugin translates
# their protocol and forwards them to Pumpkin at [backend].address (localhost only).

[listen]
address = "0.0.0.0:$PORT"

[backend]
address = "127.0.0.1:$BACKEND_PORT"
# Must equal networking.proxy.velocity.secret in pumpkin.toml (install.sh keeps them in sync).
forwarding_secret = "$SECRET"

[auth]
# Verify players with Mojang. Keep on for a public server.
online_mode = true
session_server = "https://sessionserver.mojang.com"
prevent_proxy_connections = false

[status]
# Version text in the server list. Empty = "<lowest supported>-<newest>".
version_name = ""

[versions]
# Client versions allowed to join, e.g. ["26.2", "26.3"]. Empty = every supported version.
allow = []
kick_unsupported_message = "This server supports Minecraft {versions}. You are on {client}."

[limits]
compression_threshold = 256
max_connections = 200
login_timeout_secs = 30
CFG
fi

cp -n "$SERVER/pumpkin.toml" "$SERVER/pumpkin.toml.before-multiversion" 2>/dev/null || true
python3 - "$SERVER/pumpkin.toml" "$SECRET" "$BACKEND_PORT" <<'PY'
import re, sys
path, secret, port = sys.argv[1], sys.argv[2], sys.argv[3]
esc = secret.replace("\\", "\\\\").replace('"', '\\"')
wanted = {
    "plugins": {"enabled": "true", "allow_unsigned": "true", "verify_signatures": "false"},
    "plugins.overrides.multiversion": {"enabled": "true", "allowed_permissions": '["network.tcp", "http.outbound", "fs.read.data", "fs.write.data"]', "loopback_only": "false"},
    "networking.java": {"address": f'"127.0.0.1:{port}"'},
    "networking.proxy": {"enabled": "true"},
    "networking.proxy.velocity": {"enabled": "true", "secret": f'"{esc}"'},
}
lines = open(path, encoding="utf-8").read().split("\n")
out, pending = [], {}
def flush():
    for k, v in pending.items():
        out.append(f"{k} = {v}")
for line in lines:
    h = re.match(r"^\s*\[([^\]]+)\]\s*$", line)
    if h:
        flush(); section = h.group(1).strip(); pending = dict(wanted.get(section, {})); out.append(line); continue
    kv = re.match(r"^\s*([A-Za-z0-9_]+)\s*=", line)
    if kv and kv.group(1) in pending:
        out.append(f"{kv.group(1)} = {pending.pop(kv.group(1))}"); continue
    out.append(line)
flush()
text = "\n".join(out)
for name, keys in wanted.items():
    if f"[{name}]" not in text:
        text += f"\n\n[{name}]\n" + "\n".join(f"{k} = {v}" for k, v in keys.items())
open(path, "w", encoding="utf-8", newline="\n").write(text + ("\n" if not text.endswith("\n") else ""))
print(f"Patched {path}")
PY

cat <<MSG

Installed.
  Plugin:  $SERVER/plugins/multiversion.wasm
  Config:  $CONFIG
  Pumpkin: $SERVER/pumpkin.toml now listens on 127.0.0.1:$BACKEND_PORT with velocity forwarding (backup: pumpkin.toml.before-multiversion)

Start the server as usual. Players connect to port $PORT; keep $BACKEND_PORT closed in the firewall.
MSG
