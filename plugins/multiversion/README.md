# multiversion: a Pumpkin plugin that lets other Minecraft versions join

`multiversion.wasm` is a Pumpkin plugin. Once installed, players on other Minecraft Java versions
can join your 26.3 server: the plugin speaks their protocol version and translates every packet to
and from Pumpkin's. It needs no Java, no separate proxy process and no extra port to open. Everything
runs inside the server, as a WebAssembly plugin, configured by one `config.toml`.

| Client version | Status |
|---|---|
| 26.3 (protocol 777) | native, passed through untouched |
| 26.2 (protocol 776) | translated |
| 26.1 (protocol 775) | translated |
| 1.21.11 (protocol 774) | translated |
| 1.21.9 / 1.21.10 (protocol 773) | translated |
| 1.21.7 / 1.21.8 (protocol 772) | translated |
| 1.21.6 (protocol 771) | translated |
| 1.21.5 (protocol 770) | translated |
| 1.21.4 (protocol 769) | translated |
| 1.21.2 / 1.21.3 (protocol 768) | translated |
| 1.21 / 1.21.1 (protocol 767) | translated |

Content an older client doesn't have shows as its nearest equivalent: new mobs as similar old mobs,
new items and blocks as similar old ones. Translation has been checked with unit tests on hand-built
packets; report anything that disconnects a real client.
| 26.1 and older | not yet: each version step is its own translator in `src/translate/` |

## How it works

Pumpkin only implements one protocol version and its plugin API has no hook for rewriting the
network protocol. What it does have is a WASI sandbox with real TCP sockets, so the plugin runs a
small proxy inside the server process:

```
player (26.2) ──► plugin listener :25565 ──► translator chain ──► Pumpkin 127.0.0.1:25566 (26.3)
                   │ handshake / status / login in the client's version
                   │ Mojang session check (online mode)
                   └ real UUID, skin and IP handed to Pumpkin via Velocity forwarding (HMAC-signed)
```

1. **Listener.** On load the plugin binds the public port (`[listen].address`) and Pumpkin itself
   is moved to a localhost-only port. The proxy loop runs in a scheduled task, so it never touches
   the tick thread.
2. **Login.** The plugin does the whole login with the client in the client's own version:
   encryption request, session check with Mojang (through WASI HTTP), compression. It then logs
   into Pumpkin as a 26.3 client and answers Pumpkin's `velocity:player_info` request with the
   verified profile, signed with the shared secret. Pumpkin's built-in Velocity forwarding verifies
   it, so players keep their real UUID, skin and IP and nobody can forge an identity.
3. **Translation.** Configuration and play packets go through a chain of per-version translators
   (26.3→26.2, later 26.2→26.1, ...), each converting one version step like ViaVersion's protocol
   pipeline. Registry id tables (blocks, items, entities, sounds, particles, data components,
   argument types) are generated from ViaVersion's mapping files; structural changes come from
   reading ViaBackwards' equivalent protocol.

## Install

Requirements: a Pumpkin 26.3 server (built from commit `d1190dc`, plugin API v0.1). Nothing else: no Java, no proxy, no extra process.

