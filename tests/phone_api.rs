use unseat::companion::api::{Companion, Effect, ADMIN_HEADER, LOGIN_HEADER};
use unseat::companion::http::{self, ParseError, Request, Response};
use unseat::companion::pairing::{hash_token, normalize_code, Pairing, Redeem, CODE_MAX_ATTEMPTS, CODE_TTL_MS};
use unseat::companion::store::Store;
use unseat::companion::webpush::{b64u, VapidKey};
use unseat::PhoneAlerts;

const LOGIN: &str = "owner@example.test";
const PUBLIC: &str = "pc.example.ts.net";

fn companion() -> Companion {
    let mut c = Companion::new(Store::default(), VapidKey::generate().public_key(), 8790, 8791);
    c.public_host = Some(PUBLIC.into());
    c.app.enabled = true;
    c
}

fn req(method: &str, path: &str, host: &str, headers: &[(&str, &str)], body: Option<serde_json::Value>) -> Request {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let mut h: Vec<(String, String)> = vec![("host".into(), host.into())];
    h.extend(headers.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    if body.is_some() {
        h.push(("content-type".into(), "application/json".into()));
    }
    Request { method: method.into(), path: path.into(), query: query.into(), headers: h, body: body.map(|b| b.to_string().into_bytes()).unwrap_or_default() }
}

fn phone(c: &mut Companion, method: &str, path: &str, token: Option<&str>, body: Option<serde_json::Value>) -> (Response, Vec<Effect>) {
    let auth = token.map(|t| format!("Bearer {t}"));
    let mut headers = vec![(LOGIN_HEADER, LOGIN)];
    if let Some(a) = auth.as_deref() {
        headers.push(("authorization", a));
    }
    c.handle_phone(&req(method, path, PUBLIC, &headers, body), 1_000)
}

fn admin(c: &mut Companion, method: &str, path: &str, body: Option<serde_json::Value>) -> (Response, Vec<Effect>) {
    let token = c.admin_token.clone();
    c.handle_admin(&req(method, path, "127.0.0.1:8791", &[(ADMIN_HEADER, &token)], body), 1_000)
}

fn json(r: &Response) -> serde_json::Value {
    serde_json::from_slice(&r.body).unwrap()
}

/// A pairing request as the phone sends it: the code, a name and the hash of its own secret.
fn pair_body(code: &str, secret: &str) -> serde_json::Value {
    serde_json::json!({ "code": code, "name": "Test phone", "secretHash": hash_token(secret) })
}

/// Pair one phone and return its secret (the bearer token it made itself).
fn paired(c: &mut Companion) -> String {
    let secret = b64u(&rand_core::RngCore::next_u64(&mut rand_core::OsRng).to_le_bytes());
    let code = json(&admin(c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    let r = phone(c, "POST", "/api/pair/request", None, Some(pair_body(&code, &secret)));
    let req_id = json(&r.0)["reqId"].as_str().unwrap().to_string();
    let (r, fx) = admin(c, "POST", "/admin/pair/decide", Some(serde_json::json!({ "reqId": req_id, "approve": true })));
    assert_eq!(r.status, 200);
    assert_eq!(fx, vec![Effect::Persist]);
    let r = json(&phone(c, "GET", &format!("/api/pair/poll?reqId={req_id}"), None, None).0);
    assert_eq!(r["status"], "approved");
    assert!(r.get("token").is_none(), "no secret is ever handed out");
    secret
}

fn apple_subscription() -> serde_json::Value {
    let sk = p256::SecretKey::random(&mut rand_core::OsRng);
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    serde_json::json!({ "subscription": {
        "endpoint": "https://web.push.apple.com/QGuQyavXutnMPQ2eCi1d8Ft6V8pnTO5L",
        "keys": { "p256dh": b64u(sk.public_key().to_encoded_point(false).as_bytes()), "auth": b64u(&[7u8; 16]) }
    }})
}

// ---------------------------------------------------------------- access rules

#[test]
fn phone_side_refuses_requests_that_did_not_come_through_the_tailnet() {
    let mut c = companion();
    let r = c.handle_phone(&req("GET", "/", PUBLIC, &[], None), 0).0;
    assert_eq!(r.status, 403);
    let r = c.handle_phone(&req("GET", "/", PUBLIC, &[(LOGIN_HEADER, "")], None), 0).0;
    assert_eq!(r.status, 403);
}

#[test]
fn phone_side_refuses_unknown_hosts() {
    let mut c = companion();
    let r = c.handle_phone(&req("GET", "/", "evil.example", &[(LOGIN_HEADER, LOGIN)], None), 0).0;
    assert_eq!(r.status, 421);
}

#[test]
fn phone_side_serves_the_app_and_never_the_admin_api() {
    let mut c = companion();
    let r = phone(&mut c, "GET", "/", None, None).0;
    assert_eq!(r.status, 200);
    assert!(String::from_utf8_lossy(&r.to_bytes()).contains("Content-Security-Policy: default-src 'self'"));
    assert_eq!(phone(&mut c, "GET", "/admin/state", None, None).0.status, 404);
    assert_eq!(phone(&mut c, "POST", "/admin/pair/new", None, Some(serde_json::json!({}))).0.status, 404);
    assert_eq!(phone(&mut c, "GET", "/admin.js", None, None).0.status, 404);
}

#[test]
fn admin_side_needs_loopback_host_the_run_key_and_no_tailnet_headers() {
    let mut c = companion();
    let key = c.admin_token.clone();
    assert_eq!(c.handle_admin(&req("GET", "/admin/state", PUBLIC, &[(ADMIN_HEADER, &key)], None), 0).0.status, 421);
    assert_eq!(c.handle_admin(&req("GET", "/admin/state", "127.0.0.1:8791", &[], None), 0).0.status, 403);
    assert_eq!(c.handle_admin(&req("GET", "/admin/state", "127.0.0.1:8791", &[(ADMIN_HEADER, "guess")], None), 0).0.status, 403);
    assert_eq!(c.handle_admin(&req("GET", "/admin/state", "127.0.0.1:8791", &[(ADMIN_HEADER, &key), (LOGIN_HEADER, LOGIN)], None), 0).0.status, 403);
    assert_eq!(c.handle_admin(&req("GET", "/admin/state", "127.0.0.1:8791", &[(ADMIN_HEADER, &key)], None), 0).0.status, 200);
    // The page itself is not secret; its data is.
    assert_eq!(c.handle_admin(&req("GET", "/", "127.0.0.1:8791", &[], None), 0).0.status, 200);
}

#[test]
fn the_qr_needs_the_key_in_the_url_because_images_cant_send_headers() {
    let mut c = companion();
    admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({})));
    let key = c.admin_token.clone();
    let ok = c.handle_admin(&req("GET", &format!("/admin/qr.svg?t={key}"), "127.0.0.1:8791", &[], None), 0).0;
    assert_eq!(ok.status, 200);
    assert!(String::from_utf8_lossy(&ok.body).starts_with("<svg"));
    assert_eq!(c.handle_admin(&req("GET", "/admin/qr.svg?t=nope", "127.0.0.1:8791", &[], None), 0).0.status, 403);
}

// ---------------------------------------------------------------- pairing

#[test]
fn pairing_needs_a_code_and_pc_approval_and_the_phone_keeps_its_own_secret() {
    let mut c = companion();
    let token = paired(&mut c);
    assert_eq!(phone(&mut c, "GET", "/api/status", Some(&token), None).0.status, 200);
    assert_eq!(c.store.devices.len(), 1);
    let d = &c.store.devices[0];
    assert_eq!(d.login, LOGIN);
    assert_eq!(d.name, "Test phone");
    assert_eq!(d.token_hash, hash_token(&token));
    assert!(!serde_json::to_string(&c.store).unwrap().contains(&token), "only the hash is stored");
}

#[test]
fn wrong_codes_burn_the_code() {
    let mut c = companion();
    admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({})));
    for _ in 0..CODE_MAX_ATTEMPTS {
        let r = phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body("ZZZZZZZZ", "s")));
        assert_eq!(r.0.status, 403);
    }
    let r = phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body("ZZZZZZZZ", "s")));
    assert!(json(&r.0)["error"].as_str().unwrap().contains("no active code"));
}

