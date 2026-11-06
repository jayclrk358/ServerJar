//! One player connection: the client side (in its own protocol version) and, once logged in,
//! the backend side (Pumpkin, 26.3). Drives handshake → status/login → configuration/play.

use std::net::SocketAddr;

use crate::auth::{Profile, Property};
use crate::config::Config;
use crate::crypto::{self, ServerKeys};
use crate::net::reactor::Token;
use crate::platform::now_millis;
use crate::proto::buf::{offline_uuid, uuid_to_string, Reader, Writer};
use crate::proto::frame::{Inbound, Outbound};
use crate::proto::version::{Version, BACKEND};
use crate::translate::{self, Chain, Packet, State};

// 26.3 (backend) control packet ids.
pub mod ids {
    pub const LOGIN_C_DISCONNECT: i32 = 0x00;
    pub const LOGIN_C_HELLO: i32 = 0x01;
    pub const LOGIN_C_FINISHED: i32 = 0x02;
    pub const LOGIN_C_COMPRESSION: i32 = 0x03;
    pub const LOGIN_C_CUSTOM_QUERY: i32 = 0x04;
    pub const LOGIN_S_HELLO: i32 = 0x00;
    pub const LOGIN_S_KEY: i32 = 0x01;
    pub const LOGIN_S_CUSTOM_QUERY_ANSWER: i32 = 0x02;
    pub const LOGIN_S_ACKNOWLEDGED: i32 = 0x03;
    pub const CONFIG_C_DISCONNECT: i32 = 0x02;
    pub const CONFIG_C_FINISH: i32 = 0x03;
    pub const CONFIG_S_FINISH: i32 = 0x03;
    pub const PLAY_C_DISCONNECT: i32 = 32;
    pub const PLAY_C_START_CONFIGURATION: i32 = 120;
    pub const PLAY_S_CONFIGURATION_ACKNOWLEDGED: i32 = 16;
    pub const STATUS_C_RESPONSE: i32 = 0x00;
    pub const STATUS_C_PONG: i32 = 0x01;
    pub const STATUS_S_REQUEST: i32 = 0x00;
    pub const STATUS_S_PING: i32 = 0x01;
    pub const VELOCITY_CHANNEL: &str = "velocity:player_info";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Handshake,
    /// Waiting for status_request / ping from the client.
    Status,
    /// Fetching the backend's status to answer the client's request.
    StatusBackendHandshake,
    StatusBackendWait,
    LoginStart,
    /// Encryption request sent, waiting for the key response.
    LoginAwaitKey,
    /// hasJoined in flight.
    LoginAuth,
    /// Connecting + logging into the backend.
    BackendConnecting,
    BackendLogin,
    /// Backend finished login; waiting for the client's login_acknowledged.
    AwaitClientLoginAck,
    Configuration,
    Play,
    Closed,
}

pub enum Action {
    /// Queue bytes to the client socket.
    SendClient(Vec<u8>),
    /// Queue bytes to the backend socket.
    SendBackend(Vec<u8>),
    /// Open the backend connection.
    ConnectBackend,
    /// Start a session check; the proxy registers the pollable.
    StartAuth { username: String, server_hash: String, ip: Option<String> },
    /// Close the client (after flushing) with a reason for the log.
    Close(String),
    Log(String),
}

pub struct Connection {
    pub client: Token,
    pub backend: Option<Token>,
    pub peer: SocketAddr,
    pub phase: Phase,
    pub client_version: Version,
    pub client_in: Inbound,
    pub client_out: Outbound,
    pub backend_in: Inbound,
    pub backend_out: Outbound,
    pub created_ms: u64,
    pub username: String,
    pub client_uuid: u128,
    pub profile: Option<Profile>,
    verify_token: Vec<u8>,
    pub chain: Option<Chain>,
    state: State,
    /// Client-side state (lags the backend by one ack).
    client_state: State,
    /// Clientbound packets held while the client finishes its login/config ack.
    held_clientbound: Vec<Vec<u8>>,
    status_ping_payload: Option<i64>,
    pending_status_request: bool,
}

pub struct Shared<'a> {
    pub config: &'a Config,
    pub keys: &'a ServerKeys,
    pub backend_addr: SocketAddr,
}

