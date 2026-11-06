//! Plugin configuration: `plugins/multiversion/config.toml`.
//!
//! A default file is written on first start so the server owner only has to fill in the
//! forwarding secret and (optionally) change ports.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: Listen,
    pub backend: Backend,
    pub auth: Auth,
    pub status: Status,
    pub versions: Versions,
    pub limits: Limits,
}

/// Where players connect. This is the port you open in the firewall.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Listen {
    pub address: String,
}

/// The Pumpkin listener this plugin forwards to. Must be `networking.java.address` in
/// pumpkin.toml, bound to localhost, with velocity forwarding enabled.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Backend {
    pub address: String,
    /// Shared secret: must equal `networking.proxy.velocity.secret` in pumpkin.toml.
    pub forwarding_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Auth {
    /// Verify players with Mojang. Keep this on for a public server.
    pub online_mode: bool,
    /// Session server used for `hasJoined`. Change only for alternative auth servers.
    pub session_server: String,
    /// Reject players that connect through a proxy Mojang reports for their IP.
    pub prevent_proxy_connections: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Status {
    /// Version name shown in the server list. Empty = "<lowest>-<highest>".
    pub version_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Versions {
    /// Client versions allowed to join, by name (e.g. "26.2"). Empty = every supported version.
    pub allow: Vec<String>,
    pub kick_unsupported_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Packets to clients larger than this are zlib-compressed. -1 disables compression.
    pub compression_threshold: i32,
    pub max_connections: usize,
    /// Seconds a connection may sit in handshake/login before it is dropped.
    pub login_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: Listen::default(),
            backend: Backend::default(),
            auth: Auth::default(),
            status: Status::default(),
            versions: Versions::default(),
            limits: Limits::default(),
        }
    }
}
impl Default for Listen {
    fn default() -> Self {
        Self { address: "0.0.0.0:25565".into() }
    }
}
impl Default for Backend {
    fn default() -> Self {
        Self { address: "127.0.0.1:25566".into(), forwarding_secret: String::new() }
    }
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            online_mode: true,
            session_server: "https://sessionserver.mojang.com".into(),
            prevent_proxy_connections: false,
        }
    }
}
impl Default for Status {
    fn default() -> Self {
        Self { version_name: String::new() }
    }
}
impl Default for Versions {
    fn default() -> Self {
        Self {
            allow: Vec::new(),
            kick_unsupported_message: "This server supports Minecraft {versions}. You are on {client}.".into(),
        }
    }
}
impl Default for Limits {
    fn default() -> Self {
        Self { compression_threshold: 256, max_connections: 200, login_timeout_secs: 30 }
    }
}

pub const FILE_HEADER: &str = "# multiversion: lets other Minecraft versions join this Pumpkin server.\n\
# Players connect to [listen].address; the plugin translates their protocol and forwards them to\n\
# Pumpkin at [backend].address. In pumpkin.toml set:\n\
#   [networking.java]            address = \"127.0.0.1:25566\"   online_mode = false\n\
#   [networking.proxy]           enabled = true\n\
#   [networking.proxy.velocity]  enabled = true   secret = \"<same as forwarding_secret below>\"\n\n";

impl Config {
    pub fn load_or_create(path: &str) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{path}: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut cfg = Self::default();
                cfg.backend.forwarding_secret = crate::crypto::random_token();
                let body = toml::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
                std::fs::write(path, format!("{FILE_HEADER}{body}")).map_err(|e| format!("{path}: {e}"))?;
                Ok(cfg)
            }
            Err(e) => Err(format!("{path}: {e}")),
        }
    }
}
