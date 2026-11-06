# Multi-version support (ViaProxy + PumpkinForward)

Pumpkin speaks exactly one protocol version (Minecraft 26.3, protocol 777). Its plugin system is a
WASM sandbox for gameplay logic and can't rewrite the network protocol, so letting 1.8 or 1.21
clients join needs a translating proxy in front of the server. This folder sets that up:

```
players (1.7.2 … 26.3)  ──►  ViaProxy :25565  ──►  Pumpkin 127.0.0.1:25566
                              │ translates protocol
                              │ verifies players with Mojang
                              └ PumpkinForward plugin passes UUID / skin / IP to Pumpkin
```

- **[ViaProxy](https://github.com/ViaVersion/ViaProxy)** (ViaVersion's standalone proxy) accepts
  clients from **1.7.2 up to 26.3** and talks 26.3 to Pumpkin. Pinned in `viaproxy.env`.
- **PumpkinForward** (`plugin/`) is a small ViaProxy plugin written for this repo. ViaProxy on its own
  can only hand players to an *offline-mode* backend, which would give everyone a name-based UUID
  and no skin, and would let anyone join under any name if the backend port leaked. The plugin
  answers Pumpkin's Velocity `velocity:player_info` login request with the profile ViaProxy just
  verified against Mojang (real UUID, skin textures, client IP), signed with a shared secret.
  Pumpkin's built-in Velocity forwarding support verifies the signature and uses that identity, so
  player data keeps the same UUIDs as a direct online-mode server.

## Setup

Needs Java 17 or newer on the server (`sudo apt install openjdk-21-jre-headless`). Nothing else
changes: Pumpkin stays the native binary.

```bash
./via/setup.sh            # Linux; Windows: powershell -ExecutionPolicy Bypass -File via\setup.ps1
./server/start.sh         # Pumpkin, now on 127.0.0.1:25566
./via/start-via.sh        # ViaProxy on 0.0.0.0:25565
```

`setup.sh` downloads the pinned ViaProxy jar (sha256-checked) into `via/run/`, installs the plugin
(prebuilt jar from the release, or builds it with `javac` if a JDK is present), generates the shared
secret, writes `via/run/viaproxy.yml`, and edits `server/pumpkin.toml`:

| Key | Before | After |
|---|---|---|
| `networking.java.address` | `0.0.0.0:25565` | `127.0.0.1:25566` (localhost only) |
| `networking.java.online_mode` | `true` | `false` (ViaProxy does the Mojang check) |
| `networking.proxy.enabled` | `false` | `true` |
| `networking.proxy.velocity.enabled` / `secret` | off | on, shared with the plugin |

A copy of the old config is kept as `server/pumpkin.toml.before-via`. Only port 25565 needs to be
open in the firewall; keep 25566 closed so nobody can bypass the proxy.

For systemd, use `viaproxy.service` next to `server/pumpkin.service` (edit user and paths).
Re-running `setup.sh` is safe: it keeps the existing secret and `viaproxy.yml`.

### Options

- `./via/setup.sh --port 25565 --backend-port 25566` to pick other ports.
- `via/run/viaproxy.yml`: ViaProxy's own settings (see its comments after first start). Keep
  `proxy-online-mode: true` and `auth-method: none`.
- The plugin's secret lives in `via/run/plugins/PumpkinForward/secret.txt`, or in the
  `PUMPKIN_FORWARD_SECRET` environment variable. It has to match `networking.proxy.velocity.secret`.

## What to expect

- Memory: ViaProxy is a JVM process. Expect roughly 150–300 MB on top of Pumpkin's ~25 MB, so the
  whole stack is still far below a vanilla Java server. `start-via.sh` caps it at 512 MB.
- Latency: one extra local hop plus translation work, well under a millisecond on the same machine.
- Older clients get what ViaVersion/ViaBackwards/ViaRewind can emulate: new blocks, items and mobs
  are shown as the closest thing the old client knows. Chat signing, transfers and cookies are handled
  by ViaProxy.
- The server list shows Pumpkin's MOTD; ViaProxy reports a protocol the pinging client accepts.

## Updating

`viaproxy.env` pins the ViaProxy version and jar hash. To move to a newer ViaProxy (for a new
Minecraft version, together with a Pumpkin bump in `build.sh`), change the version there, download
the jar once, paste its `sha256sum`, and update `target-version` in `viaproxy.yml`.

## Developing the plugin

```
via/plugin/
  src/main/java/serverjar/pumpkinforward/PumpkinForwardPlugin.java   plugin entry: loads the secret, hooks the backend pipeline
  src/main/java/serverjar/pumpkinforward/VelocityForwardHandler.java netty handler: answers velocity:player_info
  src/main/resources/viaproxy.yml                                     ViaProxy plugin manifest
  src/test/                                                           encoder dump + Python check against Pumpkin's parser
  build.sh / build.ps1                                                javac build, no Gradle
```

The handler sits in ViaProxy's proxy→server pipeline *before* the protocol translator, so it sees
Pumpkin's login plugin request as raw 26.3 bytes regardless of the client's version (for pre-1.13
clients the translator would otherwise answer "not understood" and Pumpkin would kick). Login packet
IDs it relies on (`custom_query` 0x04 clientbound, `custom_query_answer` 0x02 serverbound) have been
stable since 1.13 and are taken from Pumpkin's `assets/packets.json`.

Test the wire format without a server:

```bash
./via/plugin/build.sh
javac -proc:none --release 17 -cp via/plugin/build/ViaProxy-3.4.14.jar:via/plugin/build/classes \
  -d via/plugin/build/test-classes via/plugin/src/test/java/serverjar/pumpkinforward/EncodeDump.java
java -cp via/plugin/build/ViaProxy-3.4.14.jar:via/plugin/build/classes:via/plugin/build/test-classes \
  serverjar.pumpkinforward.EncodeDump | python3 via/plugin/src/test/check.py
```

The plugin is GPL-3.0 like the rest of this repo. ViaProxy is GPL-3.0 and is downloaded, not
redistributed, by the setup scripts.
