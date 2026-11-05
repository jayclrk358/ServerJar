#!/usr/bin/env bash
# Linux/macOS build: same steps as build.ps1.
#   1. clones Pumpkin at the pinned commit (first run only)
#   2. applies everything in patches/
#   3. downloads the vanilla server jar Pumpkin needs for its game data (hash-checked)
#   4. builds a release binary tuned for this CPU and copies it to server/
#
# Usage: ./build.sh [--portable | --cpu NAME]
#   --portable   build for any CPU of this architecture instead of this machine's
#   --cpu NAME   build for a specific CPU level, e.g. x86-64-v3
set -euo pipefail

PUMPKIN_REPO="${PUMPKIN_REPO:-https://github.com/Pumpkin-MC/Pumpkin.git}"
PUMPKIN_COMMIT="${PUMPKIN_COMMIT:-d1190dc51edc3d4447780017e244f8bc882cb81a}"
MC_VERSION="${MC_VERSION:-26.3}"
CPU="native"
case "${1:-}" in
  --portable) CPU="" ;;
  --cpu) CPU="${2:?--cpu needs a CPU name}" ;;
  "") ;;
  *) echo "Unknown option: $1" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "$0")" && pwd)"
SRC="$ROOT/Pumpkin"
export PATH="$HOME/.cargo/bin:$PATH"

for tool in git cargo curl python3; do
  command -v "$tool" >/dev/null || { echo "$tool not found. Install it first (Rust: https://rustup.rs)." >&2; exit 1; }
done

# 1. source
if [[ ! -d "$SRC" ]]; then
  echo "Cloning Pumpkin..."
  git clone --recurse-submodules "$PUMPKIN_REPO" "$SRC"
  git -C "$SRC" checkout --quiet "$PUMPKIN_COMMIT"
  git -C "$SRC" submodule update --init --recursive
fi

# 2. patches (skip ones that are already applied)
for p in "$ROOT"/patches/*.patch; do
  [[ -e "$p" ]] || continue
  if git -C "$SRC" apply --reverse --check "$p" 2>/dev/null; then
    echo "Already applied: $(basename "$p")"
  else
    echo "Applying: $(basename "$p")"
    git -C "$SRC" apply --whitespace=nowarn "$p"
  fi
done

# 3. vanilla server jar
mkdir -p "$ROOT/vanilla"
JAR="$ROOT/vanilla/server-$MC_VERSION.jar"
read -r URL SHA1 < <(python3 - "$MC_VERSION" <<'PY'
import json, sys, urllib.request
ver = sys.argv[1]
m = json.load(urllib.request.urlopen("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"))
e = next((v for v in m["versions"] if v["id"] == ver), None)
if e is None:
    sys.exit(f"Minecraft {ver} not found in Mojang's version manifest")
s = json.load(urllib.request.urlopen(e["url"]))["downloads"]["server"]
print(s["url"], s["sha1"])
PY
)
if [[ ! -f "$JAR" ]] || [[ "$(sha1sum "$JAR" | cut -d' ' -f1)" != "$SHA1" ]]; then
  echo "Downloading vanilla $MC_VERSION server jar..."
  curl -L --retry 5 --fail -o "$JAR" "$URL"
  [[ "$(sha1sum "$JAR" | cut -d' ' -f1)" == "$SHA1" ]] || { echo "Vanilla jar hash mismatch" >&2; exit 1; }
fi
export PUMPKIN_MINECRAFT_SERVER_JAR="$JAR"

# 4. build
if [[ -n "$CPU" ]]; then export RUSTFLAGS="-C target-cpu=$CPU"; else export RUSTFLAGS=""; fi
echo "Building (release, LTO; this takes a while)..."
(cd "$SRC" && cargo build --release -p pumpkin)

mkdir -p "$ROOT/server"
cp "$SRC/target/release/pumpkin" "$ROOT/server/pumpkin"
echo "Done: server/pumpkin  (start it with server/start.sh)"
