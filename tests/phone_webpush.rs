use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use hkdf::Hkdf;
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use sha2::Sha256;
use unseat::companion::webpush::{
    allowed_endpoint, b64u, b64u_decode, classify, encrypt, Ephemeral, Outcome, PushError, VapidKey, MAX_BODY,
};

// RFC 8291 section 5.
const PLAINTEXT: &str = "When I grow up, I want to be a watermelon";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const UA_PUBLIC: &str = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const BODY: &str = concat!(
    "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml",
    "mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT",
    "pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
);

/// The receiving phone's side of RFC 8291, to check what the sender produced.
pub fn decrypt(body: &[u8], ua_private: &[u8], auth: &[u8]) -> Vec<u8> {
    let ua = SecretKey::from_slice(ua_private).unwrap();
    let ua_public = ua.public_key().to_encoded_point(false);
    let salt = &body[..16];
    let idlen = body[20] as usize;
    let as_public = &body[21..21 + idlen];
    let ct = &body[21 + idlen..];
    let shared = p256::ecdh::diffie_hellman(ua.to_nonzero_scalar(), PublicKey::from_sec1_bytes(as_public).unwrap().as_affine());
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public.as_bytes());
    info.extend_from_slice(as_public);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&info, &mut ikm).unwrap();
    let prk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
    prk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).unwrap();
    prk.expand(b"Content-Encoding: nonce\0", &mut nonce).unwrap();
    let mut padded = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(&Nonce::from(nonce), ct).unwrap();
    while padded.last() == Some(&0) {
        padded.pop();
    }
    assert_eq!(padded.pop(), Some(2), "last-record delimiter");
    padded
}

fn subscriber() -> (SecretKey, [u8; 16], String, String) {
    let sk = SecretKey::random(&mut rand_core::OsRng);
    let auth = unseat::companion::webpush::random_bytes::<16>();
    let p256dh = b64u(sk.public_key().to_encoded_point(false).as_bytes());
    (sk.clone(), auth, p256dh, b64u(&auth))
}

#[test]
fn reproduces_the_rfc_8291_example_byte_for_byte() {
    let expected = b64u_decode(BODY).unwrap();
    // 86-byte header + 41-byte text + delimiter + 16-byte tag. (The RFC's request line says
    // Content-Length: 145, which doesn't match its own body.)
    assert_eq!(expected.len(), 144);
    let fixed = Ephemeral {
        secret: b64u_decode(AS_PRIVATE).unwrap().try_into().unwrap(),
        salt: expected[..16].try_into().unwrap(),
    };
    let out = encrypt(PLAINTEXT.as_bytes(), UA_PUBLIC, AUTH, Some(&fixed)).unwrap();
    assert_eq!(b64u(&out), BODY);
}

#[test]
fn the_rfc_example_decrypts_with_the_receivers_key() {
    let pt = decrypt(&b64u_decode(BODY).unwrap(), &b64u_decode(UA_PRIVATE).unwrap(), &b64u_decode(AUTH).unwrap());
    assert_eq!(pt, PLAINTEXT.as_bytes());
}

#[test]
fn every_message_gets_a_fresh_salt_and_sender_key() {
    let (sk, auth, p256dh, auth_b64) = subscriber();
    let a = encrypt(b"hello", &p256dh, &auth_b64, None).unwrap();
    let b = encrypt(b"hello", &p256dh, &auth_b64, None).unwrap();
    assert_ne!(a[..16], b[..16]);
    assert_ne!(a[21..86], b[21..86]);
    assert_eq!(decrypt(&a, &sk.to_bytes(), &auth), b"hello");
}

#[test]
fn malformed_keys_are_rejected() {
    let (_, _, p256dh, auth) = subscriber();
    assert_eq!(encrypt(b"x", &p256dh, &b64u(&[0u8; 8]), None), Err(PushError::BadKey("auth")));
    assert_eq!(encrypt(b"x", &b64u(&[2u8; 33]), &auth, None), Err(PushError::BadKey("p256dh")));
    let mut off_curve = [4u8; 65];
    off_curve[1] = 1;
    assert_eq!(encrypt(b"x", &b64u(&off_curve), &auth, None), Err(PushError::BadKey("p256dh")));
    assert_eq!(encrypt(b"x", "not base64 !!", &auth, None), Err(PushError::BadKey("p256dh")));
}

#[test]
fn bodies_over_apples_4_kb_limit_are_refused() {
    let (_, _, p256dh, auth) = subscriber();
    let max_plain = MAX_BODY - 21 - 65 - 16 - 1;
    assert!(encrypt(&vec![b'a'; max_plain], &p256dh, &auth, None).is_ok());
    assert!(matches!(encrypt(&vec![b'a'; max_plain + 1], &p256dh, &auth, None), Err(PushError::TooLarge(_))));
}