impl Connection {
    pub fn new(client: Token, peer: SocketAddr) -> Self {
        Self {
            client,
            backend: None,
            peer,
            phase: Phase::Handshake,
            client_version: BACKEND,
            client_in: Inbound::default(),
            client_out: Outbound::default(),
            backend_in: Inbound::default(),
            backend_out: Outbound::default(),
            created_ms: now_millis(),
            username: String::new(),
            client_uuid: 0,
            profile: None,
            verify_token: Vec::new(),
            chain: None,
            state: State::Configuration,
            client_state: State::Configuration,
            held_clientbound: Vec::new(),
            status_ping_payload: None,
            pending_status_request: false,
        }
    }

    fn client_pkt(&mut self, pkt: &Packet, out: &mut Vec<Action>) {
        let mut bytes = Vec::new();
        self.client_out.frame(&pkt.encode(), &mut bytes);
        self.client_out.seal(&mut bytes);
        out.push(Action::SendClient(bytes));
    }

    fn backend_pkt(&mut self, pkt: &Packet, out: &mut Vec<Action>) {
        let mut bytes = Vec::new();
        self.backend_out.frame(&pkt.encode(), &mut bytes);
        out.push(Action::SendBackend(bytes));
    }

    fn kick_json(&mut self, message: &str, out: &mut Vec<Action>) {
        // Login disconnect takes a JSON text component (all supported versions).
        let json = serde_json::json!({ "text": message }).to_string();
        let mut w = Writer::new();
        w.string(&json);
        let pkt = Packet::new(ids::LOGIN_C_DISCONNECT, w.into_inner());
        if matches!(self.phase, Phase::LoginStart | Phase::LoginAwaitKey | Phase::LoginAuth | Phase::BackendConnecting | Phase::BackendLogin) {
            self.client_pkt(&pkt, out);
        }
        self.phase = Phase::Closed;
        out.push(Action::Close(message.to_string()));
    }

