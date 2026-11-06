//! Minecraft wire primitives: VarInt/VarLong, strings, UUIDs, positions.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("unexpected end of packet")]
    Eof,
    #[error("varint too long")]
    VarIntTooLong,
    #[error("string too long ({0} > {1})")]
    StringTooLong(usize, usize),
    #[error("invalid utf-8")]
    Utf8,
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, DecodeError>;

/// Cursor over a packet payload.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    pub fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }
    pub fn position(&self) -> usize {
        self.pos
    }
    pub fn set_position(&mut self, pos: usize) {
        self.pos = pos.min(self.buf.len());
    }
    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }
    pub fn len(&self) -> usize {
        self.buf.len() - self.pos
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.pos + n > self.buf.len() {
            return Err(DecodeError::Eof);
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.u8()? != 0)
    }
    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.i32()? as u32))
    }
    pub fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_bits(self.i64()? as u64))
    }
    pub fn varint(&mut self) -> Result<i32> {
        let mut value: u32 = 0;
        for i in 0..5 {
            let b = self.u8()?;
            value |= u32::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(value as i32);
            }
        }
        Err(DecodeError::VarIntTooLong)
    }
    pub fn varlong(&mut self) -> Result<i64> {
        let mut value: u64 = 0;
        for i in 0..10 {
            let b = self.u8()?;
            value |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Ok(value as i64);
            }
        }
        Err(DecodeError::VarIntTooLong)
    }
    pub fn string_max(&mut self, max_chars: usize) -> Result<&'a str> {
        let len = self.varint()?;
        if len < 0 || len as usize > max_chars * 3 {
            return Err(DecodeError::StringTooLong(len.max(0) as usize, max_chars * 3));
        }
        let bytes = self.bytes(len as usize)?;
        std::str::from_utf8(bytes).map_err(|_| DecodeError::Utf8)
    }
    pub fn string(&mut self) -> Result<&'a str> {
        self.string_max(32767)
    }
    pub fn uuid(&mut self) -> Result<u128> {
        Ok(u128::from_be_bytes(self.bytes(16)?.try_into().unwrap()))
    }
    pub fn byte_array(&mut self) -> Result<&'a [u8]> {
        let len = self.varint()?;
        if len < 0 {
            return Err(DecodeError::Other("negative array length".into()));
        }
        self.bytes(len as usize)
    }
    pub fn long_array(&mut self) -> Result<Vec<i64>> {
        let len = self.varint()?;
        if len < 0 {
            return Err(DecodeError::Other("negative array length".into()));
        }
        (0..len).map(|_| self.i64()).collect()
    }
    /// VarInt-prefixed list.
    pub fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> Result<T>) -> Result<Vec<T>> {
        let len = self.varint()?;
        if len < 0 {
            return Err(DecodeError::Other("negative list length".into()));
        }
        (0..len).map(|_| f(self)).collect()
    }
    pub fn option<T>(&mut self, f: impl FnOnce(&mut Self) -> Result<T>) -> Result<Option<T>> {
        if self.bool()? { Ok(Some(f(self)?)) } else { Ok(None) }
    }
    /// Copies raw bytes of a sub-read into `out` (passthrough helper).
    pub fn copy_to(&mut self, out: &mut Writer, f: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        let start = self.pos;
        f(self)?;
        out.raw(&self.buf[start..self.pos]);
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(n: usize) -> Self {
        Self { buf: Vec::with_capacity(n) }
    }
    pub fn into_inner(self) -> Vec<u8> {
        self.buf
    }
    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }
    pub fn i8(&mut self, v: i8) -> &mut Self {
        self.u8(v as u8)
    }
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(u8::from(v))
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.raw(&v.to_be_bytes())
    }
    pub fn i16(&mut self, v: i16) -> &mut Self {
        self.u16(v as u16)
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.raw(&v.to_be_bytes())
    }
    pub fn i64(&mut self, v: i64) -> &mut Self {
        self.raw(&v.to_be_bytes())
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.i32(v.to_bits() as i32)
    }
    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.i64(v.to_bits() as i64)
    }
    pub fn varint(&mut self, v: i32) -> &mut Self {
        let mut v = v as u32;
        loop {
            if v & !0x7f == 0 {
                self.buf.push(v as u8);
                return self;
            }
            self.buf.push((v as u8 & 0x7f) | 0x80);
            v >>= 7;
        }
    }
    pub fn varlong(&mut self, v: i64) -> &mut Self {
        let mut v = v as u64;
        loop {
            if v & !0x7f == 0 {
                self.buf.push(v as u8);
                return self;
            }
            self.buf.push((v as u8 & 0x7f) | 0x80);
            v >>= 7;
        }
    }
    pub fn string(&mut self, s: &str) -> &mut Self {
        self.varint(s.len() as i32);
        self.raw(s.as_bytes())
    }
    pub fn uuid(&mut self, v: u128) -> &mut Self {
        self.raw(&v.to_be_bytes())
    }
    pub fn byte_array(&mut self, b: &[u8]) -> &mut Self {
        self.varint(b.len() as i32);
        self.raw(b)
    }
    pub fn long_array(&mut self, v: &[i64]) -> &mut Self {
        self.varint(v.len() as i32);
        for x in v {
            self.i64(*x);
        }
        self
    }
    pub fn option<T>(&mut self, v: Option<T>, f: impl FnOnce(&mut Self, T)) -> &mut Self {
        match v {
            Some(v) => {
                self.bool(true);
                f(self, v);
            }
            None => {
                self.bool(false);
            }
        }
        self
    }
}

