//! The proxy loop: owns the reactor, maps sockets to connections, runs session checks.

use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::auth::{self, PendingAuth};
use crate::config::Config;
use crate::conn::{Action, Connection, Phase, Shared};
use crate::crypto::ServerKeys;
use crate::log;
use crate::net::reactor::{Event, Handler, Reactor, Token};
use crate::platform::now_millis;

static STOP: AtomicBool = AtomicBool::new(false);

pub fn request_stop() {
    STOP.store(true, Ordering::Relaxed);
}

fn resolve(addr: &str) -> Result<SocketAddr, String> {
    addr.to_socket_addrs()
        .map_err(|e| format!("cannot resolve {addr}: {e}"))?
        .next()
        .ok_or_else(|| format!("no address for {addr}"))
}

struct ProxyHandler {
    config: Config,
    keys: ServerKeys,
    backend_addr: SocketAddr,
    conns: HashMap<Token, Connection>,
    /// backend token -> client token
    backend_of: HashMap<Token, Token>,
    /// external pollable id -> (client token, pending auth)
    auths: HashMap<u32, (Token, PendingAuth)>,
}

impl ProxyHandler {
    fn apply(&mut self, reactor: &mut Reactor, client: Token, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::SendClient(bytes) => reactor.send(client, &bytes),
                Action::SendBackend(bytes) => {
                    if let Some(b) = self.conns.get(&client).and_then(|c| c.backend) {
                        reactor.send(b, &bytes);
                    }
                }
                Action::ConnectBackend => match reactor.connect(self.backend_addr) {
                    Ok(token) => {
                        if let Some(c) = self.conns.get_mut(&client) {
                            c.backend = Some(token);
                        }
                        self.backend_of.insert(token, client);
                    }
                    Err(e) => {
                        let mut more = Vec::new();
                        if let Some(c) = self.conns.get_mut(&client) {
                            let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                            c.on_backend_connect_failed(&e, &shared, &mut more);
                        }
                        self.apply(reactor, client, more);
                    }
                },
                Action::StartAuth { username, server_hash, ip } => {
                    match auth::start(&self.config.auth.session_server, &username, &server_hash, ip.as_deref()) {
                        Ok(pending) => {
                            let id = reactor.watch(pending.pollable());
                            self.auths.insert(id, (client, pending));
                        }
                        Err(e) => {
                            let mut more = Vec::new();
                            if let Some(c) = self.conns.get_mut(&client) {
                                let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                                c.on_auth(auth::AuthOutcome::Error(e), &shared, &mut more);
                            }
                            self.apply(reactor, client, more);
                        }
                    }
                }
                Action::Close(reason) => {
                    self.close(reactor, client, &reason, false);
                }
                Action::Log(msg) => {
                    if let Some(b) = msg.strip_prefix("__close_backend:") {
                        if let Ok(tok) = b.parse::<Token>() {
                            reactor.close_gracefully(tok);
                            self.backend_of.remove(&tok);
                            if let Some(c) = self.conns.get_mut(&client) {
                                if c.backend == Some(tok) {
                                    c.backend = None;
                                }
                            }
                        }
                    } else {
                        log::info(&msg);
                    }
                }
            }
        }
    }

    fn close(&mut self, reactor: &mut Reactor, client: Token, reason: &str, abrupt: bool) {
        if let Some(c) = self.conns.remove(&client) {
            if c.phase != Phase::Status && c.phase != Phase::StatusBackendHandshake && c.phase != Phase::StatusBackendWait && !reason.starts_with("status") {
                log::info(&format!("{} disconnected: {reason}", c.describe()));
            }
            if abrupt {
                reactor.close_now(client);
            } else {
                reactor.close_gracefully(client);
            }
            if let Some(b) = c.backend {
                self.backend_of.remove(&b);
                reactor.close_gracefully(b);
            }
        }
    }
}

