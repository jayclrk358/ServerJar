//! Plugin configuration: `plugins/data/multiversion/config.toml`.
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
    /// Client versions allowed to join. Empty = every version the plugin can translate.
    /// Entries: a version ("26.2"), a family ("1.21.*") or a range ("1.21.5-26.3").
    pub allow: Vec<String>,
    /// Versions refused even if `allow` would admit them. Same syntax as `allow`.
    pub deny: Vec<String>,
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
            allow: vec!["*".into()],
            deny: Vec::new(),
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
#   [networking.proxy.velocity]  enabled = true   secret = \"<same as forwarding_secret below>\"\n\
#\n\
# [versions] chooses who may join. Entries are a version (\"1.21.4\"), a family (\"1.21.*\") or a\n\
# range (\"1.21.5-26.3\"). allow = [\"*\"] (the default) or [] means every version the plugin can\n\
# translate; deny wins over\n\
# allow. Versions that share a protocol count as one: 1.21.1 = 1.21, 1.21.3 = 1.21.2, 1.21.8 = 1.21.7,\n\
# 1.21.10 = 1.21.9. Example: allow = [\"1.21.*\", \"26.3\"], deny = [\"1.21\"].\n\n";

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

/// The pumpkin.toml settings this plugin needs, ready to paste. Pumpkin's `online_mode` can stay
/// on: with proxy forwarding enabled Pumpkin trusts the signed profile instead of asking Mojang.
pub fn pumpkin_snippet(cfg: &Config) -> String {
    format!(
        "# multiversion needs these three things in pumpkin.toml (restart the server afterwards):\n\
         #\n\
         # 1. Pumpkin moves to a localhost port; the plugin takes over {public}.\n\
         [networking.java]\n\
         address = \"{backend}\"\n\
         \n\
         # 2. Pumpkin accepts the player identity the plugin forwards (signed with the shared secret).\n\
         [networking.proxy]\n\
         enabled = true\n\
         \n\
         [networking.proxy.velocity]\n\
         enabled = true\n\
         secret = \"{secret}\"\n\
         \n\
         # 3. Plugins on, with this plugin's permissions pre-approved (or answer the console prompt).\n\
         [plugins]\n\
         enabled = true\n\
         \n\
         [plugins.overrides.multiversion]\n\
         allowed_permissions = [\"network.tcp\", \"http.outbound\", \"fs.read.data\", \"fs.write.data\"]\n\
         \n\
         # Keep {backend_port} closed in your firewall; only {public} needs to be open.\n",
        public = cfg.listen.address,
        backend = cfg.backend.address,
        backend_port = cfg.backend.address.rsplit(':').next().unwrap_or("25566"),
        secret = cfg.backend.forwarding_secret.replace('\\', "\\\\").replace('"', "\\\""),
    )
}

impl Versions {
    /// Whether a client version passes the allow/deny lists (translator support is checked separately).
    pub fn permits(&self, v: crate::proto::version::Version) -> bool {
        if self.deny.iter().any(|e| selector_matches(e, v)) {
            return false;
        }
        self.allow.is_empty() || self.allow.iter().any(|e| selector_matches(e, v))
    }
}

/// Resolves a version name, including names of releases that share a protocol with a listed one
/// (1.21.1 -> 1.21, 1.21.3 -> 1.21.2, 1.21.8 -> 1.21.7, 1.21.10 -> 1.21.9).
fn resolve(name: &str) -> Option<crate::proto::version::Version> {
    use crate::proto::version::Version;
    let name = name.trim().trim_start_matches("minecraft ").trim_start_matches('v');
    if let Some(v) = Version::from_name(name) {
        return Some(v);
    }
    if let Ok(p) = name.parse::<i32>() {
        return Some(Version { protocol: p });
    }
    // Patch release sharing the protocol of an earlier one: walk the patch number down.
    let mut parts: Vec<u32> = name.split('.').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if parts.len() != 3 {
        return None;
    }
    while parts[2] > 0 {
        parts[2] -= 1;
        let candidate = if parts[2] == 0 { format!("{}.{}", parts[0], parts[1]) } else { format!("{}.{}.{}", parts[0], parts[1], parts[2]) };
        if let Some(v) = Version::from_name(&candidate) {
            return Some(v);
        }
    }
    None
}

/// `"26.2"`, `"1.21.*"` (family) or `"1.21.5-26.3"` (inclusive range, either end may be omitted).
pub fn selector_matches(selector: &str, v: crate::proto::version::Version) -> bool {
    let sel = selector.trim();
    if sel == "*" || sel.eq_ignore_ascii_case("all") {
        return true;
    }
    if let Some(family) = sel.strip_suffix(".*") {
        let name = v.name();
        return name == family || name.starts_with(&format!("{family}."));
    }
    if let Some((lo, hi)) = sel.split_once('-') {
        let lo_ok = lo.trim().is_empty() || resolve(lo).is_some_and(|l| v.protocol >= l.protocol);
        let hi_ok = hi.trim().is_empty() || resolve(hi).is_some_and(|h| v.protocol <= h.protocol);
        return lo_ok && hi_ok && (resolve(lo).is_some() || resolve(hi).is_some());
    }
    resolve(sel) == Some(v)
}

#[cfg(test)]
mod version_selector_tests {
    use super::*;
    use crate::proto::version::Version;
    fn v(name: &str) -> Version {
        Version::from_name(name).unwrap()
    }
    #[test]
    fn selectors() {
        assert!(selector_matches("26.2", v("26.2")));
        assert!(!selector_matches("26.2", v("26.3")));
        assert!(selector_matches("1.21.*", v("1.21.5")));
        assert!(selector_matches("1.21.*", v("1.21")));
        assert!(!selector_matches("1.21.*", v("26.1")));
        assert!(selector_matches("1.21.5-26.1", v("1.21.9")));
        assert!(!selector_matches("1.21.5-26.1", v("26.2")));
        assert!(selector_matches("-1.21.4", v("1.21")));
        assert!(selector_matches("26.2-", v("26.3")));
        assert!(selector_matches("1.21.1", v("1.21")));
        assert!(selector_matches("1.21.10", v("1.21.9")));
        assert!(!selector_matches("bogus", v("26.3")));
    }
    #[test]
    fn allow_and_deny() {
        let cfg = Versions { allow: vec!["1.21.*".into(), "26.3".into()], deny: vec!["1.21".into()], ..Default::default() };
        assert!(cfg.permits(v("1.21.4")));
        assert!(cfg.permits(v("26.3")));
        assert!(!cfg.permits(v("1.21")));
        assert!(!cfg.permits(v("26.2")));
        assert!(Versions::default().permits(v("1.21.2")));
        assert!(Versions::default().permits(v("26.3")));
        assert!(Versions { allow: Vec::new(), ..Default::default() }.permits(v("1.21")));
    }
}
