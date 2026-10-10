//! The companion's HTTP API as a pure function of (request, state, time). The host only moves
//! bytes; every rule (who may call what, limits, pairing) lives here and is unit-tested.
//!
//! Two listeners, both bound to 127.0.0.1 by the host:
//! - phone: the iPhone app and its API. Only `tailscale serve` exposes it, to the tailnet.
//!   Every request must carry the `Tailscale-User-Login` header that Serve adds (and strips
//!   from clients). Funnel traffic never carries it, so even a mistaken Funnel can't reach it.
//! - admin: the PC's own page (pairing, approvals, devices). Never served. Every API call needs
//!   the per-run admin token, and the Host must be the loopback address (no DNS rebinding).

use super::http::{Request, Response};
use super::pairing::{constant_time_eq, hash_token, new_token, token_matches, Pairing, Redeem};
use super::store::{Device, Store, Subscription, MAX_DEVICES};
use super::webpush::{allowed_endpoint, b64u, b64u_decode, random_bytes};
use super::{assets, qr_svg};
use crate::settings::PhoneAlerts;
use serde_json::{json, Value};

const PAIR_ATTEMPTS_PER_MINUTE: usize = 10;
const MAX_PENDING_REQUESTS: usize = 5;
const REQUEST_TTL_MS: u64 = 10 * 60_000;
const TEST_EVERY_MS: u64 = 10_000;
pub const ADMIN_HEADER: &str = "x-unseat-admin";
pub const LOGIN_HEADER: &str = "tailscale-user-login";

/// What the host must do after a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Save the device store.
    Persist,
    /// Change the phone settings (applied on the UI thread).
    SetPhone { enabled: bool, alerts: PhoneAlerts },
    /// Send a test reminder to one device.
    SendTest { device: String },
    /// Forget queued reminders for a device (revoked or unsubscribed).
    DropJobs { device: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PairStatus {
    Pending,
    Approved { device: String, token: Option<String> },
    Denied,
}

#[derive(Clone, Debug)]
struct PairRequest {
    id: String,
    name: String,
    login: String,
    created_ms: u64,
    status: PairStatus,
}

/// What the host tells the API about the app, refreshed as it changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppView {
    pub enabled: bool,
    pub alerts: PhoneAlerts,
    pub timer_line: String,
    pub today_line: String,
    pub pending: usize,
    pub serve_status: String,
    pub funnel_status: String,
}

pub struct Companion {
    pub store: Store,
    pub pairing: Pairing,
    pub vapid_public: String,
    /// The PC's `*.ts.net` name, if Tailscale reported one.
    pub public_host: Option<String>,
    pub phone_port: u16,
    pub admin_port: u16,
    pub admin_token: String,
    /// Extra push origins accepted in test runs only.
    pub test_origins: Vec<String>,
    pub app: AppView,
    requests: Vec<PairRequest>,
    pair_hits: Vec<u64>,
    last_test: Vec<(String, u64)>,
}

impl Companion {
    pub fn new(store: Store, vapid_public: String, phone_port: u16, admin_port: u16) -> Self {
        Self {
            store,
            pairing: Pairing::default(),
            vapid_public,
            public_host: None,
            phone_port,
            admin_port,
            admin_token: b64u(&random_bytes::<24>()),
            test_origins: Vec::new(),
            app: AppView::default(),
            requests: Vec::new(),
            pair_hits: Vec::new(),
            last_test: Vec::new(),
        }
    }

    pub fn phone_url(&self) -> Option<String> {
        self.public_host.as_ref().map(|h| format!("https://{h}/"))
    }

    /// The command the user runs once so the tailnet (only) can reach the phone listener.
    pub fn serve_command(&self) -> String {
        format!("tailscale serve --bg {}", self.phone_port)
    }

    // ------------------------------------------------------------------ phone listener