impl Handler for ProxyHandler {
    fn handle(&mut self, reactor: &mut Reactor, event: Event) {
        if STOP.load(Ordering::Relaxed) {
            reactor.stop();
            return;
        }
        match event {
            Event::Accepted { token, peer } => {
                if self.conns.len() >= self.config.limits.max_connections {
                    reactor.close_now(token);
                    return;
                }
                self.conns.insert(token, Connection::new(token, peer));
            }
            Event::Data { token, data } => {
                let mut actions = Vec::new();
                if let Some(client) = self.backend_of.get(&token).copied() {
                    if let Some(c) = self.conns.get_mut(&client) {
                        let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                        c.on_backend_data(&data, &shared, &mut actions);
                    }
                    self.apply(reactor, client, actions);
                } else if let Some(c) = self.conns.get_mut(&token) {
                    let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                    c.on_client_data(&data, &shared, &mut actions);
                    self.apply(reactor, token, actions);
                }
            }
            Event::Connected { token, result } => {
                let Some(client) = self.backend_of.get(&token).copied() else { return };
                let mut actions = Vec::new();
                if let Some(c) = self.conns.get_mut(&client) {
                    let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                    match result {
                        Ok(()) => c.on_backend_connected(&shared, &mut actions),
                        Err(e) => c.on_backend_connect_failed(&e, &shared, &mut actions),
                    }
                }
                self.apply(reactor, client, actions);
            }
            Event::Closed { token, reason } => {
                if token == 0 {
                    log::warn(&format!("listener: {reason}"));
                    return;
                }
                if let Some(client) = self.backend_of.remove(&token) {
                    let mut actions = Vec::new();
                    if let Some(c) = self.conns.get_mut(&client) {
                        c.backend = None;
                        let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                        c.on_backend_closed(&reason, &shared, &mut actions);
                    }
                    self.apply(reactor, client, actions);
                } else if self.conns.contains_key(&token) {
                    self.close(reactor, token, &reason, true);
                }
            }
            Event::External { id } => {
                if let Some((client, pending)) = self.auths.remove(&id) {
                    let outcome = auth::finish(pending);
                    let mut actions = Vec::new();
                    if let Some(c) = self.conns.get_mut(&client) {
                        let shared = Shared { config: &self.config, keys: &self.keys, backend_addr: self.backend_addr };
                        c.on_auth(outcome, &shared, &mut actions);
                    }
                    self.apply(reactor, client, actions);
                }
            }
            Event::Tick => {
                // Drop connections stuck before play for too long.
                let now = now_millis();
                let timeout = self.config.limits.login_timeout_secs * 1000;
                let stale: Vec<Token> = self
                    .conns
                    .iter()
                    .filter(|(_, c)| c.phase != Phase::Play && c.phase != Phase::Configuration && now.saturating_sub(c.created_ms) > timeout)
                    .map(|(t, _)| *t)
                    .collect();
                for t in stale {
                    self.close(reactor, t, "login timeout", true);
                }
            }
        }
    }
}

/// Runs the proxy until `request_stop` is called. Blocks the calling task.
pub fn run(config: Config) -> Result<(), String> {
    let listen = resolve(&config.listen.address)?;
    let backend_addr = resolve(&config.backend.address)?;
    let keys = ServerKeys::generate()?;
    let mut reactor = Reactor::new();
    if let Err(e) = reactor.listen(listen, 128) {
        if e.contains("address-in-use") || e.contains("AddressInUse") {
            return Err(format!(
                "port {} is taken, most likely by Pumpkin itself. In pumpkin.toml set [networking.java] address = \"{}\" \
                 (see plugins/data/multiversion/pumpkin-settings.txt) and restart.",
                listen.port(),
                config.backend.address
            ));
        }
        return Err(e);
    }
    log::info(&format!("accepting connections on {listen}"));
    let mut handler = ProxyHandler {
        config,
        keys,
        backend_addr,
        conns: HashMap::new(),
        backend_of: HashMap::new(),
        auths: HashMap::new(),
    };
    reactor.run(&mut handler);
    Ok(())
}
