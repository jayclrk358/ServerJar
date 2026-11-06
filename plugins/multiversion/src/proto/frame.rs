//! Packet framing: length prefix, optional zlib compression, optional AES/CFB8 encryption.
//! Works on byte buffers fed by the reactor; no blocking I/O here.

use super::buf::{varint_len, Reader, Writer};
use aes::cipher::{AsyncStreamCipher, KeyIvInit};
#[allow(unused_imports)]
use std::convert::TryInto;
use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress, Status};

type Aes128Cfb8Enc = cfb8::Encryptor<aes::Aes128>;
type Aes128Cfb8Dec = cfb8::Decryptor<aes::Aes128>;

pub const MAX_PACKET: usize = 1 << 21; // vanilla: 2 MiB (uncompressed) / 8 MiB compressed cap

/// One direction of a connection's byte pipeline (decrypt → split frames → decompress).
pub struct Inbound {
    buf: Vec<u8>,
    cipher: Option<Cfb8Stream>,
    pub compression: i32, // -1 = off
    inflater: Decompress,
}

impl Default for Inbound {
    fn default() -> Self {
        Self { buf: Vec::new(), cipher: None, compression: -1, inflater: Decompress::new(true) }
    }
}

impl Inbound {
    pub fn enable_encryption(&mut self, key: [u8; 16]) {
        self.cipher = Some(Cfb8Stream::new(key));
    }
    /// Feeds raw bytes from the socket (decrypting as they arrive).
    pub fn feed(&mut self, data: &[u8]) {
        let start = self.buf.len();
        self.buf.extend_from_slice(data);
        if let Some(c) = self.cipher.as_mut() {
            c.decrypt(&mut self.buf[start..]);
        }
    }
    pub fn buffered(&self) -> usize {
        self.buf.len()
    }
    /// Pops the next complete, decompressed packet (id + payload) if available.
    pub fn next_packet(&mut self) -> Result<Option<Vec<u8>>, String> {
        let (len, hdr) = {
            let mut r = Reader::new(&self.buf);
            match r.varint() {
                Ok(len) => (len, r.position()),
                Err(_) if self.buf.len() < 5 => return Ok(None),
                Err(e) => return Err(format!("bad frame length: {e}")),
            }
        };
        if len <= 0 || len as usize > MAX_PACKET * 4 {
            return Err(format!("invalid frame length {len}"));
        }
        let len = len as usize;
        if self.buf.len() < hdr + len {
            return Ok(None);
        }
        let frame: Vec<u8> = self.buf.drain(..hdr + len).skip(hdr).collect();
        if self.compression < 0 {
            return Ok(Some(frame));
        }
        let mut r = Reader::new(&frame);
        let data_len = r.varint().map_err(|e| e.to_string())?;
        if data_len == 0 {
            return Ok(Some(r.remaining().to_vec()));
        }
        if data_len < 0 || data_len as usize > MAX_PACKET * 4 {
            return Err(format!("invalid uncompressed length {data_len}"));
        }
        let mut out = Vec::with_capacity(data_len as usize + 16);
        self.inflater.reset(true);
        let input = r.remaining();
        loop {
            let consumed = self.inflater.total_in() as usize;
            let before_out = self.inflater.total_out();
            let status = self.inflater
                .decompress_vec(&input[consumed..], &mut out, FlushDecompress::Finish)
                .map_err(|e| format!("zlib: {e}"))?;
            match status {
                Status::StreamEnd => break,
                Status::Ok | Status::BufError => {
                    if out.len() > data_len as usize {
                        return Err(format!("decompressed more than the declared {data_len} bytes"));
                    }
                    if self.inflater.total_out() == before_out && self.inflater.total_in() as usize == consumed {
                        return Err("truncated compressed packet".into());
                    }
                    if out.len() == out.capacity() {
                        out.reserve(1024);
                    }
                }
            }
        }
        if out.len() != data_len as usize {
            return Err(format!("decompressed {} bytes, expected {data_len}", out.len()));
        }
        Ok(Some(out))
    }
}

/// Encryption-aware inbound: keeps ciphertext state correctly. Replaces `Inbound::feed` when
/// encryption is on. (Kept separate so the hot path without encryption stays simple.)
pub struct Cfb8Stream {
    key: [u8; 16],
    iv: [u8; 16],
}

impl Cfb8Stream {
    pub fn new(key: [u8; 16]) -> Self {
        Self { key, iv: key }
    }
    /// Decrypts `data` in place.
    pub fn decrypt(&mut self, data: &mut [u8]) {
        if data.is_empty() {
            return;
        }
        let mut last_iv = self.iv;
        // Next IV = last 16 ciphertext bytes (with previous IV as prefix if fewer).
        let mut tail = [0u8; 16];
        let combined: Vec<u8> = last_iv.iter().copied().chain(data.iter().copied()).collect();
        tail.copy_from_slice(&combined[combined.len() - 16..]);
        Aes128Cfb8Dec::new((&self.key).into(), (&last_iv).into()).decrypt(data);
        last_iv = tail;
        self.iv = last_iv;
    }
    /// Encrypts `data` in place.
    pub fn encrypt(&mut self, data: &mut [u8]) {
        if data.is_empty() {
            return;
        }
        Aes128Cfb8Enc::new((&self.key).into(), (&self.iv).into()).encrypt(data);
        let combined: Vec<u8> = self.iv.iter().copied().chain(data.iter().copied()).collect();
        self.iv.copy_from_slice(&combined[combined.len() - 16..]);
    }
}

