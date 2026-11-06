# ServerJar

A very lightweight, fast-starting Minecraft Java server, built on
[Pumpkin](https://github.com/Pumpkin-MC/Pumpkin), a Minecraft server written from scratch in Rust.
This repo adds performance patches and a tuned low-resource config on top of it.

> It's a native program (`pumpkin.exe`), not a `.jar`: there's no JVM. That's where most of the
> startup and memory savings come from.

| | Vanilla Java server | This server |
|---|---|---|
| Startup | ~15 s | **~0.14 s** |
| Idle RAM (world loaded) | ~1.5–2 GB | **~22–28 MB** |
| 100 walking players | – | 20 TPS, ~1.7 cores, ~600 MB peak |

Target: **Minecraft Java 26.3** (protocol 777). Other client versions can join through the
[`multiversion` plugin](plugins/multiversion/README.md) (26.2 so far, more versions as translators
are added). Plugin compatibility (Bukkit/Paper) isn't a goal yet.

## What's in here

| Path | What it is |
|---|---|
| `build.ps1` / `build.sh` | Windows / Linux build: clones Pumpkin at a pinned commit, applies `patches/`, downloads the vanilla jar for game data, builds. |
| `patches/` | Our changes to Pumpkin, applied in order. |
| `server/pumpkin.toml` | Low-resource config (see below). |
| `server/start.bat` / `start.sh` | Starts the server. |
| `server/pumpkin.service` | systemd unit for Linux. |
| `plugins/multiversion/` | **Pumpkin plugin** (WebAssembly, Rust) that lets other Minecraft versions join: an in-process protocol-translating proxy with Mojang auth and identity forwarding. Drop in `multiversion.wasm`, apply the three `pumpkin.toml` settings it writes out, done. See [`plugins/multiversion/README.md`](plugins/multiversion/README.md). |
| `.github/workflows/release.yml` | Builds the Linux and Windows release downloads and `multiversion.wasm` on GitHub. |

## Download (no build needed)

Ready-made servers are on the [Releases](https://github.com/jayclrk358/ServerJar/releases) page.
It's one file, like a server jar but without needing Java.

**Linux VPS** (x86-64, Ubuntu 22.04+ / Debian 12+):

```bash
mkdir -p ~/mc && cd ~/mc
curl -L https://github.com/jayclrk358/ServerJar/releases/latest/download/ServerJar-linux-x86_64.tar.gz | tar -xz --strip-components=1
./start.sh
```

Open TCP 25565 in the firewall (`sudo ufw allow 25565/tcp`, plus your provider's panel), then
connect with a Minecraft Java 26.3 client. To run it as a service, edit the user and paths in
`pumpkin.service` (they should point at `~/mc`), copy it to `/etc/systemd/system/` and run
`sudo systemctl enable --now pumpkin`.

**Windows:** download `ServerJar-windows-x86_64.zip`, unzip it, and run `start.bat`.

To update, download the new release over the old `pumpkin` / `pumpkin.exe` and keep your
`pumpkin.toml` and `world` folder. Back up `world` first.

Release builds target x86-64-v3 CPUs (AVX2, roughly 2015 or newer). They're built by GitHub
Actions (`.github/workflows/release.yml`) whenever a `v*` tag is pushed.

## Build from source

Only needed if you want to change the patches, or tune the build to one specific CPU.
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

## Letting other Minecraft versions join

Pumpkin only speaks 26.3. The `multiversion` plugin handles the rest by itself: it listens on the
public port, speaks the client's own version, verifies the player with Mojang, logs into Pumpkin on a
localhost port and forwards the real UUID, skin and IP with a signed Velocity handshake. Supported
today: 26.3 (native) and 26.2 (translated); older versions are added one translator at a time.

```bash
cp multiversion.wasm server/plugins/      # from the release, or ./plugins/multiversion/build.sh
./server/start.sh                          # first start writes plugins/data/multiversion/config.toml and
                                           # pumpkin-settings.txt: paste those settings into pumpkin.toml
                                           # (moves Pumpkin to 127.0.0.1:25566), then restart
```

`./plugins/multiversion/install.sh` does the copy and the `pumpkin.toml` edit for you if you prefer.
Players connect to 25565 as before; only that port needs to be open.

## Patches

| Patch | Why |
|---|---|
| `0001-answer-ping-and-keep-alive-on-network-thread` | Pumpkin queued every in-game packet, including ping requests and keep-alives, until the next 50 ms tick. That added 0–50 ms (avg ~25 ms) to every player's ping, including the tab-list ping. Vanilla answers these on the network thread; this patch does the same. Skipped when a plugin listens for packets, so plugin events are unchanged. |

Measured with 100 walking bot players (60 s hold, same PC, localhost):

| | Before 0001 | After 0001 |
|---|---|---|
| In-game ping p50 | 31.2 ms | **0.21 ms** |
| In-game ping p99 | 59.8 ms | **3.1 ms** |
| In-game ping max | 125 ms | **5.2 ms** |
| MSPT avg | 14.4 ms | 12.2 ms |
| RAM peak | 610 MB | 598 MB |
| TPS | 20.0 | 20.0 |

| Patch | Why |
|---|---|
| `0002-keep-serial-entity-schedule-through-small-count-changes` | Pumpkin tries to tick separate groups ("islands") of entities in parallel, and backs off for 20 ticks when a plan isn't worth it. But it dropped that back-off whenever the entity count changed, which with players around is almost every tick, as mobs spawn and despawn. Profiling with 100 players showed it re-planning on ~35% of ticks and throwing every plan away (partitioning ran out of its budget each time). Now only a change of more than 1/8 of the count cuts the back-off short. Tick order is unchanged: those ticks ran serially anyway. Covered by Pumpkin's unit tests; not yet measured under load. |

### Where tick time goes (for future patches)

Profiled with 100 players spread over ~1,800 active chunks. Overworld tick, without LTO:

| Phase | 65 entities | ~900 entities |
|---|---|---|
| Entity ticking | 0.8 ms | **14.5 ms** |
| Player ticking | 2.6 ms | 7.9 ms |
| Chunk ticking | 0.2 ms | 4.1 ms |
| Entity tracker | 0.1 ms | 2.4 ms |

Entity ticking runs on a single thread: no parallel plan ever succeeded. Only sheep, pigs, cows
and chickens have audited tick reach. Every other mob gets a conservative reach of ~32–50 blocks,
so the islands merge and planning runs out of budget. Auditing the common hostile mobs (skeleton,
creeper, zombie, enderman, spider) is the big remaining win, but it has to follow Pumpkin's
vanilla-parity rules for entity islands (see `AGENTS.md` in Pumpkin).

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
