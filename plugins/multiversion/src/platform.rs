//! Thin platform layer so the protocol code and its unit tests also build natively.

#[cfg(target_arch = "wasm32")]
pub fn random_bytes(n: usize) -> Vec<u8> {
    wasip2::random::random::get_random_bytes(n as u64)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    getrandom::getrandom(&mut v).expect("os random");
    v
}

/// Monotonic milliseconds.
#[cfg(target_arch = "wasm32")]
pub fn now_millis() -> u64 {
    wasip2::clocks::monotonic_clock::now() / 1_000_000
}

#[cfg(not(target_arch = "wasm32"))]
pub fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// getrandom 0.2 "custom" backend so dependencies that call getrandom() work under WASI 0.2.
#[cfg(target_arch = "wasm32")]
fn wasi_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    let bytes = wasip2::random::random::get_random_bytes(buf.len() as u64);
    buf.copy_from_slice(&bytes);
    Ok(())
}
#[cfg(target_arch = "wasm32")]
getrandom::register_custom_getrandom!(wasi_getrandom);
