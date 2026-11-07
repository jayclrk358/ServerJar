//! multiversion: a Pumpkin plugin that lets other Minecraft Java versions join.
//!
//! It runs a protocol-translating proxy *inside* the server process: it listens on the public
//! port, speaks each client's own protocol version, verifies the player with Mojang, connects
//! to Pumpkin's localhost listener as a 26.3 client and hands Pumpkin the real identity through
//! Velocity-style forwarding (signed with a shared secret). Game packets flow through a chain of
//! translators between the client's version and Pumpkin's.

#![allow(clippy::too_many_lines)]

pub mod config;
pub mod crypto;
pub mod net;
pub mod platform;
pub mod proto;
pub mod translate;

#[cfg(target_arch = "wasm32")]
pub mod auth;
#[cfg(target_arch = "wasm32")]
pub mod conn;
#[cfg(target_arch = "wasm32")]
pub mod proxy;

#[cfg(target_arch = "wasm32")]
mod plugin {
    use pumpkin_plugin_api::scheduler::SchedulerExt;
    use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, permissions, register_plugin};
    use std::sync::Mutex;

    pub struct MultiVersion {
        config: Mutex<Option<crate::config::Config>>,
    }

    impl Plugin for MultiVersion {
        fn new() -> Self {
            Self { config: Mutex::new(None) }
        }

        fn metadata(&self) -> PluginMetadata {
            PluginMetadata {
                name: "multiversion".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                authors: vec!["Mineversed".into()],
                description: "Lets older and newer Minecraft Java clients join by translating their protocol.".into(),
                dependencies: vec![],
                permissions: vec![
                    permissions::NETWORK_TCP.into(),      // bind the public port, connect to Pumpkin
                    permissions::HTTP_OUTBOUND.into(),    // sessionserver.mojang.com (online mode)
                    permissions::FS_READ_DATA.into(),     // config.toml
                    permissions::FS_WRITE_DATA.into(),    // write default config on first start
                ],
            }
        }

        fn on_load(&self, context: Context) -> pumpkin_plugin_api::Result<()> {
            let folder = context.get_data_folder();
            let path = format!("{folder}/config.toml");
            let config = crate::config::Config::load_or_create(&path).map_err(|e| format!("multiversion: {e}"))?;
            // Pumpkin lets a plugin write only its own data folder, so the three pumpkin.toml settings
            // it needs are written out ready to paste, and checked for at runtime.
            let snippet = crate::config::pumpkin_snippet(&config);
            let _ = std::fs::write(format!("{folder}/pumpkin-settings.txt"), &snippet);
            crate::log::info(&format!(
                "listening on {} for Minecraft {} clients, forwarding to Pumpkin at {}",
                config.listen.address,
                crate::translate::allowed_range(&config.versions),
                config.backend.address
            ));
            if config.backend.forwarding_secret.is_empty() {
                crate::log::warn("backend.forwarding_secret is empty; Pumpkin will reject logins. Set it in config.toml and pumpkin.toml.");
            }
            crate::log::info("if players can't join, apply plugins/data/multiversion/pumpkin-settings.txt to pumpkin.toml (three settings) and restart");
            *self.config.lock().unwrap() = Some(config.clone());
            // Bind now; then run the proxy loop in short slices, one per server tick, so Pumpkin
            // can still call our other hooks (and unload us on shutdown) in between.
            crate::proxy::start(config).map_err(|e| format!("multiversion: {e}"))?;
            context.schedule_repeating_task(1, 1, |_server| crate::proxy::tick());
            Ok(())
        }

        fn on_unload(&self, _context: Context) -> pumpkin_plugin_api::Result<()> {
            crate::proxy::request_stop();
            Ok(())
        }
    }

    register_plugin!(MultiVersion);
}

/// Logging that goes to the server console through the plugin API on wasm, stderr natively.
pub mod log {
    /// Logs a warning once per `key` for the life of the plugin.
    pub fn warn_once(key: &str, msg: &str) {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if seen.iter().any(|k| k == key) {
            return;
        }
        seen.push(key.to_string());
        drop(seen);
        warn(msg);
    }
    #[cfg(target_arch = "wasm32")]
    pub fn info(msg: &str) {
        pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Info, &format!("[multiversion] {msg}"));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn warn(msg: &str) {
        pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Warn, &format!("[multiversion] {msg}"));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn error(msg: &str) {
        pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Error, &format!("[multiversion] {msg}"));
    }
    #[cfg(target_arch = "wasm32")]
    pub fn debug(msg: &str) {
        pumpkin_plugin_api::logging::log(pumpkin_plugin_api::logging::LogLevel::Debug, &format!("[multiversion] {msg}"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn info(msg: &str) {
        eprintln!("[info] {msg}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn warn(msg: &str) {
        eprintln!("[warn] {msg}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn error(msg: &str) {
        eprintln!("[error] {msg}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug(msg: &str) {
        eprintln!("[debug] {msg}");
    }
}
