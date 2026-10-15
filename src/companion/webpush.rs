//! Web Push for Apple's push service: VAPID (RFC 8292) and aes128gcm message encryption
//! (RFC 8291 / RFC 8188). Pure Rust, no I/O; the Windows host sends the request.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

/// Apple rejects encrypted bodies over 4 KB (`PayloadTooLarge`).
pub const MAX_BODY: usize = 4096;
const RECORD_SIZE: u32 = 4096;
/// Apple: "Don't refresh your JWT more frequently than once per hour", and `exp` may be at
/// most a day ahead.
pub const JWT_REUSE_SECS: u64 = 3600;
const JWT_LIFETIME_SECS: u64 = 12 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushError {
    BadKey(&'static str),
    TooLarge(usize),
    BadEndpoint,
}

impl std::fmt::Display for PushError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadKey(what) => write!(f, "bad {what}"),
            Self::TooLarge(n) => write!(f, "encrypted body is {n} bytes; the limit is {MAX_BODY}"),
            Self::BadEndpoint => write!(f, "endpoint is not an allowed push service URL"),
        }
    }
}

pub fn b64u(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn b64u_decode(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).ok()
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    OsRng.fill_bytes(&mut out);
    out
}

/// The application server's VAPID key pair.
#[derive(Clone)]
pub struct VapidKey {
    secret: SecretKey,
}

impl VapidKey {
    pub fn generate() -> Self {
        Self { secret: SecretKey::random(&mut OsRng) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        SecretKey::from_slice(bytes).ok().map(|secret| Self { secret })
    }

    /// The 32-byte private scalar, for the host to store encrypted.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.secret.to_bytes().to_vec()
    }

    /// Uncompressed public point, base64url: what `PushManager.subscribe` takes.
    pub fn public_key(&self) -> String {
        b64u(self.secret.public_key().to_encoded_point(false).as_bytes())
    }

    /// `Authorization` header value for one push service origin.
    pub fn authorization(&self, endpoint: &str, subject: &str, now_unix: u64) -> Result<String, PushError> {
        let aud = origin(endpoint).ok_or(PushError::BadEndpoint)?;
        let head = b64u(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({ "aud": aud, "exp": now_unix + JWT_LIFETIME_SECS, "sub": subject });
        let claims = b64u(claims.to_string().as_bytes());
        let signing = format!("{head}.{claims}");
        let sig: Signature = SigningKey::from(&self.secret).sign(signing.as_bytes());
        Ok(format!("vapid t={signing}.{}, k={}", b64u(&sig.to_bytes()), self.public_key()))
    }
}

/// `scheme://host[:port]` of a URL, or `None` if it isn't one. Control characters are refused
/// anywhere: Windows reads host names as NUL-terminated strings, so a NUL could make the host
/// that was checked differ from the host that is connected to.
pub fn origin(url: &str) -> Option<String> {
    if url.chars().any(char::is_control) {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() || host.contains('@') || host.contains(char::is_whitespace) {
        return None;
    }
    Some(format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// Only Apple's push service, over HTTPS, so a paired phone can't make the PC call other hosts.
/// `extra` holds exact origins allowed in test runs.
pub fn allowed_endpoint(endpoint: &str, extra: &[String]) -> bool {
    let Some(o) = origin(endpoint) else { return false };
    if extra.iter().any(|e| e == &o) {
        return true;
    }
    let Some(host) = o.strip_prefix("https://") else { return false };
    if !host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-') {
        return false;
    }
    host == "push.apple.com" || host.ends_with(".push.apple.com")
}

/// Fixed values that tests pass to reproduce RFC 8291's example; real sends use fresh ones.
pub struct Ephemeral {
    pub secret: [u8; 32],
    pub salt: [u8; 16],
}

/// Encrypt one message for a subscription's `p256dh` and `auth` keys (base64url), as a
/// single aes128gcm record.
pub fn encrypt(plaintext: &[u8], p256dh: &str, auth: &str, fixed: Option<&Ephemeral>) -> Result<Vec<u8>, PushError> {
    let ua_public_bytes = b64u_decode(p256dh).ok_or(PushError::BadKey("p256dh"))?;
    let auth = b64u_decode(auth).ok_or(PushError::BadKey("auth"))?;
    if ua_public_bytes.len() != 65 || ua_public_bytes[0] != 4 {
        return Err(PushError::BadKey("p256dh"));
    }
    if auth.len() != 16 {
        return Err(PushError::BadKey("auth"));
    }
    let ua_public = PublicKey::from_sec1_bytes(&ua_public_bytes).map_err(|_| PushError::BadKey("p256dh"))?;

    let (as_secret, salt) = match fixed {
        Some(f) => (SecretKey::from_slice(&f.secret).map_err(|_| PushError::BadKey("sender key"))?, f.salt),
        None => (SecretKey::random(&mut OsRng), random_bytes::<16>()),
    };
    let as_public = as_secret.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());

    // RFC 8291 3.4: IKM = HKDF(salt = auth, ikm = ecdh_secret, info = "WebPush: info\0" || ua || as)
    let mut key_info = Vec::with_capacity(14 + 130);
    key_info.extend_from_slice(b"WebPush: info\0");
    key_info.extend_from_slice(&ua_public_bytes);
    key_info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&auth), shared.raw_secret_bytes())
        .expand(&key_info, &mut ikm)
        .map_err(|_| PushError::BadKey("hkdf"))?;

    // RFC 8188: content key and nonce from the record salt.
    let prk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| PushError::BadKey("hkdf"))?;
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| PushError::BadKey("hkdf"))?;

    let mut padded = Vec::with_capacity(plaintext.len() + 1);
    padded.extend_from_slice(plaintext);
    padded.push(2); // last-record delimiter
    let body_len = 21 + 65 + padded.len() + 16;
    if padded.len() + 16 > RECORD_SIZE as usize || body_len > MAX_BODY {
        return Err(PushError::TooLarge(body_len));
    }
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| PushError::BadKey("cek"))?;
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce), padded.as_slice())
        .map_err(|_| PushError::BadKey("aes-gcm"))?;

    let mut out = Vec::with_capacity(body_len);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(65);
    out.extend_from_slice(as_public.as_bytes());
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// What a push service's HTTP status means for the queued reminder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 201: Apple has it and will deliver or hold it until the TTL ends.
    Accepted,
    /// 404/410: the subscription is gone; the phone must subscribe again.
    Gone,
    /// Network failure, 429 or 5xx: try again later.
    Retry,
    /// Any other status: the request itself is wrong; retrying won't help.
    Rejected,
}

pub fn classify(status: Option<u16>) -> Outcome {
    match status {
        Some(200..=202) => Outcome::Accepted,
        Some(404 | 410) => Outcome::Gone,
        None | Some(429) | Some(500..=599) => Outcome::Retry,
        Some(_) => Outcome::Rejected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_parsing() {
        assert_eq!(origin("https://web.push.apple.com/abc").as_deref(), Some("https://web.push.apple.com"));
        assert_eq!(origin("http://127.0.0.1:9/x").as_deref(), Some("http://127.0.0.1:9"));
        assert_eq!(origin("ftp://x/y"), None);
        assert_eq!(origin("https://user@evil/x"), None);
    }
}