#[test]
fn pairing_attempts_are_rate_limited() {
    let mut c = companion();
    let mut last = 0;
    for _ in 0..12 {
        admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({})));
        last = phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body("nope", "s"))).0.status;
    }
    assert_eq!(last, 429);
}

#[test]
fn a_declined_phone_gets_no_token() {
    let mut c = companion();
    let code = json(&admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    let id = json(&phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body(&code, "declined"))).0)["reqId"].as_str().unwrap().to_string();
    admin(&mut c, "POST", "/admin/pair/decide", Some(serde_json::json!({ "reqId": id, "approve": false })));
    let r = json(&phone(&mut c, "GET", &format!("/api/pair/poll?reqId={id}"), None, None).0);
    assert_eq!(r["status"], "denied");
    assert!(c.store.devices.is_empty());
    assert_eq!(phone(&mut c, "GET", "/api/status", Some("declined"), None).0.status, 401);
}

#[test]
fn a_pairing_request_needs_a_well_formed_secret_hash_and_it_cannot_be_reused() {
    let mut c = companion();
    let code = json(&admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    for bad in [serde_json::json!({ "code": code }), serde_json::json!({ "code": code, "secretHash": "abc" }),
                serde_json::json!({ "code": code, "secretHash": "Z".repeat(64) }), serde_json::json!({ "code": code, "secretHash": hash_token("x").to_uppercase() })] {
        assert_eq!(phone(&mut c, "POST", "/api/pair/request", None, Some(bad)).0.status, 400);
    }
    // Malformed requests don't use up the code's tries.
    assert_eq!(phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body(&code, "one"))).0.status, 200);
    let code = json(&admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    assert_eq!(phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body(&code, "one"))).0.status, 409);
}

