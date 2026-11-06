//! Mojang session check (`hasJoined`) over wasi:http. Non-blocking: start the request, hand its
//! pollable to the reactor, finish when it fires.

use crate::proto::buf::uuid_from_str;
use wasip2::http::outgoing_handler;
use wasip2::http::types::{Fields, FutureIncomingResponse, Method, OutgoingRequest, Scheme};
use wasip2::io::poll::Pollable;

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub id: u128,
    pub name: String,
    pub properties: Vec<Property>,
}

pub struct PendingAuth {
    future: FutureIncomingResponse,
}

impl PendingAuth {
    pub fn pollable(&self) -> Pollable {
        self.future.subscribe()
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Starts `GET {session_server}/session/minecraft/hasJoined?username=..&serverId=..[&ip=..]`.
pub fn start(session_server: &str, username: &str, server_hash: &str, ip: Option<&str>) -> Result<PendingAuth, String> {
    let (scheme, rest) = session_server
        .split_once("://")
        .ok_or_else(|| format!("session_server has no scheme: {session_server}"))?;
    let (authority, base_path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let mut path = format!(
        "/{}session/minecraft/hasJoined?username={}&serverId={}",
        if base_path.is_empty() { String::new() } else { format!("{}/", base_path.trim_end_matches('/')) },
        url_encode(username),
        url_encode(server_hash)
    );
    if let Some(ip) = ip {
        path.push_str("&ip=");
        path.push_str(&url_encode(ip));
    }
    let headers = Fields::from_list(&[("user-agent".to_string(), b"multiversion-pumpkin-plugin".to_vec())])
        .map_err(|e| format!("headers: {e:?}"))?;
    let req = OutgoingRequest::new(headers);
    req.set_method(&Method::Get).map_err(|()| "method")?;
    req.set_scheme(Some(&if scheme == "http" { Scheme::Http } else { Scheme::Https })).map_err(|()| "scheme")?;
    req.set_authority(Some(authority)).map_err(|()| "authority")?;
    req.set_path_with_query(Some(&path)).map_err(|()| "path")?;
    let future = outgoing_handler::handle(req, None).map_err(|e| format!("http: {e:?}"))?;
    Ok(PendingAuth { future })
}

/// Result of a finished session check.
pub enum AuthOutcome {
    /// Mojang confirmed the session.
    Ok(Profile),
    /// 204 / no content: the player has no valid session for this server.
    InvalidSession,
    /// Something else went wrong (network, 5xx, bad JSON).
    Error(String),
}

/// Completes the request once the reactor reports the pollable ready.
pub fn finish(pending: PendingAuth) -> AuthOutcome {
    let response = match pending.future.get() {
        Some(Ok(Ok(r))) => r,
        Some(Ok(Err(e))) => return AuthOutcome::Error(format!("http error: {e:?}")),
        Some(Err(())) => return AuthOutcome::Error("response already taken".into()),
        None => return AuthOutcome::Error("response not ready".into()),
    };
    let status = response.status();
    if status == 204 || status == 404 {
        return AuthOutcome::InvalidSession;
    }
    if status != 200 {
        return AuthOutcome::Error(format!("session server returned {status}"));
    }
    let body = match response.consume() {
        Ok(b) => b,
        Err(()) => return AuthOutcome::Error("no body".into()),
    };
    let stream = match body.stream() {
        Ok(s) => s,
        Err(()) => return AuthOutcome::Error("no body stream".into()),
    };
    let mut bytes = Vec::new();
    loop {
        match stream.blocking_read(1 << 16) {
            Ok(chunk) => {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > 1 << 20 {
                    return AuthOutcome::Error("response too large".into());
                }
            }
            Err(wasip2::io::streams::StreamError::Closed) => break,
            Err(e) => return AuthOutcome::Error(format!("read: {e:?}")),
        }
    }
    drop(stream);
    parse_profile(&bytes)
}

pub fn parse_profile(json: &[u8]) -> AuthOutcome {
    let v: serde_json::Value = match serde_json::from_slice(json) {
        Ok(v) => v,
        Err(e) => return AuthOutcome::Error(format!("bad json: {e}")),
    };
    let Some(id) = v.get("id").and_then(|i| i.as_str()).and_then(uuid_from_str) else {
        return AuthOutcome::Error("profile without id".into());
    };
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    let properties = v
        .get("properties")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(Property {
                        name: p.get("name")?.as_str()?.to_string(),
                        value: p.get("value")?.as_str()?.to_string(),
                        signature: p.get("signature").and_then(|s| s.as_str()).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    AuthOutcome::Ok(Profile { id, name, properties })
}