    /// Bytes arrived from the client socket.
    pub fn on_client_data(&mut self, data: &[u8], shared: &Shared, out: &mut Vec<Action>) {
        self.client_in.feed(data);
        loop {
            match self.client_in.next_packet() {
                Ok(Some(pkt)) => {
                    if let Err(e) = self.handle_client_packet(pkt, shared, out) {
                        self.kick_json(&e, out);
                        return;
                    }
                    if self.phase == Phase::Closed {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    self.kick_json(&format!("Protocol error: {e}"), out);
                    return;
                }
            }
        }
    }

    fn handle_client_packet(&mut self, raw: Vec<u8>, shared: &Shared, out: &mut Vec<Action>) -> Result<(), String> {
        let mut r = Reader::new(&raw);
        let id = r.varint().map_err(|e| e.to_string())?;
        let payload = r.remaining().to_vec();
        match self.phase {
            Phase::Handshake => self.handle_handshake(id, &payload, shared, out),
            Phase::Status | Phase::StatusBackendHandshake | Phase::StatusBackendWait => self.handle_status(id, &payload, shared, out),
            Phase::LoginStart => self.handle_login_start(id, &payload, shared, out),
            Phase::LoginAwaitKey => self.handle_login_key(id, &payload, shared, out),
            Phase::LoginAuth | Phase::BackendConnecting | Phase::BackendLogin => {
                // Clients send nothing here (cookie responses aside); ignore.
                Ok(())
            }
            Phase::AwaitClientLoginAck => {
                if id == ids::LOGIN_S_ACKNOWLEDGED {
                    // Both sides enter configuration. Tell the backend, release held packets.
                    self.phase = Phase::Configuration;
                    self.state = State::Configuration;
                    self.client_state = State::Configuration;
                    let ack = Packet::new(ids::LOGIN_S_ACKNOWLEDGED, Vec::new());
                    self.backend_pkt(&ack, out);
                    let held = std::mem::take(&mut self.held_clientbound);
                    for bytes in held {
                        out.push(Action::SendClient(bytes));
                    }
                }
                Ok(())
            }
            Phase::Configuration | Phase::Play => self.relay_serverbound(id, payload, out),
            Phase::Closed => Ok(()),
        }
    }

    fn handle_handshake(&mut self, id: i32, payload: &[u8], shared: &Shared, _out: &mut Vec<Action>) -> Result<(), String> {
        if id != 0 {
            return Err("expected handshake".into());
        }
        let mut r = Reader::new(payload);
        let protocol = r.varint().map_err(|e| e.to_string())?;
        let _address = r.string_max(255).map_err(|e| e.to_string())?;
        let _port = r.u16().map_err(|e| e.to_string())?;
        let next = r.varint().map_err(|e| e.to_string())?;
        self.client_version = Version { protocol };
        match next {
            1 => {
                self.phase = Phase::Status;
                Ok(())
            }
            2 | 3 => {
                if !translate::is_supported(self.client_version) || !shared.config.versions.allow.is_empty() && !shared.config.versions.allow.iter().any(|n| Version::from_name(n) == Some(self.client_version)) {
                    self.phase = Phase::LoginStart; // so the kick is delivered as a login disconnect
                    let msg = shared
                        .config
                        .versions
                        .kick_unsupported_message
                        .replace("{versions}", &translate::supported_range())
                        .replace("{client}", &self.client_version.name());
                    return Err(msg);
                }
                self.chain = Chain::for_client(self.client_version);
                self.phase = Phase::LoginStart;
                Ok(())
            }
            _ => Err("bad next state".into()),
        }
    }

    fn handle_status(&mut self, id: i32, payload: &[u8], _shared: &Shared, out: &mut Vec<Action>) -> Result<(), String> {
        match id {
            ids::STATUS_S_REQUEST => {
                // Ask the backend for its status so MOTD/favicon/player count stay Pumpkin's.
                self.pending_status_request = true;
                if self.phase == Phase::Status {
                    self.phase = Phase::StatusBackendHandshake;
                    out.push(Action::ConnectBackend);
                }
                Ok(())
            }
            ids::STATUS_S_PING => {
                let mut r = Reader::new(payload);
                let v = r.i64().map_err(|e| e.to_string())?;
                if self.pending_status_request {
                    self.status_ping_payload = Some(v);
                } else {
                    let mut w = Writer::new();
                    w.i64(v);
                    let pkt = Packet::new(ids::STATUS_C_PONG, w.into_inner());
                    self.client_pkt(&pkt, out);
                    self.phase = Phase::Closed;
                    out.push(Action::Close("status done".into()));
                }
                Ok(())
            }
            _ => Err("unexpected status packet".into()),
        }
    }

    /// Backend connected while in status mode: send handshake + request.
    fn status_backend_connected(&mut self, shared: &Shared, out: &mut Vec<Action>) {
        let mut w = Writer::new();
        w.varint(BACKEND.protocol);
        w.string(&shared.backend_addr.ip().to_string());
        w.u16(shared.backend_addr.port());
        w.varint(1);
        let hs = Packet::new(0, w.into_inner());
        self.backend_pkt(&hs, out);
        let req = Packet::new(ids::STATUS_S_REQUEST, Vec::new());
        self.backend_pkt(&req, out);
        self.phase = Phase::StatusBackendWait;
    }

    fn status_fallback_json(&self, shared: &Shared) -> String {
        serde_json::json!({
            "version": { "name": self.version_name(shared), "protocol": self.client_version.protocol },
            "players": { "max": 0, "online": 0 },
            "description": { "text": "Server starting..." }
        })
        .to_string()
    }

    fn version_name(&self, shared: &Shared) -> String {
        if shared.config.status.version_name.is_empty() { translate::supported_range() } else { shared.config.status.version_name.clone() }
    }

    fn finish_status(&mut self, backend_json: Option<&str>, shared: &Shared, out: &mut Vec<Action>) {
        let mut json: serde_json::Value = backend_json
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_else(|| serde_json::from_str(&self.status_fallback_json(shared)).unwrap());
        // Report a protocol the client accepts (its own, if we can translate it), and our version range.
        let supported = translate::is_supported(self.client_version);
        let protocol = if supported { self.client_version.protocol } else { BACKEND.protocol };
        json["version"] = serde_json::json!({ "name": self.version_name(shared), "protocol": protocol });
        let mut w = Writer::new();
        w.string(&json.to_string());
        let pkt = Packet::new(ids::STATUS_C_RESPONSE, w.into_inner());
        self.client_pkt(&pkt, out);
        self.pending_status_request = false;
        if let Some(v) = self.status_ping_payload.take() {
            let mut w = Writer::new();
            w.i64(v);
            let pkt = Packet::new(ids::STATUS_C_PONG, w.into_inner());
            self.client_pkt(&pkt, out);
            self.phase = Phase::Closed;
            out.push(Action::Close("status done".into()));
        } else {
            self.phase = Phase::Status;
        }
    }

    fn handle_login_start(&mut self, id: i32, payload: &[u8], shared: &Shared, out: &mut Vec<Action>) -> Result<(), String> {
        if id != ids::LOGIN_S_HELLO {
            return Err("expected login start".into());
        }
        let mut r = Reader::new(payload);
        let name = r.string_max(16).map_err(|e| e.to_string())?.to_string();
        if name.is_empty() || name.len() > 16 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err("Invalid characters in username".into());
        }
        // 1.20.2+: uuid; 1.20/1.20.1: optional uuid.
        let uuid = if self.client_version.protocol >= 764 {
            r.uuid().map_err(|e| e.to_string())?
        } else {
            r.option(|r| r.uuid()).map_err(|e| e.to_string())?.unwrap_or_else(|| offline_uuid(&name))
        };
        self.username = name.clone();
        self.client_uuid = uuid;
        if shared.config.auth.online_mode {
            self.verify_token = crypto::random_bytes(4);
            let mut w = Writer::new();
            w.string(""); // server id
            w.byte_array(&shared.keys.public_der);
            w.byte_array(&self.verify_token);
            if self.client_version.protocol >= 766 {
                w.bool(true); // should authenticate
            }
            let pkt = Packet::new(ids::LOGIN_C_HELLO, w.into_inner());
            self.client_pkt(&pkt, out);
            self.phase = Phase::LoginAwaitKey;
            Ok(())
        } else {
            self.profile = Some(Profile { id: offline_uuid(&name), name, properties: Vec::new() });
            self.after_auth(shared, out);
            Ok(())
        }
    }

