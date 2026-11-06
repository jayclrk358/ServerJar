//! A small poll-based reactor over WASI 0.2 TCP sockets.
//!
//! Everything runs on the single plugin task: `Reactor::run` loops forever, blocking in
//! `wasi:io/poll` until any socket is readable/writable or a timer fires, then hands events to
//! the `Handler`. Each connection is identified by a `Token`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use wasip2::io::poll::{self, Pollable};
use wasip2::io::streams::{InputStream, OutputStream, StreamError};
use wasip2::sockets::instance_network::instance_network;
use wasip2::sockets::network::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress, Ipv6SocketAddress, Network,
};
use wasip2::sockets::tcp::TcpSocket;
use wasip2::sockets::tcp_create_socket::create_tcp_socket;

pub type Token = u32;

pub enum Event {
    /// A new client connected on the listener.
    Accepted { token: Token, peer: SocketAddr },
    /// An outbound connect finished (Ok) or failed (Err).
    Connected { token: Token, result: Result<(), String> },
    /// Bytes arrived.
    Data { token: Token, data: Vec<u8> },
    /// Peer closed or the stream errored.
    Closed { token: Token, reason: String },
    /// Periodic tick (about every `tick_ms`).
    Tick,
    /// An external pollable registered with `watch` became ready (it is removed from the set).
    External { id: u32 },
}

pub trait Handler {
    fn handle(&mut self, reactor: &mut Reactor, event: Event);
}

enum ConnState {
    Connecting,
    Open { input: InputStream, output: OutputStream },
    Closed,
}

struct Conn {
    socket: TcpSocket,
    state: ConnState,
    pending_out: Vec<u8>,
    close_after_flush: bool,
}

pub struct Reactor {
    network: Network,
    listener: Option<TcpSocket>,
    conns: HashMap<Token, Conn>,
    externals: HashMap<u32, Pollable>,
    next_token: Token,
    pub tick_ms: u64,
    stop: bool,
}

fn to_wasi_addr(addr: SocketAddr) -> IpSocketAddress {
    match addr {
        SocketAddr::V4(a) => {
            let o = a.ip().octets();
            IpSocketAddress::Ipv4(Ipv4SocketAddress { port: a.port(), address: (o[0], o[1], o[2], o[3]) })
        }
        SocketAddr::V6(a) => {
            let s = a.ip().segments();
            IpSocketAddress::Ipv6(Ipv6SocketAddress {
                port: a.port(),
                flow_info: 0,
                scope_id: 0,
                address: (s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]),
            })
        }
    }
}

fn from_wasi_addr(addr: IpSocketAddress) -> SocketAddr {
    match addr {
        IpSocketAddress::Ipv4(a) => {
            let (o0, o1, o2, o3) = a.address;
            SocketAddr::new(IpAddr::from([o0, o1, o2, o3]), a.port)
        }
        IpSocketAddress::Ipv6(a) => {
            let (s0, s1, s2, s3, s4, s5, s6, s7) = a.address;
            SocketAddr::new(IpAddr::from([s0, s1, s2, s3, s4, s5, s6, s7]), a.port)
        }
    }
}

fn family(addr: SocketAddr) -> IpAddressFamily {
    if addr.is_ipv4() { IpAddressFamily::Ipv4 } else { IpAddressFamily::Ipv6 }
}

fn err(e: ErrorCode) -> String {
    format!("{e:?}")
}

impl Reactor {
    pub fn new() -> Self {
        Self { network: instance_network(), listener: None, conns: HashMap::new(), externals: HashMap::new(), next_token: 1, tick_ms: 1000, stop: false }
    }

    /// Binds and listens. Blocks until the bind completes (it is quick).
    pub fn listen(&mut self, addr: SocketAddr, backlog: u64) -> Result<(), String> {
        let socket = create_tcp_socket(family(addr)).map_err(err)?;
        socket.start_bind(&self.network, to_wasi_addr(addr)).map_err(|e| format!("bind {addr}: {}", err(e)))?;
        let p = socket.subscribe();
        loop {
            match socket.finish_bind() {
                Ok(()) => break,
                Err(ErrorCode::WouldBlock) => {
                    p.block();
                }
                Err(e) => return Err(format!("bind {addr}: {}", err(e))),
            }
        }
        let _ = socket.set_listen_backlog_size(backlog);
        socket.start_listen().map_err(|e| format!("listen {addr}: {}", err(e)))?;
        loop {
            match socket.finish_listen() {
                Ok(()) => break,
                Err(ErrorCode::WouldBlock) => {
                    p.block();
                }
                Err(e) => return Err(format!("listen {addr}: {}", err(e))),
            }
        }
        self.listener = Some(socket);
        Ok(())
    }