    pub fn handle_phone(&mut self, req: &Request, now_ms: u64) -> (Response, Vec<Effect>) {
        let host = req.header("host").unwrap_or("");
        let local = [format!("127.0.0.1:{}", self.phone_port), format!("localhost:{}", self.phone_port)];
        if !(local.iter().any(|h| h == host) || self.public_host.as_deref() == Some(host)) {
            return (Response::error(421, "wrong host"), vec![]);
        }
        let Some(login) = req.header(LOGIN_HEADER).filter(|l| !l.is_empty()).map(str::to_string) else {
            return (Response::error(403, "only reachable through your tailnet"), vec![]);
        };
        let route = (req.method.as_str(), req.path.as_str());
        if req.method == "GET" {
            if let Some(asset) = assets::phone(&req.path) {
                return (Response::asset(asset.0, asset.1), vec![]);
            }
        }
        match route {
            ("GET", "/api/config") => (Response::json(200, json!({ "vapidPublicKey": self.vapid_public })), vec![]),
            ("POST", "/api/pair/request") => self.pair_request(req, &login, now_ms),
            ("GET", "/api/pair/poll") => self.pair_poll(req, now_ms),
            ("GET", "/api/status") => self.with_device(req, &login, now_ms, |c, id| c.phone_status(id)),
            ("POST", "/api/subscribe") => self.with_device(req, &login, now_ms, |c, id| c.subscribe(req, id)),
            ("POST", "/api/unsubscribe") => self.with_device(req, &login, now_ms, |c, id| c.unsubscribe(id)),
            ("POST", "/api/test") => self.with_device(req, &login, now_ms, |c, id| c.phone_test(id, now_ms)),
            (_, p) if p.starts_with("/admin") => (Response::error(404, "not found"), vec![]),
            _ => (Response::error(404, "not found"), vec![]),
        }
    }

    fn with_device(
        &mut self,
        req: &Request,
        login: &str,
        now_ms: u64,
        f: impl FnOnce(&mut Self, &str) -> (Response, Vec<Effect>),
    ) -> (Response, Vec<Effect>) {
        let token = req.header("authorization").and_then(|a| a.strip_prefix("Bearer ")).unwrap_or("");
        let Some(device) = self.store.devices.iter_mut().find(|d| token_matches(token, &d.token_hash)) else {
            return (Response::error(401, "not paired, or this phone was removed"), vec![]);
        };
        if !constant_time_eq(device.login.as_bytes(), login.as_bytes()) {
            return (Response::error(403, "signed in to the tailnet as someone else"), vec![]);
        }
        device.last_seen_ms = now_ms;
        let id = device.id.clone();
        f(self, &id)
    }

    fn pair_request(&mut self, req: &Request, login: &str, now_ms: u64) -> (Response, Vec<Effect>) {
        let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
        self.pair_hits.retain(|t| now_ms.saturating_sub(*t) < 60_000);
        if self.pair_hits.len() >= PAIR_ATTEMPTS_PER_MINUTE {
            return (Response::error(429, "too many pairing attempts; wait a minute"), vec![]);
        }
        self.pair_hits.push(now_ms);
        self.prune_requests(now_ms);
        if self.requests.iter().filter(|r| r.status == PairStatus::Pending).count() >= MAX_PENDING_REQUESTS {
            return (Response::error(429, "too many phones waiting for approval"), vec![]);
        }
        match self.pairing.redeem(body["code"].as_str().unwrap_or(""), now_ms) {
            Redeem::Ok => {}
            Redeem::NoCode => return (Response::error(403, "no active code; make a new one on the PC"), vec![]),
            Redeem::Wrong => return (Response::error(403, "wrong code"), vec![]),
        }
        let id = b64u(&random_bytes::<16>());
        self.requests.push(PairRequest {
            id: id.clone(),
            name: clean_name(body["name"].as_str().unwrap_or("")),
            login: login.to_string(),
            created_ms: now_ms,
            status: PairStatus::Pending,
        });
        (Response::json(200, json!({ "reqId": id })), vec![])
    }

    fn pair_poll(&mut self, req: &Request, now_ms: u64) -> (Response, Vec<Effect>) {
        self.prune_requests(now_ms);
        let id = req.query_param("reqId").unwrap_or_default();
        let Some(r) = self.requests.iter_mut().find(|r| constant_time_eq(r.id.as_bytes(), id.as_bytes())) else {
            return (Response::error(404, "unknown request"), vec![]);
        };
        let body = match &mut r.status {
            PairStatus::Pending => json!({ "status": "pending" }),
            PairStatus::Denied => json!({ "status": "denied" }),
            PairStatus::Approved { device, token } => match token.take() {
                // Handed out exactly once.
                Some(t) => json!({ "status": "approved", "deviceId": device, "token": t }),
                None => json!({ "status": "approved" }),
            },
        };
        (Response::json(200, body), vec![])
    }

    fn phone_status(&self, id: &str) -> (Response, Vec<Effect>) {
        let d = self.store.device(id).expect("authenticated device");
        (
            Response::json(
                200,
                json!({
                    "name": d.name,
                    "enabled": self.app.enabled,
                    "alerts": self.app.alerts.key(),
                    "subscribed": d.subscription.is_some(),
                    "needsResubscribe": d.needs_resubscribe,
                    "delivered": d.delivered,
                    "missed": d.missed,
                    "lastDeliveredMs": d.last_delivered_ms,
                    "timer": self.app.timer_line,
                    "today": self.app.today_line,
                }),
            ),
            vec![],
        )
    }