    fn handle_login_key(&mut self, id: i32, payload: &[u8], shared: &Shared, out: &mut Vec<Action>) -> Result<(), String> {
        if id != ids::LOGIN_S_KEY {
            return Err("expected encryption response".into());
        }
        let mut r = Reader::new(payload);
        let enc_secret = r.byte_array().map_err(|e| e.to_string())?;
        let enc_token = r.byte_array().map_err(|e| e.to_string())?;
        let secret = shared.keys.decrypt(enc_secret)?;
        let token = shared.keys.decrypt(enc_token)?;
        if token != self.verify_token {
            return Err("Invalid verify token".into());
        }
        if secret.len() != 16 {
            return Err("Invalid shared secret".into());
        }
        let key: [u8; 16] = secret[..].try_into().unwrap();
        self.client_in.enable_encryption(key);
        self.client_out.enable_encryption(key);
        let hash = crypto::server_hash("", &secret, &shared.keys.public_der);
        let ip = if shared.config.auth.prevent_proxy_connections { Some(self.peer.ip().to_string()) } else { None };
        self.phase = Phase::LoginAuth;
        out.push(Action::StartAuth { username: self.username.clone(), server_hash: hash, ip });
        Ok(())
    }

    /// Called by the proxy when the session check finished.
    pub fn on_auth(&mut self, result: crate::auth::AuthOutcome, shared: &Shared, out: &mut Vec<Action>) {
        match result {
            crate::auth::AuthOutcome::Ok(profile) => {
                if !profile.name.eq_ignore_ascii_case(&self.username) {
                    self.kick_json("Username mismatch", out);
                    return;
                }
                self.username = profile.name.clone();
                self.profile = Some(profile);
                self.after_auth(shared, out);
            }
            crate::auth::AuthOutcome::InvalidSession => {
                self.kick_json("Invalid session (Try restarting your game and the launcher)", out);
            }
            crate::auth::AuthOutcome::Error(e) => {
                out.push(Action::Log(format!("session check failed for {}: {e}", self.username)));
                self.kick_json("Authentication servers are down. Please try again later, sorry!", out);
            }
        }
    }