    /// Starts an outbound connection; completion arrives as `Event::Connected`.
    pub fn connect(&mut self, addr: SocketAddr) -> Result<Token, String> {
        let socket = create_tcp_socket(family(addr)).map_err(err)?;
        socket.start_connect(&self.network, to_wasi_addr(addr)).map_err(|e| format!("connect {addr}: {}", err(e)))?;
        let token = self.next_token;
        self.next_token += 1;
        self.conns.insert(token, Conn { socket, state: ConnState::Connecting, pending_out: Vec::new(), close_after_flush: false });
        Ok(token)
    }

    /// Queues bytes for sending. Flushed as the socket allows.
    pub fn send(&mut self, token: Token, data: &[u8]) {
        if let Some(c) = self.conns.get_mut(&token) {
            if !matches!(c.state, ConnState::Closed) {
                c.pending_out.extend_from_slice(data);
            }
        }
    }

    /// Closes after everything queued has been written.
    pub fn close_gracefully(&mut self, token: Token) {
        if let Some(c) = self.conns.get_mut(&token) {
            c.close_after_flush = true;
            if c.pending_out.is_empty() {
                self.close_now(token);
            }
        }
    }

    pub fn close_now(&mut self, token: Token) {
        if let Some(mut c) = self.conns.remove(&token) {
            if let ConnState::Open { input, output } = std::mem::replace(&mut c.state, ConnState::Closed) {
                drop(input);
                drop(output);
            }
            let _ = c.socket.shutdown(wasip2::sockets::tcp::ShutdownType::Both);
        }
    }

    pub fn is_open(&self, token: Token) -> bool {
        matches!(self.conns.get(&token).map(|c| &c.state), Some(ConnState::Open { .. }))
    }

    pub fn connection_count(&self) -> usize {
        self.conns.len()
    }

    pub fn stop(&mut self) {
        self.stop = true;
    }

    /// Watches an arbitrary pollable (e.g. an HTTP response future). Fires `Event::External` once.
    pub fn watch(&mut self, pollable: Pollable) -> u32 {
        let id = self.next_token;
        self.next_token += 1;
        self.externals.insert(id, pollable);
        id
    }

    fn try_flush(&mut self, token: Token) -> Result<(), String> {
        let Some(c) = self.conns.get_mut(&token) else { return Ok(()) };
        let ConnState::Open { output, .. } = &c.state else { return Ok(()) };
        while !c.pending_out.is_empty() {
            let n = match output.check_write() {
                Ok(n) => n as usize,
                Err(StreamError::Closed) => return Err("closed".into()),
                Err(StreamError::LastOperationFailed(e)) => return Err(e.to_debug_string()),
            };
            if n == 0 {
                break;
            }
            let take = n.min(c.pending_out.len());
            output.write(&c.pending_out[..take]).map_err(|e| format!("{e:?}"))?;
            c.pending_out.drain(..take);
        }
        if let Err(e) = output.flush() {
            return Err(format!("{e:?}"));
        }
        Ok(())
    }