#[test]
fn vapid_header_is_a_valid_es256_jwt_for_the_push_origin() {
    let key = VapidKey::generate();
    let now = 1_800_000_000;
    let h = key.authorization("https://web.push.apple.com/QGuQyavXutnMPQ2eCi1d8Ft6V8pnTO5L", "https://github.com/flyingsatoshi/unseat", now).unwrap();
    let (t, k) = h.strip_prefix("vapid t=").unwrap().split_once(", k=").unwrap();
    assert_eq!(k, key.public_key());
    let parts: Vec<&str> = t.split('.').collect();
    assert_eq!(parts.len(), 3);
    let head: serde_json::Value = serde_json::from_slice(&b64u_decode(parts[0]).unwrap()).unwrap();
    assert_eq!(head, serde_json::json!({ "typ": "JWT", "alg": "ES256" }));
    let claims: serde_json::Value = serde_json::from_slice(&b64u_decode(parts[1]).unwrap()).unwrap();
    assert_eq!(claims["aud"], "https://web.push.apple.com");
    assert_eq!(claims["sub"], "https://github.com/flyingsatoshi/unseat");
    let exp = claims["exp"].as_u64().unwrap();
    assert!(exp > now && exp <= now + 24 * 3600, "Apple rejects exp more than a day ahead");
    let vk = VerifyingKey::from_sec1_bytes(&b64u_decode(k).unwrap()).unwrap();
    let sig = Signature::from_slice(&b64u_decode(parts[2]).unwrap()).unwrap();
    vk.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).unwrap();
}

#[test]
fn vapid_key_round_trips_through_its_stored_bytes() {
    let key = VapidKey::generate();
    let again = VapidKey::from_bytes(&key.to_bytes()).unwrap();
    assert_eq!(again.public_key(), key.public_key());
    assert!(VapidKey::from_bytes(&[0u8; 32]).is_none());
}

#[test]
fn only_apples_push_service_is_an_allowed_endpoint() {
    let none: Vec<String> = vec![];
    assert!(allowed_endpoint("https://web.push.apple.com/abc", &none));
    assert!(allowed_endpoint("https://WEB.push.apple.com/abc", &none));
    assert!(!allowed_endpoint("http://web.push.apple.com/abc", &none));
    assert!(!allowed_endpoint("https://web.push.apple.com:8443/abc", &none));
    assert!(!allowed_endpoint("https://push.apple.com.evil.example/abc", &none));
    assert!(!allowed_endpoint("https://evilpush.apple.com.example/abc", &none));
    assert!(!allowed_endpoint("https://fcm.googleapis.com/fcm/send/abc", &none));
    assert!(!allowed_endpoint("https://127.0.0.1:8791/admin/revoke", &none));
    assert!(!allowed_endpoint("https://x@web.push.apple.com/abc", &none));
    assert!(!allowed_endpoint("not a url", &none));
    // A NUL ends the host for Windows, which would then connect to 10.0.0.5, not Apple.
    assert!(!allowed_endpoint("https://10.0.0.5\u{0}.push.apple.com/abc", &none));
    assert!(!allowed_endpoint("https://web.push.apple.com\u{0}/abc", &none));
    assert!(!allowed_endpoint("https://web.push.apple.com/a\r\nX-Evil: 1", &none));
    assert!(!allowed_endpoint("https://w\u{e9}b.push.apple.com/abc", &none));
    let fake = ["http://127.0.0.1:9999".to_string()];
    assert!(allowed_endpoint("http://127.0.0.1:9999/push/Test iPhone", &fake));
    assert!(!allowed_endpoint("http://127.0.0.1:9999/push/\u{0}x", &fake));
    assert!(allowed_endpoint("http://127.0.0.1:9999/push/1", &["http://127.0.0.1:9999".to_string()]));
}

#[test]
fn push_service_statuses_map_to_actions() {
    assert_eq!(classify(Some(201)), Outcome::Accepted);
    assert_eq!(classify(Some(410)), Outcome::Gone);
    assert_eq!(classify(Some(404)), Outcome::Gone);
    assert_eq!(classify(Some(429)), Outcome::Retry);
    assert_eq!(classify(Some(503)), Outcome::Retry);
    assert_eq!(classify(None), Outcome::Retry);
    assert_eq!(classify(Some(400)), Outcome::Rejected);
    assert_eq!(classify(Some(403)), Outcome::Rejected);
    assert_eq!(classify(Some(413)), Outcome::Rejected);
}
