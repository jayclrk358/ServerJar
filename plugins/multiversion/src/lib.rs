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
                authors: vec!["ServerJar".into()],
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
            let path = format!("{}/config.toml", context.get_data_folder());
            let config = crate::config::Config::load_or_create(&path).map_err(|e| format!("multiversion: {e}"))?;
            crate::log::info(&format!(
                "listening on {} for Minecraft {} clients, forwarding to Pumpkin at {}",
                config.listen.address,
                crate::translate::supported_range(),
                config.backend.address
            ));
            if config.backend.forwarding_secret.is_empty() {
                crate::log::warn("backend.forwarding_secret is empty; Pumpkin will reject logins. Set it in config.toml and pumpkin.toml.");
            }
            *self.config.lock().unwrap() = Some(config.clone());
            // The proxy loop runs forever inside a scheduled task, which Pumpkin spawns off the tick thread.
            context.schedule_delayed_task(1, move |_server| {
                if let Err(e) = crate::proxy::run(config.clone()) {
                    crate::log::error(&format!("proxy stopped: {e}"));
                }
            });
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