1. Copy `multiversion.wasm` (from the [latest release](https://github.com/jayclrk358/Multiversion/releases/latest), or built with `build.sh`) into your server's `plugins/` folder.
2. Start the server once. The plugin writes `plugins/data/multiversion/config.toml` with a
   fresh forwarding secret, and `pumpkin-settings.txt` next to it with the three `pumpkin.toml`
   settings it needs (Pumpkin only lets plugins write their own data folder, so this edit is the
   one thing it cannot do itself):
   - `networking.java.address = "127.0.0.1:25566"`: Pumpkin moves to a localhost port and the
     plugin takes over 25565;
   - `networking.proxy.enabled = true` and `networking.proxy.velocity` enabled with the secret:
     Pumpkin accepts the signed identity the plugin forwards;
   - `plugins.enabled = true` with the plugin's permissions pre-approved.
   Paste them into `pumpkin.toml` and restart. If you skip this, the plugin tells you on the console
   exactly which setting is missing (port still held by Pumpkin, or forwarding not enabled).
3. Players keep connecting to port 25565. Keep 25566 closed in the firewall.

`online_mode` in `pumpkin.toml` can stay `true`: with forwarding enabled, Pumpkin trusts the
plugin's signed profile instead of asking Mojang itself.

### Permissions

The plugin asks Pumpkin for `network.tcp` (listen + connect to Pumpkin), `http.outbound` (Mojang
session server), `fs.read.data` and `fs.write.data` (its config). The `pumpkin-settings.txt` snippet
pre-approves them in `[plugins.overrides.multiversion]`; otherwise Pumpkin asks on the console at first load.

## Configuration (`plugins/data/multiversion/config.toml`)

```toml
[listen]
address = "0.0.0.0:25565"          # public port players use

[backend]
address = "127.0.0.1:25566"        # Pumpkin's networking.java.address
forwarding_secret = "..."          # = networking.proxy.velocity.secret

[auth]
online_mode = true                 # verify players with Mojang
session_server = "https://sessionserver.mojang.com"
prevent_proxy_connections = false  # pass the client IP to Mojang

[status]
version_name = ""                  # server-list text; empty = the allowed range, e.g. "1.21-26.3"

[versions]
allow = ["*"]                      # default: every version the plugin can translate
deny = []                          # refused even if allowed above
# Entries: a version "1.21.4", a family "1.21.*", or a range "1.21.5-26.3" (either end optional).
# Patch releases that share a protocol count as one: 1.21.1 = 1.21, 1.21.3 = 1.21.2,
# 1.21.8 = 1.21.7, 1.21.10 = 1.21.9. Example: allow = ["1.21.*", "26.3"], deny = ["1.21"].
kick_unsupported_message = "This server supports Minecraft {versions}. You are on {client}."

[limits]
compression_threshold = 256
max_connections = 200
login_timeout_secs = 30
```

Clients on versions without a translator see the server as incompatible in the server list and get
the kick message if they try anyway.

## What's translated for 26.2 clients

Done, following ViaBackwards' 26.3→26.2 protocol:

- packet id remapping for play, configuration and serverbound packets (26.3 added
  `add_transient_block`, `post_effects`, `swing_animation`, `punch`, `spectator_action`);
- chunks (block state ids in indirect and direct palettes, 16→15 bit repacking), block updates,
  multi-block updates, block events, level events, light data (bit sets), block entities (pot
  sherds);
- registry data (removed registries dropped, trim materials, environment attributes, enchantment
  effects), tags (block/item/entity ids, renamed tag), commands (argument type ids and new parsers);
- items and every data component (ids inside remapped; 13 new components dropped or converted;
  swing animation split/merge; pot decorations; trims; instruments), in inventories, equipment,
  merchant offers, recipes and the recipe book, advancements, creative slots and hashed clicks;
- entities: types (cushion → falling block), spawn data, entity metadata (items, block states,
  particles, removed dye-colour type), movement packets (multi-step deltas → single delta),
  position sync, login/respawn game modes, arm swings/animations;
- particles (new leaf particles, speed/randomization change), sounds, explosions, maps,
  sign editor, chat filter masks, brewing stand data, player actions and teleport acks.

Known gaps: text components inside NBT (hover "show item") are not rewritten; 26.3-only blocks
and items show as their nearest 26.2 equivalent (poplar as birch, wool stairs as other stairs);
cushions appear as carpet blocks; new enchantment effect definitions are not shown.

## Build from source

```bash
./build.sh          # needs Rust; adds the wasm32-wasip2 target itself (.\build.ps1 on Windows)
cargo test --release   # native unit tests
```

### Layout

```
src/lib.rs            plugin entry (metadata, on_load, scheduled proxy task)
src/config.rs         config.toml
src/net/reactor.rs    poll-based reactor over WASI 0.2 TCP sockets
src/proto/            varint/strings, NBT, framing (zlib, AES-CFB8), version table
src/crypto.rs         RSA key pair, server hash, HMAC
src/auth.rs           Mojang hasJoined over WASI HTTP
src/conn.rs           per-connection state machine (handshake → status/login → play relay)
src/proxy.rs          reactor handler: sockets ↔ connections, timeouts
src/translate/        Translator trait + chain; v26_2/ = 26.3 ↔ 26.2 step
tools/gen_mappings.py regenerates src/translate/data_26_2.rs from ViaVersion's mapping files
```

### Adding the next version step

1. Clone https://github.com/ViaVersion/Mappings and run
   `tools/gen_mappings.py <repo> 26.2 26.1 src/translate/data_26_1.rs`.
2. Create `src/translate/v26_1/` after the pattern of `v26_2/`, using ViaBackwards'
   `Protocol26_2To26_1` as the list of what changed.
3. Register it in `translate::make_step` and add the version to `proto::version::KNOWN`.

## Testing

`cargo test` covers codecs, framing, crypto vectors and translator pieces. The full path was
exercised against a debug Pumpkin build with a scripted client (`status`, offline login, and an
online-mode login with RSA/AES against a stand-in session server) for both 26.3 and 26.2: login,
configuration, and the first few hundred play packets translate without errors. A real 26.2 client
has not been connected yet; see the main README for status.

## Licence

GPL-3.0 (same as this repo and as ViaVersion, whose generated mapping data this plugin embeds).
