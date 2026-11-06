#!/usr/bin/env bash
# Starts ViaProxy in headless mode from via/run (config, plugins and logs live there).
# Start Pumpkin first (server/start.sh); ViaProxy forwards players to it.
cd "$(dirname "$0")/run" || { echo "via/run not found: run via/setup.sh first" >&2; exit 1; }
JAR=$(ls ViaProxy-*.jar 2>/dev/null | head -n1)
[[ -n "$JAR" ]] || { echo "No ViaProxy jar in via/run: run via/setup.sh first" >&2; exit 1; }
# ViaProxy needs little memory for a small server; raise -Xmx for 50+ players.
exec java -Xms64M -Xmx512M -XX:+UseSerialGC -DskipUpdateCheck=true -jar "$JAR" cli