    fn after_auth(&mut self, shared: &Shared, out: &mut Vec<Action>) {
        // Compression towards the client, then connect to Pumpkin.
        let threshold = shared.config.limits.compression_threshold;
        if threshold >= 0 {
            let mut w = Writer::new();
            w.varint(threshold);
            let pkt = Packet::new(ids::LOGIN_C_COMPRESSION, w.into_inner());
            self.client_pkt(&pkt, out);
            self.client_in.compression = threshold;
            self.client_out.compression = threshold;
        }
        self.phase = Phase::BackendConnecting;
        out.push(Action::ConnectBackend);
    }

    /// The backend socket is connected.
    pub fn on_backend_connected(&mut self, shared: &Shared, out: &mut Vec<Action>) {
        match self.phase {
            Phase::StatusBackendHandshake => self.status_backend_connected(shared, out),
            Phase::BackendConnecting => {
                let mut w = Writer::new();
                w.varint(BACKEND.protocol);
                w.string(&shared.backend_addr.ip().to_string());
                w.u16(shared.backend_addr.port());
                w.varint(2);
                let hs = Packet::new(0, w.into_inner());
                self.backend_pkt(&hs, out);
                let profile = self.profile.as_ref().expect("profile set before backend connect");
                let mut w = Writer::new();
                w.string(&profile.name);
                w.uuid(profile.id);
                let hello = Packet::new(ids::LOGIN_S_HELLO, w.into_inner());
                self.backend_pkt(&hello, out);
                self.phase = Phase::BackendLogin;
            }
            _ => {}
        }
    }

    pub fn on_backend_connect_failed(&mut self, error: &str, shared: &Shared, out: &mut Vec<Action>) {
        match self.phase {
            Phase::StatusBackendHandshake => self.finish_status(None, shared, out),
            _ => {
                out.push(Action::Log(format!("backend connect failed: {error}")));
                self.kick_json("The server is starting up. Please try again in a moment.", out);
            }
        }
    }

    /// Bytes arrived from the backend socket.
    pub fn on_backend_data(&mut self, data: &[u8], shared: &Shared, out: &mut Vec<Action>) {
        self.backend_in.feed(data);
        loop {
            match self.backend_in.next_packet() {
                Ok(Some(pkt)) => {
                    if let Err(e) = self.handle_backend_packet(pkt, shared, out) {
                        out.push(Action::Log(format!("backend packet error for {}: {e}", self.username)));
                        let msg = if e.contains("pumpkin.toml") { "Server misconfigured: see the server console." } else { "Internal proxy error" };
                        self.kick_json(msg, out);
                        return;
                    }
                    if self.phase == Phase::Closed {
                        return;
                    }
                }
                Ok(None) => return,
                Err(e) => {
                    out.push(Action::Log(format!("backend framing error: {e}")));
                    self.kick_json("Internal proxy error", out);
                    return;
                }
            }
        }
    }

    pub fn on_backend_closed(&mut self, reason: &str, shared: &Shared, out: &mut Vec<Action>) {
        match self.phase {
            Phase::StatusBackendHandshake | Phase::StatusBackendWait => self.finish_status(None, shared, out),
            Phase::Closed => {}
            _ => {
                self.phase = Phase::Closed;
                out.push(Action::Close(format!("backend closed: {reason}")));
            }
        }
    }