    fn subscribe(&mut self, req: &Request, id: &str) -> (Response, Vec<Effect>) {
        let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
        let sub = &body["subscription"];
        let endpoint = sub["endpoint"].as_str().unwrap_or("");
        let p256dh = sub["keys"]["p256dh"].as_str().unwrap_or("");
        let auth = sub["keys"]["auth"].as_str().unwrap_or("");
        let keys_ok = b64u_decode(p256dh).is_some_and(|k| k.len() == 65 && k[0] == 4) && b64u_decode(auth).is_some_and(|k| k.len() == 16);
        if !allowed_endpoint(endpoint, &self.test_origins) || !keys_ok || endpoint.len() > 1024 {
            return (Response::error(400, "the subscription must be from Apple's push service"), vec![]);
        }
        let d = self.store.device_mut(id).expect("authenticated device");
        d.subscription = Some(Subscription { endpoint: endpoint.to_string(), p256dh: p256dh.to_string(), auth: auth.to_string() });
        d.needs_resubscribe = false;
        d.last_error = None;
        (Response::json(200, json!({ "ok": true })), vec![Effect::Persist])
    }

    fn unsubscribe(&mut self, id: &str) -> (Response, Vec<Effect>) {
        let d = self.store.device_mut(id).expect("authenticated device");
        d.subscription = None;
        (Response::json(200, json!({ "ok": true })), vec![Effect::DropJobs { device: id.to_string() }, Effect::Persist])
    }

    fn phone_test(&mut self, id: &str, now_ms: u64) -> (Response, Vec<Effect>) {
        if !self.store.device(id).is_some_and(Device::can_receive) {
            return (Response::error(409, "turn on notifications in the app first"), vec![]);
        }
        if !self.app.enabled {
            return (Response::error(409, "iPhone reminders are off on the PC"), vec![]);
        }
        if let Some((_, t)) = self.last_test.iter().find(|(d, _)| d == id) {
            if now_ms.saturating_sub(*t) < TEST_EVERY_MS {
                return (Response::error(429, "wait a few seconds between tests"), vec![]);
            }
        }
        self.last_test.retain(|(d, _)| d != id);
        self.last_test.push((id.to_string(), now_ms));
        (Response::json(200, json!({ "ok": true })), vec![Effect::SendTest { device: id.to_string() }])
    }

    fn prune_requests(&mut self, now_ms: u64) {
        self.requests.retain(|r| now_ms.saturating_sub(r.created_ms) < REQUEST_TTL_MS);
    }

    // ------------------------------------------------------------------ admin listener