/// Builds outbound frames (compress → length prefix → encrypt).
pub struct Outbound {
    pub compression: i32,
    deflater: Compress,
    cipher: Option<Cfb8Stream>,
}

impl Default for Outbound {
    fn default() -> Self {
        Self { compression: -1, deflater: Compress::new(Compression::new(1), true), cipher: None }
    }
}

impl Outbound {
    /// zlib-compresses `data` completely. `compress_vec` only fills the spare capacity of the
    /// output vector, so keep going (growing it) until the stream is finished.
    fn deflate(&mut self, data: &[u8]) -> Vec<u8> {
        self.deflater.reset();
        let mut out = Vec::with_capacity(data.len() / 2 + 64);
        loop {
            let before_in = self.deflater.total_in();
            let before_out = self.deflater.total_out();
            let status = self.deflater.compress_vec(&data[before_in as usize..], &mut out, FlushCompress::Finish).expect("zlib compression");
            match status {
                Status::StreamEnd => return out,
                Status::Ok | Status::BufError => {
                    if self.deflater.total_out() == before_out && self.deflater.total_in() == before_in {
                        out.reserve(data.len() + 64);
                    } else if out.len() == out.capacity() {
                        out.reserve(out.capacity().max(1024));
                    }
                }
            }
        }
    }

    pub fn enable_encryption(&mut self, key: [u8; 16]) {
        self.cipher = Some(Cfb8Stream::new(key));
    }
    /// Encrypts bytes that `frame` appended, right before they go to the socket.
    pub fn seal(&mut self, out: &mut [u8]) {
        if let Some(c) = self.cipher.as_mut() {
            c.encrypt(out);
        }
    }
    /// Frames one packet (id+payload) into `out`.
    pub fn frame(&mut self, packet: &[u8], out: &mut Vec<u8>) {
        if self.compression < 0 {
            let mut w = Writer::with_capacity(packet.len() + 5);
            w.varint(packet.len() as i32);
            w.raw(packet);
            out.extend_from_slice(&w.buf);
            return;
        }
        if packet.len() < self.compression as usize {
            let mut w = Writer::with_capacity(packet.len() + 6);
            w.varint(packet.len() as i32 + 1);
            w.varint(0);
            w.raw(packet);
            out.extend_from_slice(&w.buf);
            return;
        }
        let compressed = self.deflate(packet);
        let inner_len = varint_len(packet.len() as i32) + compressed.len();
        let mut w = Writer::with_capacity(inner_len + 5);
        w.varint(inner_len as i32);
        w.varint(packet.len() as i32);
        w.raw(&compressed);
        out.extend_from_slice(&w.buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_roundtrip_plain_and_compressed() {
        for threshold in [-1, 0, 256] {
            let mut ob = Outbound { compression: threshold, ..Default::default() };
            let mut ib = Inbound { compression: threshold, ..Default::default() };
            let small = vec![7u8; 10];
            let big = vec![3u8; 5000];
            let mut wire = Vec::new();
            ob.frame(&small, &mut wire);
            ob.frame(&big, &mut wire);
            ib.feed(&wire[..3]);
            assert!(ib.next_packet().unwrap().is_none() || wire.len() < 4);
            ib.feed(&wire[3..]);
            assert_eq!(ib.next_packet().unwrap().unwrap(), small);
            assert_eq!(ib.next_packet().unwrap().unwrap(), big);
            assert!(ib.next_packet().unwrap().is_none());
        }
    }
    #[test]
    fn incompressible_packets_survive_compression() {
        // Pseudo-random bytes don't shrink under zlib; the frame must still carry all of them.
        let mut x: u32 = 0x1234_5678;
        let data: Vec<u8> = (0..1146)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        let mut ob = Outbound { compression: 256, ..Default::default() };
        let mut ib = Inbound { compression: 256, ..Default::default() };
        let mut wire = Vec::new();
        ob.frame(&data, &mut wire);
        ib.feed(&wire);
        assert_eq!(ib.next_packet().unwrap().unwrap(), data);
        // and a large one that compresses well
        let big = vec![7u8; 200_000];
        let mut wire = Vec::new();
        ob.frame(&big, &mut wire);
        ib.feed(&wire);
        assert_eq!(ib.next_packet().unwrap().unwrap(), big);
    }

    #[test]
    fn cfb8_roundtrip() {
        let key = [9u8; 16];
        let mut enc = Cfb8Stream::new(key);
        let mut dec = Cfb8Stream::new(key);
        let msg = b"hello minecraft encryption stream".to_vec();
        let mut a = msg[..5].to_vec();
        let mut b = msg[5..].to_vec();
        enc.encrypt(&mut a);
        enc.encrypt(&mut b);
        let mut c = [a.clone(), b.clone()].concat();
        // decrypt in different chunking
        let (x, y) = c.split_at_mut(17);
        dec.decrypt(x);
        dec.decrypt(y);
        assert_eq!(c, msg);
    }
}