    fn handle_backend_packet(&mut self, raw: Vec<u8>, shared: &Shared, out: &mut Vec<Action>) -> Result<(), String> {
        let mut r = Reader::new(&raw);
        let id = r.varint().map_err(|e| e.to_string())?;
        let payload = r.remaining().to_vec();
        match self.phase {
            Phase::StatusBackendWait => {
                if id == ids::STATUS_C_RESPONSE {
                    let mut r = Reader::new(&payload);
                    let json = r.string().map_err(|e| e.to_string())?.to_string();
                    self.finish_status(Some(&json), shared, out);
                    if let Some(b) = self.backend {
                        out.push(Action::Log(format!("__close_backend:{b}")));
                    }
                }
                Ok(())
            }
            Phase::BackendLogin => match id {
                ids::LOGIN_C_CUSTOM_QUERY => {
                    let mut r = Reader::new(&payload);
                    let message_id = r.varint().map_err(|e| e.to_string())?;
                    let channel = r.string().map_err(|e| e.to_string())?;
                    if channel == ids::VELOCITY_CHANNEL {
                        let answer = self.velocity_answer(message_id, shared);
                        self.backend_pkt(&answer, out);
                    } else {
                        // Unknown plugin request: "not understood".
                        let mut w = Writer::new();
                        w.varint(message_id);
                        w.bool(false);
                        let pkt = Packet::new(ids::LOGIN_S_CUSTOM_QUERY_ANSWER, w.into_inner());
                        self.backend_pkt(&pkt, out);
                    }
                    Ok(())
                }
                ids::LOGIN_C_COMPRESSION => {
                    let mut r = Reader::new(&payload);
                    let t = r.varint().map_err(|e| e.to_string())?;
                    self.backend_in.compression = t;
                    self.backend_out.compression = t;
                    Ok(())
                }
                ids::LOGIN_C_HELLO => Err("Pumpkin asked for Mojang authentication instead of accepting the forwarded player: enable [networking.proxy] and [networking.proxy.velocity] in pumpkin.toml (see plugins/data/multiversion/pumpkin-settings.txt)".into()),
                ids::LOGIN_C_DISCONNECT => {
                    let mut r = Reader::new(&payload);
                    let reason = r.string().unwrap_or("{}").to_string();
                    out.push(Action::Log(format!("backend refused login for {}: {reason}", self.username)));
                    // Forward the reason as-is (JSON text component is valid for all versions).
                    let pkt = Packet::new(ids::LOGIN_C_DISCONNECT, payload);
                    self.client_pkt(&pkt, out);
                    self.phase = Phase::Closed;
                    out.push(Action::Close("backend refused login".into()));
                    Ok(())
                }
                ids::LOGIN_C_FINISHED => {
                    // Backend profile (uuid, name, properties, session id). Re-encode for the client.
                    let mut r = Reader::new(&payload);
                    let uuid = r.uuid().map_err(|e| e.to_string())?;
                    let name = r.string_max(16).map_err(|e| e.to_string())?.to_string();
                    let props = r
                        .list(|r| {
                            let n = r.string()?.to_string();
                            let v = r.string()?.to_string();
                            let s = r.option(|r| r.string().map(str::to_string))?;
                            Ok(Property { name: n, value: v, signature: s })
                        })
                        .map_err(|e| e.to_string())?;
                    let session_id = r.uuid().unwrap_or(0);
                    let pkt = self.encode_login_finished(uuid, &name, &props, session_id);
                    self.client_pkt(&pkt, out);
                    self.phase = Phase::AwaitClientLoginAck;
                    Ok(())
                }
                _ => Ok(()),
            },
            Phase::AwaitClientLoginAck => {
                // Backend may not send anything until we ack; hold just in case.
                let mut bytes = Vec::new();
                let pkt = Packet::new(id, payload);
                let translated = self.translate_clientbound(pkt)?;
                for p in translated {
                    self.client_out.frame(&p.encode(), &mut bytes);
                }
                self.client_out.seal(&mut bytes);
                self.held_clientbound.push(bytes);
                Ok(())
            }
            Phase::Configuration | Phase::Play => self.relay_clientbound(id, payload, out),
            _ => Ok(()),
        }
    }

    fn encode_login_finished(&self, uuid: u128, name: &str, props: &[Property], session_id: u128) -> Packet {
        let v = self.client_version.protocol;
        let mut w = Writer::new();
        w.uuid(uuid);
        w.string(name);
        w.varint(props.len() as i32);
        for p in props {
            w.string(&p.name);
            w.string(&p.value);
            w.option(p.signature.as_deref(), |w, s| {
                w.string(s);
            });
        }
        if v >= 776 {
            w.uuid(session_id);
        }
        if (766..768).contains(&v) {
            w.bool(true); // strict error handling
        }
        Packet::new(ids::LOGIN_C_FINISHED, w.into_inner())
    }

