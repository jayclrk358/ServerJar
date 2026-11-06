#!/usr/bin/env bash
# ServerJar one-command installer for a Linux VPS (Ubuntu 22.04+/Debian 12+, x86-64).
#
#   curl -fsSL https://raw.githubusercontent.com/jayclrk358/ServerJar/main/install.sh | sudo bash
#
# What it does, idempotently:
#   1. installs the few packages needed (ca-certificates, python3, curl, ufw)
#   2. creates a `minecraft` user and puts the latest release in /opt/minecraft
#   3. installs the multiversion plugin and wires pumpkin.toml for it (other client versions can join)
#   4. installs the web panel as a service that also starts the server at boot
#   5. opens the game port in the firewall
#   6. installs the `mc` command: mc start|stop|restart|status|console|logs|panel|update
# Re-running it updates the server binary, plugin and panel and keeps your config and world.
#
# Options (environment variables):  MC_DIR=/opt/minecraft  MC_PORT=25565  PANEL_PORT=8080
#   PANEL_PASSWORD=...  (otherwise asked)   PANEL_PUBLIC=1 (panel on all interfaces, use with HTTPS)
#   MC_RELEASE_URL=... (bundle tarball to install instead of the latest GitHub release)
set -euo pipefail

MC_DIR="${MC_DIR:-/opt/minecraft}"
MC_PORT="${MC_PORT:-25565}"
PANEL_PORT="${PANEL_PORT:-8080}"
MC_USER="${MC_USER:-minecraft}"
REPO="jayclrk358/ServerJar"
BUNDLE_URL="${MC_RELEASE_URL:-https://github.com/$REPO/releases/latest/download/ServerJar-linux-x86_64.tar.gz}"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }
[[ $(id -u) -eq 0 ]] || die "run as root (sudo bash install.sh)"
[[ "$(uname -m)" == "x86_64" ]] || die "only x86-64 Linux is supported by the prebuilt release"

# 1. packages ---------------------------------------------------------------------------------------
say "Installing packages"
export DEBIAN_FRONTEND=noninteractive
if command -v apt-get >/dev/null; then
  apt-get update -qq
  apt-get install -y -qq ca-certificates curl python3 ufw tar >/dev/null
  update-ca-certificates >/dev/null 2>&1 || true
else
  for t in curl python3 tar; do command -v "$t" >/dev/null || die "$t is missing and this is not a Debian/Ubuntu system; install it first"; done
fi

# 2. user + files -----------------------------------------------------------------------------------
id "$MC_USER" >/dev/null 2>&1 || useradd --system --create-home --home-dir "$MC_DIR" --shell /usr/sbin/nologin "$MC_USER"
mkdir -p "$MC_DIR"
say "Downloading the latest ServerJar release"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
curl -fsSL --retry 5 --max-time 600 -o "$TMP/bundle.tar.gz" "$BUNDLE_URL" || die "download failed: $BUNDLE_URL"
tar -xzf "$TMP/bundle.tar.gz" -C "$TMP"
B="$TMP/ServerJar"
[[ -x "$B/pumpkin" ]] || die "bundle has no pumpkin binary"

# binary, scripts, panel and plugin are replaced on every run; config and world are kept
install -m 755 "$B/pumpkin" "$MC_DIR/pumpkin"
install -m 755 "$B/start.sh" "$MC_DIR/start.sh"
[[ -f "$MC_DIR/pumpkin.toml" ]] || install -m 644 "$B/pumpkin.toml" "$MC_DIR/pumpkin.toml"
mkdir -p "$MC_DIR/panel" "$MC_DIR/plugins/data/multiversion"
RAW="https://raw.githubusercontent.com/$REPO/${MC_BRANCH:-main}"
# Panel: take it from the bundle, else straight from the repository (it is two plain files).
if [[ -d "$B/panel" ]]; then
  install -m 644 "$B/panel/panel.py" "$B/panel/panel.html" "$MC_DIR/panel/"
else
  say "Fetching the web panel from the repository"
  curl -fsSL --retry 3 -o "$MC_DIR/panel/panel.py" "$RAW/panel/panel.py" || die "could not download panel.py"
  curl -fsSL --retry 3 -o "$MC_DIR/panel/panel.html" "$RAW/panel/panel.html" || die "could not download panel.html"
fi
# Plugin: from the bundle, else the release asset; without it the server still runs, just 26.3-only.
HAVE_PLUGIN=0
if [[ -f "$B/plugins/multiversion/multiversion.wasm" ]]; then
  install -m 644 "$B/plugins/multiversion/multiversion.wasm" "$MC_DIR/plugins/multiversion.wasm"; HAVE_PLUGIN=1
