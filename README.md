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
| `build.ps1` | Clones Pumpkin at a pinned commit, applies `patches/`, downloads the vanilla jar for game data, builds. |
| `patches/` | Our changes to Pumpkin, applied in order. |
| `server/pumpkin.toml` | Low-resource config (see below). |
| `server/start.bat` | Starts the server. |

## Build

Needs Git, [Rust](https://rustup.rs) and the Visual Studio C++ build tools (Windows).

```powershell
powershell -ExecutionPolicy Bypass -File build.ps1
```

The first build takes ~25 minutes (fat LTO). Use `-Portable` if the binary will run on a different
machine; by default it's tuned for the CPU it's built on (`-C target-cpu=native`).

Then run `server\start.bat` and connect to `localhost` with a 26.3 client.

## Patches

| Patch | Why |
|---|---|
| `0001-answer-ping-and-keep-alive-on-network-thread` | Pumpkin queued every in-game packet, including ping requests and keep-alives, until the next 50 ms tick. That added 0–50 ms (avg ~25 ms) to every player's ping, including the tab-list ping. Vanilla answers these on the network thread; this patch does the same. Skipped when a plugin listens for packets, so plugin events are unchanged. |

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
