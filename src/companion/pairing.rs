//! Pairing codes and device credentials. The PC keeps only a SHA-256 hash of each device
//! secret.

use super::webpush::{b64u, random_bytes};
use sha2::{Digest, Sha256};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ"; // Crockford: no I, L, O, U
pub const CODE_LEN: usize = 8; // 40 bits
pub const CODE_TTL_MS: u64 = 5 * 60_000;
pub const CODE_MAX_ATTEMPTS: u32 = 5;

pub fn new_code() -> String {
    random_bytes::<CODE_LEN>().iter().map(|b| ALPHABET[(b & 31) as usize] as char).collect()
}

/// Accept what people type: lower case, spaces, dashes, and O/I/L for 0/1.
pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .take(64)
        .collect()
}

/// A 256-bit device secret, base64url.
pub fn new_token() -> String {
    b64u(&random_bytes::<32>())
}

pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// Equal-length comparison that doesn't stop at the first difference.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn token_matches(token: &str, hash_hex: &str) -> bool {
    constant_time_eq(hash_token(token).as_bytes(), hash_hex.as_bytes())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveCode {
    pub code: String,
    pub expires_ms: u64,
    pub attempts: u32,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Redeem {
    Ok,
    /// No live code: none made, already used, expired or burned.
    NoCode,
    Wrong,
}

/// One pairing code at a time. Single use, five minutes, five wrong tries. A correct code
/// only creates a request; the PC still has to approve it.
#[derive(Default)]
pub struct Pairing {
    active: Option<ActiveCode>,
}

impl Pairing {
    pub fn start(&mut self, now_ms: u64) -> ActiveCode {
        let code = ActiveCode { code: new_code(), expires_ms: now_ms + CODE_TTL_MS, attempts: 0 };
        self.active = Some(code.clone());
        code
    }

    pub fn current(&mut self, now_ms: u64) -> Option<&ActiveCode> {
        if self.active.as_ref().is_some_and(|a| now_ms >= a.expires_ms) {
            self.active = None;
        }
        self.active.as_ref()
    }

    pub fn redeem(&mut self, input: &str, now_ms: u64) -> Redeem {
        if self.current(now_ms).is_none() {
            return Redeem::NoCode;
        }
        let live = self.active.as_mut().expect("checked above");
        if constant_time_eq(normalize_code(input).as_bytes(), live.code.as_bytes()) {
            self.active = None;
            return Redeem::Ok;
        }
        live.attempts += 1;
        if live.attempts >= CODE_MAX_ATTEMPTS {
            self.active = None;
        }
        Redeem::Wrong
    }

    pub fn cancel(&mut self) {
        self.active = None;
    }
}