    /// Main loop. Returns only when `stop()` was called.
    pub fn run(&mut self, handler: &mut dyn Handler) {
        let mut last_tick = wasip2::clocks::monotonic_clock::now();
        while !self.stop {
            // Build the pollable set: listener, every open input, outputs with pending data,
            // connecting sockets, and the tick timer.
            let mut pollables: Vec<Pollable> = Vec::new();
            let mut kinds: Vec<(u8, Token)> = Vec::new(); // 0 listener, 1 input, 2 output, 3 connecting, 4 timer
            if let Some(l) = &self.listener {
                pollables.push(l.subscribe());
                kinds.push((0, 0));
            }
            for (tok, c) in &self.conns {
                match &c.state {
                    ConnState::Connecting => {
                        pollables.push(c.socket.subscribe());
                        kinds.push((3, *tok));
                    }
                    ConnState::Open { input, output } => {
                        pollables.push(input.subscribe());
                        kinds.push((1, *tok));
                        if !c.pending_out.is_empty() {
                            pollables.push(output.subscribe());
                            kinds.push((2, *tok));
                        }
                    }
                    ConnState::Closed => {}
                }
            }
            for (id, p) in &self.externals {
                // Pollable has no clone; poll by reference below via index into a temp vec.
                let _ = (id, p);
            }
            let ext_ids: Vec<u32> = self.externals.keys().copied().collect();
            let now = wasip2::clocks::monotonic_clock::now();
            let elapsed_ms = (now - last_tick) / 1_000_000;
            let wait_ns = self.tick_ms.saturating_sub(elapsed_ms).max(1) * 1_000_000;
            pollables.push(wasip2::clocks::monotonic_clock::subscribe_duration(wait_ns));
            kinds.push((4, 0));

            let mut refs: Vec<&Pollable> = pollables.iter().collect();
            for id in &ext_ids {
                refs.push(&self.externals[id]);
                kinds.push((5, *id));
            }
            let ready: Vec<u32> = poll::poll(&refs);
            // Drop every pollable before touching sockets/streams: WASI refuses to drop a resource
            // that still has live children (its pollables).
            drop(refs);
            drop(pollables);

            let mut events: Vec<Event> = Vec::new();
            let mut flush: Vec<Token> = Vec::new();
            for idx in ready {
                let (kind, tok) = kinds[idx as usize];
                match kind {
                    0 => {
                        if let Some(l) = &self.listener {
                            loop {
                                match l.accept() {
                                    Ok((socket, input, output)) => {
                                        let peer = socket
                                            .remote_address()
                                            .map(from_wasi_addr)
                                            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));
                                        let _ = socket.set_keep_alive_enabled(true);
                                        let token = self.next_token;
                                        self.next_token += 1;
                                        self.conns.insert(token, Conn {
                                            socket,
                                            state: ConnState::Open { input, output },
                                            pending_out: Vec::new(),
                                            close_after_flush: false,
                                        });
                                        events.push(Event::Accepted { token, peer });
                                    }
                                    Err(ErrorCode::WouldBlock) => break,
                                    Err(e) => {
                                        events.push(Event::Closed { token: 0, reason: format!("accept: {}", err(e)) });
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    1 => {
                        if let Some(c) = self.conns.get_mut(&tok) {
                            if let ConnState::Open { input, .. } = &c.state {
                                match input.read(1 << 16) {
                                    Ok(data) if data.is_empty() => {}
                                    Ok(data) => events.push(Event::Data { token: tok, data }),
                                    Err(StreamError::Closed) => {
                                        events.push(Event::Closed { token: tok, reason: "peer closed".into() })
                                    }
                                    Err(StreamError::LastOperationFailed(e)) => {
                                        events.push(Event::Closed { token: tok, reason: e.to_debug_string() })
                                    }
                                }
                            }
                        }
                    }
                    2 => flush.push(tok),
                    5 => {
                        self.externals.remove(&tok);
                        events.push(Event::External { id: tok });
                    }
                    3 => {
                        if let Some(c) = self.conns.get_mut(&tok) {
                            match c.socket.finish_connect() {
                                Ok((input, output)) => {
                                    let _ = c.socket.set_keep_alive_enabled(true);
                                    c.state = ConnState::Open { input, output };
                                    events.push(Event::Connected { token: tok, result: Ok(()) });
                                    flush.push(tok);
                                }
                                Err(ErrorCode::WouldBlock) => {}
                                Err(e) => {
                                    c.state = ConnState::Closed;
                                    events.push(Event::Connected { token: tok, result: Err(err(e)) });
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            if elapsed_ms >= self.tick_ms {
                last_tick = now;
                events.push(Event::Tick);
            }
            for ev in events {
                if let Event::Closed { token, .. } = &ev {
                    if *token != 0 {
                        self.close_now(*token);
                    }
                }
                handler.handle(self, ev);
            }
            // Flush everything with pending output (new data queued by the handler too).
            let tokens: Vec<Token> = self.conns.iter().filter(|(_, c)| !c.pending_out.is_empty() || c.close_after_flush).map(|(t, _)| *t).collect();
            for tok in tokens.into_iter().chain(flush) {
                if let Err(reason) = self.try_flush(tok) {
                    self.close_now(tok);
                    handler.handle(self, Event::Closed { token: tok, reason });
                    continue;
                }
                if let Some(c) = self.conns.get(&tok) {
                    if c.close_after_flush && c.pending_out.is_empty() {
                        self.close_now(tok);
                    }
                }
            }
        }
    }
}
