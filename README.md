# ServerJar

A very lightweight, fast-starting Minecraft Java server, built on
[Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), a Minecraft server written from scratch in Rust.
This repo adds performance patches, a tuned low-resource config on top of it.

> It's a native program (`pumpkin.exe`), not a `.jar`: there's no JVM. That's where most of the
> startup and memory savings come from.

| | Vanilla Java server | This server |
|---|---|---|
| Startup | ~15 s | **~0.14 s** |
| Idle RAM (world loaded) | ~1.5–2 GB | **~22–28 MB** |
| 100 walking players | – | 20 TPS, ~1.7 cores, ~600 MB peak |

Target: **Minecraft Java 26.3** (protocol 777). Plugin compatibility (Bukkit/Paper) isn't a goal yet.

## What's in here

| Path | What it is |
|---|---|
| `build.ps1` / `build.sh` | Windows / Linux build: clones Pumpkin at a pinned commit, applies `patches/`, downloads the vanilla jar for game data, builds. |
| `patches/` | Our changes to Pumpkin, applied in order. |
| `server/pumpkin.toml` | Low-resource config (see below). |
| `server/start.bat` / `start.sh` | Starts the server. |
| `server/pumpkin.service` | systemd unit for Linux. |

## Build

Needs Git, [Rust](https://rustup.rs) and the Visual Studio C++ build tools (Windows).

```powershell
powershell -ExecutionPolicy Bypass -File build.ps1
```

The first build takes ~25 minutes (fat LTO). Use `-Portable` if the binary will run on a different
machine; by default it's tuned for the CPU it's built on (`-C target-cpu=native`).

Then run `server\start.bat` and connect to `localhost` with a 26.3 client.

### Linux (e.g. a VPS)

```bash
sudo apt install -y git build-essential pkg-config curl python3
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && source ~/.cargo/env
git clone https://github.com/jayclrk358/ServerJar.git && cd ServerJar
chmod +x build.sh server/start.sh && ./build.sh
./server/start.sh
```

- The final LTO link can use 3–6 GB of RAM. On a small VPS, add swap before building, or build on
  a bigger Linux machine with `./build.sh --portable` and copy `server/pumpkin` over.
- To run it as a service (starts on boot, restarts on crash), edit the user and paths in
  `server/pumpkin.service`, then copy it to `/etc/systemd/system/` and
  `sudo systemctl enable --now pumpkin`.
- Open TCP 25565 in the firewall (`sudo ufw allow 25565/tcp` and your provider's panel).

The Linux scripts haven't been tested on a real Linux machine yet.

## Patches

| Patch | Why |
|---|---|
| `0001-answer-ping-and-keep-alive-on-network-thread` | Pumpkin queued every in-game packet, including ping requests and keep-alives, until the next 50 ms tick. That added 0–50 ms (avg ~25 ms) to every player's ping, including the tab-list ping. Vanilla answers these on the network thread; this patch does the same. Skipped when a plugin listens for packets, so plugin events are unchanged. |

Measured with 100 walking bot players ( 60 s hold, same PC, localhost):

| | Before 0001 | After 0001 |
|---|---|---|
| In-game ping p50 | 31.2 ms | **0.21 ms** |
| In-game ping p99 | 59.8 ms | **3.1 ms** |
| In-game ping max | 125 ms | **5.2 ms** |
| MSPT avg | 14.4 ms | 12.2 ms |
| RAM peak | 610 MB | 598 MB |
| TPS | 20.0 | 20.0 |

## Config changes (`server/pumpkin.toml`)

Compared with Pumpkin's defaults:

- View distance 16 → 8, simulation distance 10 → 6 (the biggest RAM/CPU lever with players online)
- Bedrock listener, plugin engine and telemetry off
- Max players 1000 → 20
- Network compression level 4 → 1, threshold 256 → 512 bytes
- Autosave every 10 minutes instead of 5
- April Fools content off

## License

GPL-3.0, same as Pumpkin, since the patches modify Pumpkin's code. See [LICENSE](LICENSE).
Minecraft is a trademark of Mojang; this project isn't affiliated with Mojang or Microsoft.
