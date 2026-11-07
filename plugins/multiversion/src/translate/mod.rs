//! Protocol translation between the client's version and Pumpkin's (26.3).
//!
//! A `Translator` converts packets one version step (e.g. 26.3 ↔ 26.2). A `Chain` stacks the
//! steps needed for a given client, newest first for clientbound and oldest first for
//! serverbound, exactly like ViaVersion's protocol pipeline.

pub mod data_26_2;
pub mod data_26_1;
pub mod data_1_21_11;
pub mod data_1_21_9;
pub mod data_1_21_7;
pub mod data_1_21_6;
pub mod data_1_21_5;
pub mod data_1_21_4;
pub mod data_1_21_2;
pub mod data_1_21;
pub mod v26_2;
pub mod v26_1;

use crate::proto::version::{Version, BACKEND, KNOWN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Configuration,
    Play,
}

/// A packet in some protocol version: id + payload.
#[derive(Debug, Clone)]
pub struct Packet {
    pub id: i32,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(id: i32, payload: Vec<u8>) -> Self {
        Self { id, payload }
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut w = crate::proto::buf::Writer::with_capacity(self.payload.len() + 5);
        w.varint(self.id);
        w.raw(&self.payload);
        w.into_inner()
    }
}

pub type TResult = Result<(), String>;

/// One version step. `from` is the newer version (server side), `to` the older (client side).
pub trait Translator {
    fn from(&self) -> Version;
    fn to(&self) -> Version;
    /// Server → client: input is a `from`-version packet, output `to`-version packets (0..n).
    fn clientbound(&mut self, state: State, pkt: &Packet, out: &mut Vec<Packet>) -> TResult;
    /// Client → server: input is a `to`-version packet, output `from`-version packets (0..n).
    fn serverbound(&mut self, state: State, pkt: &Packet, out: &mut Vec<Packet>) -> TResult;
}

/// All steps for one client, newest first.
pub struct Chain {
    steps: Vec<Box<dyn Translator>>,
    pub client: Version,
}

impl Chain {
    /// Builds the chain for `client`, or `None` if the version is not supported.
    pub fn for_client(client: Version) -> Option<Chain> {
        if client == BACKEND {
            return Some(Chain { steps: Vec::new(), client });
        }
        let mut steps: Vec<Box<dyn Translator>> = Vec::new();
        let mut current = BACKEND;
        while current != client {
            let step = make_step(current)?;
            current = step.to();
            steps.push(step);
        }
        Some(Chain { steps, client })
    }

    pub fn is_passthrough(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn clientbound(&mut self, state: State, pkt: Packet) -> Result<Vec<Packet>, String> {
        let mut current = vec![pkt];
        for step in &mut self.steps {
            let mut next = Vec::with_capacity(current.len());
            for p in &current {
                step.clientbound(state, p, &mut next)?;
            }
            current = next;
        }
        Ok(current)
    }

    pub fn serverbound(&mut self, state: State, pkt: Packet) -> Result<Vec<Packet>, String> {
        let mut current = vec![pkt];
        for step in self.steps.iter_mut().rev() {
            let mut next = Vec::with_capacity(current.len());
            for p in &current {
                step.serverbound(state, p, &mut next)?;
            }
            current = next;
        }
        Ok(current)
    }
}

/// The translator that takes `from` one step down, if implemented.
fn make_step(from: Version) -> Option<Box<dyn Translator>> {
    match from.protocol {
        777 => Some(Box::new(v26_2::V26_3To26_2::new())),
        776 => Some(Box::new(v26_1::V26_2To26_1::new())),
        _ => None,
    }
}

/// Lowest client version a chain exists for.
pub fn lowest_supported() -> Version {
    let mut v = BACKEND;
    while let Some(step) = make_step(v) {
        v = step.to();
    }
    v
}

pub fn is_supported(client: Version) -> bool {
    KNOWN.iter().any(|k| k.protocol == client.protocol)
        && client.protocol <= BACKEND.protocol
        && client.protocol >= lowest_supported().protocol
}

pub fn supported_range() -> String {
    let low = lowest_supported();
    if low == BACKEND { BACKEND.name() } else { format!("{}-{}", low.name(), BACKEND.name()) }
}

/// Versions that both have a translator and pass the config's allow/deny lists, newest first.
pub fn allowed_versions(cfg: &crate::config::Versions) -> Vec<Version> {
    KNOWN.iter().map(|k| Version { protocol: k.protocol }).filter(|v| is_supported(*v) && cfg.permits(*v)).collect()
}

/// Human-readable list of the allowed versions, contiguous runs collapsed ("1.21-1.21.4, 26.2-26.3").
pub fn allowed_range(cfg: &crate::config::Versions) -> String {
    let mut vs = allowed_versions(cfg);
    vs.sort();
    let supported: Vec<Version> = {
        let mut s: Vec<Version> = KNOWN.iter().map(|k| Version { protocol: k.protocol }).filter(|v| is_supported(*v)).collect();
        s.sort();
        s
    };
    let mut runs: Vec<(Version, Version)> = Vec::new();
    for v in vs {
        match runs.last_mut() {
            Some((_, hi)) if supported.iter().position(|s| *s == v) == supported.iter().position(|s| s == hi).map(|i| i + 1) => *hi = v,
            _ => runs.push((v, v)),
        }
    }
    if runs.is_empty() {
        return "no version".into();
    }
    runs.iter().map(|(lo, hi)| if lo == hi { lo.name() } else { format!("{}-{}", lo.name(), hi.name()) }).collect::<Vec<_>>().join(", ")
}