elif curl -fsSL --retry 3 --max-time 120 -o "$TMP/multiversion.wasm" "https://github.com/$REPO/releases/latest/download/multiversion.wasm" 2>/dev/null; then
  install -m 644 "$TMP/multiversion.wasm" "$MC_DIR/plugins/multiversion.wasm"; HAVE_PLUGIN=1
elif [[ -f "$MC_DIR/plugins/multiversion.wasm" ]]; then
  HAVE_PLUGIN=1
else
  say "No multiversion.wasm in this release yet: the server will accept 26.3 clients only. Re-run 'mc update' after the next release."
fi

# 3. plugin + pumpkin.toml ------------------------------------------------------------------------
if [[ "$HAVE_PLUGIN" == "1" ]]; then
CFG="$MC_DIR/plugins/data/multiversion/config.toml"
if [[ -f "$CFG" ]]; then
  SECRET="$(python3 -c 'import re,sys; m=re.search(r"^forwarding_secret\s*=\s*\"([^\"]*)\"", open(sys.argv[1]).read(), re.M); print(m.group(1) if m else "")' "$CFG")"
fi
if [[ -z "${SECRET:-}" ]]; then
  SECRET="$(python3 -c 'import secrets; print(secrets.token_urlsafe(24))')"
  cat > "$CFG" <<TOML
# multiversion plugin: lets other Minecraft versions join. Players connect to [listen].address.
[listen]
address = "0.0.0.0:$MC_PORT"

[backend]
address = "127.0.0.1:25566"
forwarding_secret = "$SECRET"

[auth]
online_mode = true
TOML
fi
say "Configuring pumpkin.toml"
python3 - "$MC_DIR/pumpkin.toml" "$SECRET" "$MC_PORT" <<'PY'
import re, sys
path, secret, port = sys.argv[1], sys.argv[2], sys.argv[3]
wanted = {
    "plugins": {"enabled": "true", "allow_unsigned": "true", "verify_signatures": "false"},
    "plugins.overrides.multiversion": {"enabled": "true", "allowed_permissions": '["network.tcp", "http.outbound", "fs.read.data", "fs.write.data"]'},
    "networking.java": {"address": '"127.0.0.1:25566"'},
    "networking.proxy": {"enabled": "true"},
    "networking.proxy.velocity": {"enabled": "true", "secret": '"' + secret.replace("\\", "\\\\").replace('"', '\\"') + '"'},
    "commands": {"use_tty": "false"},
}
lines = open(path, encoding="utf-8").read().split("\n")
out, pending, section = [], {}, None
def flush():
    for k, v in pending.items(): out.append(f"{k} = {v}")
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
open(path, "w", encoding="utf-8", newline="\n").write(text.rstrip("\n") + "\n")
PY
else
  # No plugin: Pumpkin itself answers on the public port.
  say "Configuring pumpkin.toml"
  python3 - "$MC_DIR/pumpkin.toml" "$MC_PORT" <<'PY'
import re, sys
path, port = sys.argv[1], sys.argv[2]
lines = open(path, encoding="utf-8").read().split("\n")
out, section = [], None
for line in lines:
    h = re.match(r"^\s*\[([^\]]+)\]\s*$", line)
    if h: section = h.group(1).strip()
    if section == "networking.java" and re.match(r"^\s*address\s*=", line): line = f'address = "0.0.0.0:{port}"'
    if section == "commands" and re.match(r"^\s*use_tty\s*=", line): line = "use_tty = false"
    out.append(line)
open(path, "w", encoding="utf-8", newline="\n").write("\n".join(out))
PY
fi

# 4. panel service ----------------------------------------------------------------------------------
if [[ ! -f "$MC_DIR/.panel-auth.json" ]]; then
  if [[ -z "${PANEL_PASSWORD:-}" ]]; then
    if [[ -t 0 ]]; then
      read -r -s -p "Choose a panel password (8+ characters): " PANEL_PASSWORD </dev/tty; echo
    else
      PANEL_PASSWORD="$(python3 -c 'import secrets; print(secrets.token_urlsafe(12))')"
      GENERATED_PW=1
    fi
  fi
  (cd "$MC_DIR" && PANEL_PASSWORD="$PANEL_PASSWORD" python3 panel/panel.py --server "$MC_DIR" --set-password >/dev/null)
fi
BIND="127.0.0.1:$PANEL_PORT"; [[ "${PANEL_PUBLIC:-0}" == "1" ]] && BIND="0.0.0.0:$PANEL_PORT"
chown -R "$MC_USER:$MC_USER" "$MC_DIR"
cat > /etc/systemd/system/minecraft.service <<UNIT
[Unit]
Description=Minecraft server (ServerJar: Pumpkin + web panel)
After=network-online.target
Wants=network-online.target