    pub fn handle_admin(&mut self, req: &Request, now_ms: u64) -> (Response, Vec<Effect>) {
        let host = req.header("host").unwrap_or("");
        if host != format!("127.0.0.1:{}", self.admin_port) && host != format!("localhost:{}", self.admin_port) {
            return (Response::error(421, "wrong host"), vec![]);
        }
        // A request that came through Tailscale Serve carries identity headers; the admin
        // listener is never served, so refuse anything that looks like it was.
        if req.header(LOGIN_HEADER).is_some() {
            return (Response::error(403, "PC only"), vec![]);
        }
        if req.method == "GET" {
            if let Some(asset) = assets::admin(&req.path) {
                return (Response::asset(asset.0, asset.1), vec![]);
            }
        }
        let given = if req.path == "/admin/qr.svg" { req.query_param("t").unwrap_or_default() } else { req.header(ADMIN_HEADER).unwrap_or("").to_string() };
        if !constant_time_eq(given.as_bytes(), self.admin_token.as_bytes()) {
            return (Response::error(403, "open this page from Unseat's tray menu"), vec![]);
        }
        match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/admin/state") => (Response::json(200, self.admin_state(now_ms)), vec![]),
            ("GET", "/admin/qr.svg") => match (self.pairing.current(now_ms).cloned(), self.phone_url()) {
                (Some(code), Some(url)) => {
                    let mut r = Response::asset("image/svg+xml", qr_svg(&format!("{url}#pair={}", code.code)).as_bytes());
                    r.cacheable = false;
                    (r, vec![])
                }
                _ => (Response::error(404, "no active code"), vec![]),
            },
            ("POST", "/admin/pair/new") => {
                let code = self.pairing.start(now_ms);
                (Response::json(200, json!({ "code": code.code, "expiresMs": code.expires_ms })), vec![])
            }
            ("POST", "/admin/pair/decide") => self.decide(req, now_ms),
            ("POST", "/admin/revoke") => {
                let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
                let id = body["deviceId"].as_str().unwrap_or("").to_string();
                let before = self.store.devices.len();
                self.store.devices.retain(|d| d.id != id);
                if before == self.store.devices.len() {
                    return (Response::error(404, "no such device"), vec![]);
                }
                (Response::json(200, json!({ "ok": true })), vec![Effect::DropJobs { device: id }, Effect::Persist])
            }
            ("POST", "/admin/settings") => {
                let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
                let enabled = body["enabled"].as_bool().unwrap_or(self.app.enabled);
                let alerts = body["alerts"].as_str().map(PhoneAlerts::from_key).unwrap_or(self.app.alerts);
                (Response::json(200, json!({ "ok": true })), vec![Effect::SetPhone { enabled, alerts }])
            }
            ("POST", "/admin/test") => {
                let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
                let id = body["deviceId"].as_str().unwrap_or("").to_string();
                if !self.store.device(&id).is_some_and(Device::can_receive) {
                    return (Response::error(409, "that phone hasn't turned on notifications"), vec![]);
                }
                if !self.app.enabled {
                    return (Response::error(409, "turn iPhone reminders on first"), vec![]);
                }
                (Response::json(200, json!({ "ok": true })), vec![Effect::SendTest { device: id }])
            }
            _ => (Response::error(404, "not found"), vec![]),
        }
    }

    fn decide(&mut self, req: &Request, now_ms: u64) -> (Response, Vec<Effect>) {
        let Some(body) = json_body(req) else { return (Response::error(415, "send JSON"), vec![]) };
        self.prune_requests(now_ms);
        let id = body["reqId"].as_str().unwrap_or("");
        let devices = self.store.devices.len();
        let Some(r) = self.requests.iter_mut().find(|r| r.id == id && r.status == PairStatus::Pending) else {
            return (Response::error(404, "no such request"), vec![]);
        };
        if body["approve"].as_bool() != Some(true) {
            r.status = PairStatus::Denied;
            return (Response::json(200, json!({ "ok": true })), vec![]);
        }
        if devices >= MAX_DEVICES {
            return (Response::error(409, "too many phones; remove one first"), vec![]);
        }
        let token = new_token();
        let device_id = b64u(&random_bytes::<12>());
        self.store.devices.push(Device {
            id: device_id.clone(),
            name: r.name.clone(),
            login: r.login.clone(),
            token_hash: hash_token(&token),
            created_ms: now_ms,
            last_seen_ms: 0,
            subscription: None,
            needs_resubscribe: false,
            delivered: 0,
            missed: 0,
            last_delivered_ms: 0,
            last_error: None,
        });
        r.status = PairStatus::Approved { device: device_id.clone(), token: Some(token) };
        (Response::json(200, json!({ "ok": true, "deviceId": device_id })), vec![Effect::Persist])
    }

    fn admin_state(&mut self, now_ms: u64) -> Value {
        self.prune_requests(now_ms);
        let code = self.pairing.current(now_ms).cloned();
        json!({
            "enabled": self.app.enabled,
            "alerts": self.app.alerts.key(),
            "phoneUrl": self.phone_url(),
            "serveCommand": self.serve_command(),
            "phonePort": self.phone_port,
            "serveStatus": self.app.serve_status,
            "funnelStatus": self.app.funnel_status,
            "pending": self.app.pending,
            "pairing": code.map(|c| json!({ "code": c.code, "expiresMs": c.expires_ms, "attemptsLeft": super::pairing::CODE_MAX_ATTEMPTS - c.attempts })),
            "requests": self.requests.iter().filter(|r| r.status == PairStatus::Pending)
                .map(|r| json!({ "reqId": r.id, "name": r.name, "login": r.login })).collect::<Vec<_>>(),
            "devices": self.store.devices.iter().map(|d| json!({
                "id": d.id, "name": d.name, "login": d.login, "createdMs": d.created_ms, "lastSeenMs": d.last_seen_ms,
                "subscribed": d.subscription.is_some(), "needsResubscribe": d.needs_resubscribe,
                "delivered": d.delivered, "missed": d.missed, "lastDeliveredMs": d.last_delivered_ms, "lastError": d.last_error,
            })).collect::<Vec<_>>(),
            "nowMs": now_ms,
        })
    }
}

fn json_body(req: &Request) -> Option<Value> {
    if !req.header("content-type").is_some_and(|c| c.starts_with("application/json")) {
        return None;
    }
    serde_json::from_slice::<Value>(&req.body).ok().filter(Value::is_object)
}

fn clean_name(name: &str) -> String {
    let n: String = name.chars().filter(|c| c.is_alphanumeric() || " '._-".contains(*c)).take(40).collect();
    let n = n.trim().to_string();
    if n.is_empty() {
        "iPhone".to_string()
    } else {
        n
    }
}