#[test]
fn approval_can_be_read_again_by_a_phone_that_slept_through_it() {
    let mut c = companion();
    let code = json(&admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    let id = json(&phone(&mut c, "POST", "/api/pair/request", None, Some(pair_body(&code, "sleepy"))).0)["reqId"].as_str().unwrap().to_string();
    admin(&mut c, "POST", "/admin/pair/decide", Some(serde_json::json!({ "reqId": id, "approve": true })));
    for _ in 0..3 {
        assert_eq!(json(&phone(&mut c, "GET", &format!("/api/pair/poll?reqId={id}"), None, None).0)["status"], "approved");
    }
    // Even after the PC forgets the request, the phone's own secret proves it was approved.
    let later = c.handle_phone(&req("GET", &format!("/api/pair/poll?reqId={id}"), PUBLIC, &[(LOGIN_HEADER, LOGIN)], None), 11 * 60_000).0;
    assert_eq!(later.status, 404);
    assert_eq!(phone(&mut c, "GET", "/api/status", Some("sleepy"), None).0.status, 200);
}

#[test]
fn pairing_codes_expire_are_single_use_and_forgive_typing() {
    let mut p = Pairing::default();
    let code = p.start(0).code;
    assert_eq!(normalize_code(&format!(" {}-{} ", &code[..4].to_lowercase(), &code[4..])), code);
    assert_eq!(p.redeem(&code.to_lowercase(), 1), Redeem::Ok);
    assert_eq!(p.redeem(&code, 2), Redeem::NoCode);
    let code = p.start(0).code;
    assert_eq!(p.redeem(&code, CODE_TTL_MS), Redeem::NoCode);
    let code = p.start(0).code;
    assert_eq!(p.redeem(&format!("{code}{}", "X".repeat(100)), 1), Redeem::Wrong);
}

// ---------------------------------------------------------------- device API

#[test]
fn device_calls_need_the_token_and_the_same_tailnet_login() {
    let mut c = companion();
    let token = paired(&mut c);
    assert_eq!(phone(&mut c, "GET", "/api/status", None, None).0.status, 401);
    assert_eq!(phone(&mut c, "GET", "/api/status", Some("wrong"), None).0.status, 401);
    let other = c.handle_phone(&req("GET", "/api/status", PUBLIC, &[(LOGIN_HEADER, "someone@else.test"), ("authorization", &format!("Bearer {token}"))], None), 0).0;
    assert_eq!(other.status, 403);
    assert_eq!(phone(&mut c, "GET", "/api/status", Some(&token), None).0.status, 200);
}

#[test]
fn subscriptions_must_come_from_apple_with_valid_keys() {
    let mut c = companion();
    let token = paired(&mut c);
    let mut bad = apple_subscription();
    bad["subscription"]["endpoint"] = "https://fcm.googleapis.com/fcm/send/x".into();
    assert_eq!(phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(bad)).0.status, 400);
    let mut bad = apple_subscription();
    bad["subscription"]["keys"]["auth"] = b64u(&[1u8; 8]).into();
    assert_eq!(phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(bad)).0.status, 400);
    let (r, fx) = phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(apple_subscription()));
    assert_eq!(r.status, 200);
    assert_eq!(fx, vec![Effect::Persist]);
    assert!(c.store.devices[0].can_receive());
}

#[test]
fn status_tells_the_phone_which_subscription_the_pc_has() {
    let mut c = companion();
    let token = paired(&mut c);
    assert!(json(&phone(&mut c, "GET", "/api/status", Some(&token), None).0)["endpoint"].is_null());
    let sub = apple_subscription();
    phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(sub.clone()));
    assert_eq!(json(&phone(&mut c, "GET", "/api/status", Some(&token), None).0)["endpoint"], sub["subscription"]["endpoint"]);
}