pub fn varint_len(v: i32) -> usize {
    let v = v as u32;
    match v {
        0..=0x7f => 1,
        0x80..=0x3fff => 2,
        0x4000..=0x1f_ffff => 3,
        0x20_0000..=0xfff_ffff => 4,
        _ => 5,
    }
}

pub fn uuid_to_string(id: u128) -> String {
    let b = id.to_be_bytes();
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn uuid_from_str(s: &str) -> Option<u128> {
    let clean: String = s.chars().filter(|c| *c != '-').collect();
    if clean.len() != 32 {
        return None;
    }
    u128::from_str_radix(&clean, 16).ok()
}

/// Offline-mode UUID: UUIDv3 of "OfflinePlayer:<name>", like vanilla.
pub fn offline_uuid(name: &str) -> u128 {
    let digest = md5(format!("OfflinePlayer:{name}").as_bytes());
    let mut b = digest;
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    u128::from_be_bytes(b)
}

/// Tiny MD5 (only needed for offline UUIDs; avoids another dependency).
pub fn md5(input: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
        14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15,
        21, 6, 10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let (mut a0, mut b0, mut c0, mut d0) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    let mut msg = input.to_vec();
    let bit_len = (input.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_le_bytes());
    for chunk in msg.chunks(64) {
        let m: Vec<u32> = chunk.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);
        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | (!b & d), i),
                16..=31 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let f = f.wrapping_add(a).wrapping_add(k[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(S[i]));
        }
        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&a0.to_le_bytes());
    out[4..8].copy_from_slice(&b0.to_le_bytes());
    out[8..12].copy_from_slice(&c0.to_le_bytes());
    out[12..16].copy_from_slice(&d0.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn varint_roundtrip() {
        for v in [0, 1, 127, 128, 255, 25565, -1, i32::MAX, i32::MIN] {
            let mut w = Writer::new();
            w.varint(v);
            assert_eq!(w.buf.len(), varint_len(v));
            assert_eq!(Reader::new(&w.buf).varint().unwrap(), v);
        }
    }
    #[test]
    fn md5_known() {
        assert_eq!(hex::encode(md5(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(hex::encode(md5(b"The quick brown fox jumps over the lazy dog")), "9e107d9d372bb6826bd81d3542a419d6");
    }
    #[test]
    fn offline_uuid_notch() {
        // Known offline UUID for "Notch"
        assert_eq!(uuid_to_string(offline_uuid("Notch")), "b50ad385-829d-3141-a216-7e7d7539ba7f");
    }
}
