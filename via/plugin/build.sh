#!/usr/bin/env bash
# Builds the PumpkinForward ViaProxy plugin with plain javac (no Gradle needed).
# Needs a JDK 17+ and the ViaProxy jar to compile against (downloaded if missing).
#   ./build.sh [path/to/ViaProxy.jar]
# Output: build/PumpkinForward.jar
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
source "$HERE/../viaproxy.env"
VIAPROXY_JAR="${1:-$HERE/build/ViaProxy-$VIAPROXY_VERSION.jar}"

command -v javac >/dev/null || { echo "javac not found: install a JDK 17 or newer" >&2; exit 1; }
mkdir -p "$HERE/build/classes"

if [[ ! -f "$VIAPROXY_JAR" ]]; then
  echo "Downloading ViaProxy $VIAPROXY_VERSION (compile dependency)..."
  curl -L --retry 5 --fail -o "$VIAPROXY_JAR" "$VIAPROXY_URL"
fi
[[ "$(sha256sum "$VIAPROXY_JAR" | cut -d' ' -f1)" == "$VIAPROXY_SHA256" ]] || { echo "ViaProxy jar hash mismatch" >&2; exit 1; }

rm -rf "$HERE/build/classes"/*
javac --release 17 -proc:none -Xlint:all -Werror -cp "$VIAPROXY_JAR" -d "$HERE/build/classes" \
  $(find "$HERE/src/main/java" -name '*.java')
cp "$HERE/src/main/resources/viaproxy.yml" "$HERE/build/classes/"
jar --create --file "$HERE/build/PumpkinForward.jar" -C "$HERE/build/classes" .
echo "Built: $HERE/build/PumpkinForward.jar"