#[test]
fn a_phone_confirms_each_reminder_once_and_unknown_ones_are_ignored() {
    let mut c = companion();
    let token = paired(&mut c);
    c.note_sent(5, 400);
    let (r, fx) = phone(&mut c, "POST", "/api/receipt", Some(&token), Some(serde_json::json!({ "event": 5 })));
    assert_eq!(r.status, 200);
    let device = c.store.devices[0].id.clone();
    assert_eq!(fx, vec![Effect::Receipt { device, event: 5, after_ms: 600 }, Effect::Persist]);
    assert_eq!(c.store.devices[0].received, 1);
    let (r, fx) = phone(&mut c, "POST", "/api/receipt", Some(&token), Some(serde_json::json!({ "event": 5 })));
    assert_eq!((r.status, fx.len()), (200, 0), "a second receipt for the same reminder doesn't count");
    let (_, fx) = phone(&mut c, "POST", "/api/receipt", Some(&token), Some(serde_json::json!({ "event": 99 })));
    assert!(fx.is_empty());
    assert_eq!(phone(&mut c, "POST", "/api/receipt", None, Some(serde_json::json!({ "event": 5 }))).0.status, 401);
    assert_eq!(json(&phone(&mut c, "GET", "/api/status", Some(&token), None).0)["received"], 1);
}

#[test]
fn a_damaged_store_is_an_error_not_an_empty_store() {
    assert!(Store::parse(b"{ not json").is_err());
    assert!(Store::parse(b"").is_err());
    let mut c = companion();
    paired(&mut c);
    let back = Store::parse(&c.store.to_json()).unwrap();
    assert_eq!(back.devices.len(), 1);
}

#[test]
fn subscribing_again_recovers_a_subscription_apple_dropped() {
    let mut c = companion();
    let token = paired(&mut c);
    phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(apple_subscription()));
    c.store.devices[0].needs_resubscribe = true;
    let s = json(&phone(&mut c, "GET", "/api/status", Some(&token), None).0);
    assert_eq!(s["needsResubscribe"], true);
    phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(apple_subscription()));
    let s = json(&phone(&mut c, "GET", "/api/status", Some(&token), None).0);
    assert_eq!(s["needsResubscribe"], false);
    assert!(c.store.devices[0].can_receive());
}

#[test]
fn test_pushes_need_notifications_on_and_are_rate_limited() {
    let mut c = companion();
    let token = paired(&mut c);
    assert_eq!(phone(&mut c, "POST", "/api/test", Some(&token), Some(serde_json::json!({}))).0.status, 409);
    phone(&mut c, "POST", "/api/subscribe", Some(&token), Some(apple_subscription()));
    let (r, fx) = phone(&mut c, "POST", "/api/test", Some(&token), Some(serde_json::json!({})));
    assert_eq!(r.status, 200);
    assert!(matches!(&fx[..], [Effect::SendTest { .. }]));
    assert_eq!(phone(&mut c, "POST", "/api/test", Some(&token), Some(serde_json::json!({}))).0.status, 429);
    c.app.enabled = false;
    let later = c.handle_phone(&req("POST", "/api/test", PUBLIC, &[(LOGIN_HEADER, LOGIN), ("authorization", &format!("Bearer {token}"))], Some(serde_json::json!({}))), 60_000).0;
    assert_eq!(later.status, 409, "nothing is sent while the PC switch is off");
}

#[test]
fn removing_a_phone_locks_it_out_at_once_and_drops_its_queue() {
    let mut c = companion();
    let token = paired(&mut c);
    let id = c.store.devices[0].id.clone();
    let (r, fx) = admin(&mut c, "POST", "/admin/revoke", Some(serde_json::json!({ "deviceId": id })));
    assert_eq!(r.status, 200);
    assert_eq!(fx, vec![Effect::DropJobs { device: id }, Effect::Persist]);
    assert_eq!(phone(&mut c, "GET", "/api/status", Some(&token), None).0.status, 401);
}

#[test]
fn the_pc_page_changes_phone_settings_through_the_ui_thread() {
    let mut c = companion();
    let (_, fx) = admin(&mut c, "POST", "/admin/settings", Some(serde_json::json!({ "enabled": false, "alerts": "idle_only" })));
    assert_eq!(fx, vec![Effect::SetPhone { enabled: false, alerts: PhoneAlerts::IdleOnly }]);
}

#[test]
fn json_endpoints_refuse_other_content_types() {
    let mut c = companion();
    let mut r = req("POST", "/api/pair/request", PUBLIC, &[(LOGIN_HEADER, LOGIN), ("content-type", "text/plain")], None);
    r.body = br#"{"code":"x"}"#.to_vec();
    assert_eq!(c.handle_phone(&r, 0).0.status, 415);
}