    /// Velocity modern forwarding v1 payload, signed with the shared secret (what Pumpkin checks).
    fn velocity_answer(&self, message_id: i32, shared: &Shared) -> Packet {
        let profile = self.profile.as_ref().expect("profile");
        let mut p = Writer::new();
        p.varint(1);
        p.string(&self.peer.ip().to_string());
        p.uuid(profile.id);
        p.string(&profile.name);
        p.varint(profile.properties.len() as i32);
        for prop in &profile.properties {
            p.string(&prop.name);
            p.string(&prop.value);
            p.option(prop.signature.as_deref(), |w, s| {
                w.string(s);
            });
        }
        let sig = crypto::hmac_sha256(shared.config.backend.forwarding_secret.as_bytes(), &p.buf);
        let mut w = Writer::new();
        w.varint(message_id);
        w.bool(true);
        w.raw(&sig);
        w.raw(&p.buf);
        Packet::new(ids::LOGIN_S_CUSTOM_QUERY_ANSWER, w.into_inner())
    }

    fn translate_clientbound(&mut self, pkt: Packet) -> Result<Vec<Packet>, String> {
        match self.chain.as_mut() {
            Some(chain) if !chain.is_passthrough() => chain.clientbound(self.state, pkt),
            _ => Ok(vec![pkt]),
        }
    }

    fn translate_serverbound(&mut self, pkt: Packet) -> Result<Vec<Packet>, String> {
        match self.chain.as_mut() {
            Some(chain) if !chain.is_passthrough() => chain.serverbound(self.client_state, pkt),
            _ => Ok(vec![pkt]),
        }
    }

    fn relay_clientbound(&mut self, id: i32, payload: Vec<u8>, out: &mut Vec<Action>) -> Result<(), String> {
        // Track state changes on the backend (26.3) side before translating.
        let state_before = self.state;
        match (self.state, id) {
            (State::Configuration, ids::CONFIG_C_FINISH) => self.state = State::Play,
            (State::Play, ids::PLAY_C_START_CONFIGURATION) => self.state = State::Configuration,
            (State::Configuration, ids::CONFIG_C_DISCONNECT) | (State::Play, ids::PLAY_C_DISCONNECT) => {
                out.push(Action::Log(format!("{} disconnected by server", self.username)));
            }
            _ => {}
        }
        let pkt = Packet::new(id, payload);
        let translated = match self.chain.as_mut() {
            Some(chain) if !chain.is_passthrough() => chain.clientbound(state_before, pkt)?,
            _ => vec![pkt],
        };
        let mut bytes = Vec::new();
        for p in translated {
            self.client_out.frame(&p.encode(), &mut bytes);
        }
        self.client_out.seal(&mut bytes);
        out.push(Action::SendClient(bytes));
        Ok(())
    }

    fn relay_serverbound(&mut self, id: i32, payload: Vec<u8>, out: &mut Vec<Action>) -> Result<(), String> {
        let pkt = Packet::new(id, payload);
        let translated = self.translate_serverbound(pkt)?;
        let mut bytes = Vec::new();
        for p in translated {
            // State changes are tracked on the backend (26.3) ids after translation.
            match (self.client_state, p.id) {
                (State::Configuration, ids::CONFIG_S_FINISH) => {
                    self.client_state = State::Play;
                    self.phase = Phase::Play;
                }
                (State::Play, ids::PLAY_S_CONFIGURATION_ACKNOWLEDGED) => {
                    self.client_state = State::Configuration;
                    self.phase = Phase::Configuration;
                }
                _ => {}
            }
            self.backend_out.frame(&p.encode(), &mut bytes);
        }
        out.push(Action::SendBackend(bytes));
        Ok(())
    }

    pub fn describe(&self) -> String {
        if self.username.is_empty() {
            format!("{} ({})", self.peer, self.client_version.name())
        } else {
            format!("{} [{}] ({}) uuid {}", self.username, self.peer, self.client_version.name(), uuid_to_string(self.client_uuid))
        }
    }
}
