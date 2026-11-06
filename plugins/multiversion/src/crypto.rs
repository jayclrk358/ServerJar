//! Login encryption (RSA key exchange, AES key), the Mojang session check, and HMAC signing for
//! the Velocity-style forwarding payload that Pumpkin verifies.

use hmac::{Hmac, Mac};
use rsa::pkcs8::EncodePublicKey;
use rsa::{Pkcs1v15Encrypt, RsaPrivateKey, RsaPublicKey};
use sha1::{Digest, Sha1};
use sha2::Sha256;

pub struct ServerKeys {
    private: RsaPrivateKey,
    /// DER-encoded SubjectPublicKeyInfo, as sent in the encryption request.
    pub public_der: Vec<u8>,
}

impl ServerKeys {
    pub fn generate() -> Result<Self, String> {
        let mut rng = WasiRng;
        let private = RsaPrivateKey::new(&mut rng, 1024).map_err(|e| format!("rsa keygen: {e}"))?;
        let public = RsaPublicKey::from(&private);
        let public_der = public.to_public_key_der().map_err(|e| e.to_string())?.as_bytes().to_vec();
        Ok(Self { private, public_der })
    }
    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, String> {
        self.private.decrypt(Pkcs1v15Encrypt, data).map_err(|e| format!("rsa decrypt: {e}"))
    }
}

/// `rand_core` RNG backed by WASI random (via getrandom's custom backend below).
pub struct WasiRng;

impl rand_core::RngCore for WasiRng {
    fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }
    fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        let bytes = crate::platform::random_bytes(dest.len());
        dest.copy_from_slice(&bytes);
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl rand_core::CryptoRng for WasiRng {}

pub fn random_bytes(n: usize) -> Vec<u8> {
    crate::platform::random_bytes(n)
}

/// URL-safe random token for secrets.
pub fn random_token() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    random_bytes(32).iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect()
}

/// Vanilla's server hash: hex of SHA-1(serverId + sharedSecret + publicKeyDer) as a signed
/// two's-complement big integer.
pub fn server_hash(server_id: &str, shared_secret: &[u8], public_der: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(server_id.as_bytes());
    h.update(shared_secret);
    h.update(public_der);
    let digest: [u8; 20] = h.finalize().into();
    let negative = digest[0] & 0x80 != 0;
    let mut bytes = digest;
    if negative {
        // two's complement negate
        let mut carry = true;
        for b in bytes.iter_mut().rev() {
            *b = !*b;
            if carry {
                let (v, c) = b.overflowing_add(1);
                *b = v;
                carry = c;
            }
        }
    }
    let hex = hex::encode(bytes);
    let trimmed = hex.trim_start_matches('0');
    let trimmed = if trimmed.is_empty() { "0" } else { trimmed };
    if negative { format!("-{trimmed}") } else { trimmed.to_string() }
}

pub fn hmac_sha256(secret: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_hash_known_vectors() {
        // From the protocol wiki
        assert_eq!(server_hash("Notch", b"", b""), "4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48");
        assert_eq!(server_hash("jeb_", b"", b""), "-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1");
        assert_eq!(server_hash("simon", b"", b""), "88e16a1019277b15d58faf0541e11910eb756f6");
    }
}