#[test]
fn names_are_cleaned() {
    let mut c = companion();
    let code = json(&admin(&mut c, "POST", "/admin/pair/new", Some(serde_json::json!({}))).0)["code"].as_str().unwrap().to_string();
    let id = json(&phone(&mut c, "POST", "/api/pair/request", None, Some(serde_json::json!({ "code": code, "name": "<script>x</script>", "secretHash": hash_token("named") }))).0)["reqId"].as_str().unwrap().to_string();
    admin(&mut c, "POST", "/admin/pair/decide", Some(serde_json::json!({ "reqId": id, "approve": true })));
    assert_eq!(c.store.devices[0].name, "scriptxscript");
}

// ---------------------------------------------------------------- HTTP parsing

#[test]
fn parses_a_normal_request() {
    let raw = b"POST /api/test?a=1&reqId=x%2By HTTP/1.1\r\nHost: 127.0.0.1:8790\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
    let r = http::parse(raw).unwrap();
    assert_eq!(r.method, "POST");
    assert_eq!(r.path, "/api/test");
    assert_eq!(r.query_param("reqId").as_deref(), Some("x+y"));
    assert_eq!(r.header("content-type"), Some("application/json"));
    assert_eq!(r.body, b"{}");
}

#[test]
fn waits_for_the_whole_body() {
    let raw = b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\n\r\nab";
    assert_eq!(http::parse(raw), Err(ParseError::Incomplete));
    assert_eq!(http::parse(b"GET / HTTP/1.1\r\nHost: h\r\n"), Err(ParseError::Incomplete));
}

#[test]
fn refuses_smuggling_and_oversized_requests() {
    let cases: &[(&[u8], ParseError)] = &[
        (b"POST / HTTP/1.1\r\nHost: h\r\nTransfer-Encoding: chunked\r\n\r\n", ParseError::Unsupported),
        (b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\nab", ParseError::Bad),
        (b"GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n", ParseError::Bad),
        (b"GET / HTTP/1.1\r\nHost: a\r\nTailscale-User-Login: a\r\nTailscale-User-Login: b\r\n\r\n", ParseError::Bad),
        (b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: -1\r\n\r\n", ParseError::Bad),
        (b"POST / HTTP/1.1\r\nHost: h\r\nContent-Length: 99999999\r\n\r\n", ParseError::TooLarge),
        (b"GET /../x HTTP/1.1\r\nHost: h\r\n\r\n", ParseError::Bad),
        (b"GET //x HTTP/1.1\r\nHost: h\r\n\r\n", ParseError::Bad),
        (b"GET http://evil/ HTTP/1.1\r\nHost: h\r\n\r\n", ParseError::Bad),
        (b"GET / HTTP/2\r\nHost: h\r\n\r\n", ParseError::Bad),
        (b"GET / HTTP/1.1\r\n\r\n", ParseError::Bad),
        (b"get / HTTP/1.1\r\nHost: h\r\n\r\n", ParseError::Bad),
        (b"GET / HTTP/1.1\r\nBad Header: x\r\nHost: h\r\n\r\n", ParseError::Bad),
    ];
    for (raw, want) in cases {
        assert_eq!(http::parse(raw), Err(*want), "{}", String::from_utf8_lossy(raw));
    }
    let mut huge = b"GET / HTTP/1.1\r\nHost: h\r\nX: ".to_vec();
    huge.extend(std::iter::repeat(b'a').take(http::MAX_HEAD + 10));
    assert_eq!(http::parse(&huge), Err(ParseError::TooLarge));
}

#[test]
fn random_bytes_never_panic_the_parser() {
    let mut seed = 0x2545F4914F6CDD1Du64;
    for _ in 0..20_000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let len = (seed % 300) as usize;
        let mut buf: Vec<u8> = (0..len).map(|i| (seed.rotate_left(i as u32 % 64) & 0xff) as u8).collect();
        if seed % 3 == 0 {
            buf.splice(0..0, b"GET / HTTP/1.1\r\nHost: h\r\n".iter().copied());
            buf.extend_from_slice(b"\r\n\r\n");
        }
        let _ = http::parse(&buf);
    }
}

#[test]
fn every_response_carries_the_security_headers() {
    let text = String::from_utf8(Response::error(404, "x").to_bytes()).unwrap();
    for h in ["X-Content-Type-Options: nosniff", "X-Frame-Options: DENY", "frame-ancestors 'none'", "Cache-Control: no-store", "Connection: close"] {
        assert!(text.contains(h), "{h}");
    }
}