[Service]
User=$MC_USER
WorkingDirectory=$MC_DIR
ExecStart=/usr/bin/python3 $MC_DIR/panel/panel.py --server $MC_DIR --bind $BIND --autostart
Restart=on-failure
RestartSec=5
KillSignal=SIGINT
TimeoutStopSec=90
Environment=SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

[Install]
WantedBy=multi-user.target
UNIT
if [[ -d /run/systemd/system ]]; then
  systemctl daemon-reload
  systemctl enable minecraft >/dev/null 2>&1
  say "Starting the server"
  systemctl restart minecraft
else
  say "No systemd here; starting the panel in the background instead (log: $MC_DIR/panel.log)"
  pkill -f "$MC_DIR/panel/panel.py" 2>/dev/null || true
  (cd "$MC_DIR" && setsid su -s /bin/bash "$MC_USER" -c "SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt python3 $MC_DIR/panel/panel.py --server $MC_DIR --bind $BIND --autostart" > "$MC_DIR/panel.log" 2>&1 < /dev/null &)
fi

# 5. firewall ---------------------------------------------------------------------------------------
if command -v ufw >/dev/null; then
  ufw allow "$MC_PORT/tcp" >/dev/null 2>&1 || true
  [[ "${PANEL_PUBLIC:-0}" == "1" ]] && ufw allow "$PANEL_PORT/tcp" >/dev/null 2>&1 || true
  if ufw status | grep -q "Status: inactive"; then
    ufw allow OpenSSH >/dev/null 2>&1 || ufw allow 22/tcp >/dev/null 2>&1 || true
    ufw --force enable >/dev/null 2>&1 || true
  fi
fi

# 6. mc command -------------------------------------------------------------------------------------
cat > /usr/local/bin/mc <<'MC'
#!/usr/bin/env bash
# mc: manage the ServerJar Minecraft server installed by install.sh
MC_DIR="${MC_DIR:-/opt/minecraft}"
case "${1:-help}" in
  start|stop|restart) systemctl "$1" minecraft ;;
  status) systemctl status minecraft --no-pager | sed -n 1,6p; echo; ss -ltnp 2>/dev/null | grep -E ':25565|:25566|:8080' || true ;;
  logs) journalctl -u minecraft -f -n 50 ;;
  console) echo "Open the web panel for the live console: mc panel"; journalctl -u minecraft -f -n 30 ;;
  panel) ip=$(curl -4 -fsS --max-time 5 https://ifconfig.me 2>/dev/null || hostname -I | awk '{print $1}')
         echo "Panel: from your PC run   ssh -L 8080:127.0.0.1:8080 root@$ip   then open http://localhost:8080"
         echo "Password: set during install; change with   sudo -u minecraft python3 $MC_DIR/panel/panel.py --server $MC_DIR --set-password" ;;
  config) ${EDITOR:-nano} "$MC_DIR/pumpkin.toml" && echo "restart to apply: mc restart" ;;
  update) curl -fsSL "https://raw.githubusercontent.com/jayclrk358/ServerJar/main/install.sh" | sudo bash ;;
  *) echo "usage: mc start|stop|restart|status|logs|panel|config|update" ;;
esac
MC
chmod 755 /usr/local/bin/mc

# done ----------------------------------------------------------------------------------------------
IP="$(curl -4 -fsS --max-time 5 https://ifconfig.me 2>/dev/null || hostname -I | awk '{print $1}')"
echo
say "Done. Your server is starting."
if [[ "$HAVE_PLUGIN" == "1" ]]; then
  echo "   Players connect to:   $IP:$MC_PORT   (Minecraft 26.3; 26.2 via the multiversion plugin)"
else
  echo "   Players connect to:   $IP:$MC_PORT   (Minecraft 26.3)"
fi
if [[ "${PANEL_PUBLIC:-0}" == "1" ]]; then
  echo "   Web panel:            http://$IP:$PANEL_PORT   (put HTTPS in front before using it from afar)"
else
  echo "   Web panel:            on your PC run  ssh -L $PANEL_PORT:127.0.0.1:$PANEL_PORT root@$IP  then open http://localhost:$PANEL_PORT"
fi
[[ -n "${GENERATED_PW:-}" ]] && echo "   Panel password:       $PANEL_PASSWORD   (change with: sudo -u $MC_USER python3 $MC_DIR/panel/panel.py --server $MC_DIR --set-password)"
echo "   Manage it with:       mc status | mc logs | mc restart | mc config | mc update"
echo "   Files live in:        $MC_DIR  (world, pumpkin.toml, plugins/)"
