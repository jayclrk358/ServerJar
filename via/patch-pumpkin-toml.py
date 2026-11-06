#!/usr/bin/env python3
"""Points server/pumpkin.toml at ViaProxy: Pumpkin listens on localhost only, offline mode,
Velocity forwarding on with the given secret. Section-aware line edits, so comments and the
rest of the file are left alone. Usage: patch-pumpkin-toml.py <pumpkin.toml> <secret> [backend-port]"""
import re
import sys

path, secret = sys.argv[1], sys.argv[2]
port = sys.argv[3] if len(sys.argv) > 3 else "25566"

wanted = {
    "networking.java": {"address": f'"127.0.0.1:{port}"', "online_mode": "false"},
    "networking.proxy": {"enabled": "true"},
    "networking.proxy.velocity": {"enabled": "true", "secret": '"' + secret.replace("\\", "\\\\").replace('"', '\\"') + '"'},
}

lines = open(path, encoding="utf-8").read().split("\n")
out, section, pending = [], None, {}

def flush():
    for key, value in pending.items():
        out.append(f"{key} = {value}")

for line in lines:
    header = re.match(r"^\s*\[([^\]]+)\]\s*$", line)
    if header:
        flush()
        section = header.group(1).strip()
        pending = dict(wanted.get(section, {}))
        out.append(line)
        continue
    kv = re.match(r"^\s*([A-Za-z0-9_]+)\s*=", line)
    if kv and kv.group(1) in pending:
        out.append(f"{kv.group(1)} = {pending.pop(kv.group(1))}")
        continue
    out.append(line)
flush()

for name, keys in wanted.items():
    if f"[{name}]" not in "\n".join(out):
        out += ["", f"[{name}]"] + [f"{k} = {v}" for k, v in keys.items()]

open(path, "w", encoding="utf-8", newline="\n").write("\n".join(out))
print(f"Patched {path}: Pumpkin on 127.0.0.1:{port}, online_mode=false, velocity forwarding on")
