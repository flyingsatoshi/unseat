//! iPhone reminders on Windows: the loopback listeners, the push sender thread and key
//! storage. Rules live in `unseat::companion`; this module only moves bytes.
//!
//! Nothing here runs until the user turns iPhone reminders on or opens the iPhone page from
//! the tray: no socket, no thread, no file, no network traffic.
//!
//! Threads: the phone listener (only while reminders are on), the admin listener (once the
//! page has been opened) and one sender. They share the companion state and the queue behind
//! mutexes and never touch `App`; changes to settings go to the UI thread as a window message.

use crate::win::{testhook, winhttp};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unseat::companion::api::{Companion, Effect};
use unseat::companion::outbox::{Note, Outbox};
use unseat::companion::reminder::{self, Presence, ReminderKind};
use unseat::companion::store::Store;
use unseat::companion::webpush::{self, b64u, b64u_decode, Outcome, VapidKey};
use unseat::companion::{http, ADMIN_PORT, PHONE_PORT, VAPID_SUBJECT};
use unseat::{Beep, Command, PhoneAlerts, Settings, VisibleState};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HLOCAL, HWND, LPARAM, LocalFree, WPARAM};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, PostMessageW, MB_ICONWARNING, MB_OK, SW_SHOWNORMAL, WM_APP};

/// Posted to the hidden window when the PC page changes the phone settings.
/// wParam bit 0: enabled; bit 1: idle-only.
pub const MSG_PHONE_SETTINGS: u32 = WM_APP + 0x60;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
const PUSH_TIMEOUT_MS: i32 = 10_000;
/// How often to retry the phone listener after its port was busy.
const BIND_RETRY: Duration = Duration::from_secs(10);
/// How often to ask Tailscale for this PC's name: often until it answers (it may start after
/// Unseat at login), then now and then in case the machine was renamed.
const TAILSCALE_RETRY_MS: u64 = 30_000;
const TAILSCALE_RECHECK_MS: u64 = 10 * 60_000;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct Shared {
    companion: Mutex<Companion>,
    outbox: Mutex<Outbox>,
    wake: Condvar,
    vapid: VapidKey,
    store_path: PathBuf,
    hidden: isize,
    stop: AtomicBool,
    /// Wall-clock ms of the last Tailscale status read (0 = never).
    tailscale_checked_ms: AtomicU64,
}

struct Listener {
    stop: Arc<AtomicBool>,
    port: u16,
    thread: JoinHandle<()>,
}

impl Listener {
    fn close(self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so the thread sees the flag.
        let _ = TcpStream::connect_timeout(&([127, 0, 0, 1], self.port).into(), Duration::from_millis(500));
        let _ = self.thread.join();
    }
}

pub struct Phone {
    hidden: isize,
    data_dir: PathBuf,
    shared: Option<Arc<Shared>>,
    phone: Option<Listener>,
    admin: Option<Listener>,
    /// Reminders are on, so the phone listener should be running.
    want_phone: bool,
    last_bind_try: Option<Instant>,
    was_overdue: bool,
}

pub fn wall_ms() -> u64 {
    (SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default() + testhook::clock_offset()).as_millis() as u64
}

fn ports() -> (u16, u16) {
    // Test runs sit beside the user's own Unseat, so they use their own ports.
    if testhook::enabled() {
        (PHONE_PORT + 10_000, ADMIN_PORT + 10_000)
    } else {
        (PHONE_PORT, ADMIN_PORT)
    }
}

impl Phone {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            hidden: 0,
            data_dir: data_dir.to_path_buf(),
            shared: None,
            phone: None,
            admin: None,
            want_phone: false,
            last_bind_try: None,
            was_overdue: false,
        }
    }

    pub fn set_hidden(&mut self, hidden: HWND) {
        self.hidden = hidden.0 as isize;
    }

    /// Load the device store and key, start the sender. Only on first use.
    fn ensure(&mut self) -> Option<Arc<Shared>> {
        if let Some(s) = &self.shared {
            return Some(s.clone());
        }
        let store_path = self.data_dir.join("companion.json");
        let mut store = match std::fs::read(&store_path) {
            Ok(bytes) => match Store::parse(&bytes) {
                Ok(s) => s,
                Err(e) => {
                    // Damaged beyond use: keep it aside rather than overwrite it, and start over.
                    let aside = store_path.with_extension(format!("json.bad-{}", wall_ms()));
                    let kept = std::fs::rename(&store_path, &aside).is_ok();
                    testhook::log(&format!("phone store unreadable ({e}); kept aside={kept}"));
                    if !kept {
                        return None;
                    }
                    Store::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store::default(),
            Err(e) => {
                // Locked or unreadable for now (antivirus, a sync tool): try again later
                // instead of starting empty and overwriting every pairing.
                testhook::log(&format!("phone store busy: {e}"));
                return None;
            }
        };
        let vapid = match store.vapid_protected.as_deref().and_then(b64u_decode).and_then(|b| unprotect(&b)).and_then(|k| VapidKey::from_bytes(&k)) {
            Some(k) => k,
            None => {
                let k = VapidKey::generate();
                let Some(sealed) = protect(&k.to_bytes()) else {
                    testhook::log("phone key protection failed");
                    return None;
                };
                // A new key invalidates old subscriptions; phones subscribe again when opened.
                for d in &mut store.devices {
                    if d.subscription.take().is_some() {
                        d.needs_resubscribe = true;
                    }
                }
                store.vapid_protected = Some(b64u(&sealed));
                let _ = write_atomic(&store_path, &store.to_json());
                k
            }
        };
        let (phone_port, admin_port) = ports();
        let mut companion = Companion::new(store, vapid.public_key(), phone_port, admin_port);
        if testhook::enabled() {
            companion.test_origins = std::env::var("UNSEAT_TEST_PUSH_ORIGIN").ok().into_iter().collect();
            companion.public_host = std::env::var("UNSEAT_TEST_PUBLIC_HOST").ok();
        }
        let shared = Arc::new(Shared {
            companion: Mutex::new(companion),
            outbox: Mutex::new(Outbox::new()),
            wake: Condvar::new(),
            vapid,
            store_path,
            hidden: self.hidden,
            stop: AtomicBool::new(false),
            tailscale_checked_ms: AtomicU64::new(0),
        });
        let s = shared.clone();
        let _ = std::thread::Builder::new().name("unseat-push".into()).spawn(move || sender(s));
        self.shared = Some(shared.clone());
        refresh_tailscale(&shared, 0);
        Some(shared)
    }

    /// Follow the settings: the phone listener runs only while reminders are on.
    pub fn sync(&mut self, settings: &Settings) {
        self.want_phone = settings.phone_enabled;
        if settings.phone_enabled {
            self.last_bind_try = Some(Instant::now());
            let Some(shared) = self.ensure() else { return };
            {
                let mut c = shared.companion.lock().unwrap();
                c.app.enabled = true;
                c.app.alerts = settings.phone_alerts;
            }
            self.start_phone_listener(&shared);
        } else {
            if let Some(shared) = &self.shared {
                shared.companion.lock().unwrap().app.listener_error = None;
            }
            if let Some(l) = self.phone.take() {
                l.close();
                testhook::log("phone listener off");
            }
            if let Some(shared) = &self.shared {
                shared.companion.lock().unwrap().app.enabled = false;
                let mut ob = shared.outbox.lock().unwrap();
                for topic in [ReminderKind::TimeUp.topic(), ReminderKind::BreakOver.topic(), ReminderKind::Test.topic()] {
                    ob.cancel_topic(topic);
                }
            }
        }
    }

    fn start_phone_listener(&mut self, shared: &Arc<Shared>) {
        if self.phone.is_some() {
            return;
        }
        let port = ports().0;
        self.phone = listen(shared, port, false);
        testhook::log(&format!("phone listener {}", if self.phone.is_some() { "on" } else { "failed" }));
        shared.companion.lock().unwrap().app.listener_error = self.phone.is_none().then(|| {
            format!("Port {port} is in use by another program, so the phone can't connect. Unseat keeps trying.")
        });
    }

    /// Open the PC page in the browser (tray menu → iPhone).
    pub fn open_admin(&mut self) {
        match self.start_admin() {
            Some(url) => unsafe {
                let _ = ShellExecuteW(HWND::default(), &HSTRING::from("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
            },
            None => unsafe {
                let text = format!(
                    "Unseat couldn't open the iPhone page. Port {} may be in use by another program; close it and try again.",
                    ports().1
                );
                let _ = MessageBoxW(HWND(self.hidden as *mut _), &HSTRING::from(text), &HSTRING::from("Unseat"), MB_OK | MB_ICONWARNING);
            },
        }
    }

    /// Start the PC page's listener and return its URL (with the one-time key in the fragment).
    pub fn start_admin(&mut self) -> Option<String> {
        let shared = self.ensure()?;
        if self.admin.is_none() {
            self.admin = listen(&shared, ports().1, true);
        }
        self.admin.as_ref()?;
        refresh_tailscale(&shared, 0);
        let token = shared.companion.lock().unwrap().admin_token.clone();
        let url = format!("http://127.0.0.1:{}/#t={token}", ports().1);
        testhook::log(&format!("phone admin {url}"));
        Some(url)
    }

    /// Every timer tick: what the phone shows, and upkeep (listener retry, Tailscale name,
    /// dropping sitting reminders that a break or a reset made obsolete).
    pub fn tick(&mut self, timer_line: &str, today_line: &str, state: VisibleState, sitting: Duration) {
        // Overdue until a new session starts: back to sitting, a break, or (with "Pause when
        // idle") an away spell long enough to clear the session. A user pause keeps the flag;
        // the pause command itself cancels.
        let session_ended = self.was_overdue
            && match state {
                VisibleState::Sitting | VisibleState::Break => true,
                VisibleState::Away => sitting.is_zero(),
                VisibleState::Overdue | VisibleState::Paused => false,
            };
        if state == VisibleState::Overdue {
            self.was_overdue = true;
        } else if session_ended {
            self.was_overdue = false;
        }
        if self.want_phone && self.phone.is_none() && self.last_bind_try.is_none_or(|t| t.elapsed() >= BIND_RETRY) {
            self.last_bind_try = Some(Instant::now());
            if let Some(shared) = self.ensure() {
                self.start_phone_listener(&shared);
            }
        }
        let Some(shared) = self.shared.clone() else { return };
        if session_ended {
            // A natural break (idle long enough) reset the session without a PC command.
            let notes = shared.outbox.lock().unwrap().cancel_topic(ReminderKind::TimeUp.topic());
            record(&shared, &notes);
        }
        let pending = shared.outbox.lock().unwrap().pending();
        let known = {
            let mut c = shared.companion.lock().unwrap();
            if c.app.timer_line != timer_line {
                c.app.timer_line = timer_line.to_string();
            }
            if c.app.today_line != today_line {
                c.app.today_line = today_line.to_string();
            }
            c.app.pending = pending;
            c.public_host.is_some()
        };
        if self.phone.is_some() {
            refresh_tailscale(&shared, if known { TAILSCALE_RECHECK_MS } else { TAILSCALE_RETRY_MS });
        }
    }

    /// An engine reminder fired on the PC: mirror it to the phones.
    pub fn on_beep(&mut self, beep: Beep, settings: &Settings, sitting: Duration, presence: Presence) {
        if beep == Beep::None || !settings.phone_enabled {
            return;
        }
        if !reminder::phone_should_alert(settings, presence) {
            testhook::log(&format!("phone skip {beep:?} (idle-only, user at PC)"));
            return;
        }
        let (Some(shared), Some(r)) = (self.ensure(), reminder::reminder_for(beep, settings, sitting)) else { return };
        enqueue(&shared, &r, None);
    }

    /// The user acted on the PC: queued sitting reminders are obsolete.
    pub fn on_command(&self, command: Command) {
        let Some(shared) = &self.shared else { return };
        if matches!(command, Command::Reset | Command::StartBreak | Command::Snooze(_) | Command::TogglePause) {
            let notes = shared.outbox.lock().unwrap().cancel_topic(ReminderKind::TimeUp.topic());
            record(shared, &notes);
        }
    }

    pub fn shutdown(&mut self) {
        for l in [self.phone.take(), self.admin.take()].into_iter().flatten() {
            l.close();
        }
        if let Some(s) = self.shared.take() {
            // Under the queue lock, so the sender can't miss the wake-up between its check
            // and its wait.
            let _ob = s.outbox.lock().unwrap();
            s.stop.store(true, Ordering::SeqCst);
            s.wake.notify_all();
        }
    }
}

/// Queue a reminder for every receiving device (or one device for a test).
fn enqueue(shared: &Arc<Shared>, r: &reminder::Reminder, only: Option<&str>) {
    let (devices, navigate) = {
        let c = shared.companion.lock().unwrap();
        let mut d = c.store.receivers();
        if let Some(id) = only {
            d.retain(|x| x == id);
        }
        (d, c.phone_url().unwrap_or_else(|| "/".to_string()))
    };
    if devices.is_empty() {
        testhook::log(&format!("phone no receivers for {}", r.kind.key()));
        return;
    }
    let now = wall_ms();
    let (event, notes) = {
        let mut ob = shared.outbox.lock().unwrap();
        let payload = reminder::message(r, &navigate, ob.next_event());
        ob.enqueue(r.kind, payload, r.ttl_secs, &devices, now)
    };
    shared.companion.lock().unwrap().note_sent(event, now);
    testhook::log(&format!("phone queued {} event={event} devices={}", r.kind.key(), devices.len()));
    record(shared, &notes);
    shared.wake.notify_all();
}

// ---------------------------------------------------------------- sender

fn sender(shared: Arc<Shared>) {
    let mut vapid_cache: HashMap<String, (u64, String)> = HashMap::new();
    loop {
        let job = {
            let mut ob = shared.outbox.lock().unwrap();
            loop {
                if shared.stop.load(Ordering::SeqCst) {
                    return;
                }
                let now = wall_ms();
                let (job, notes) = ob.take_due(now);
                if !notes.is_empty() {
                    drop(ob);
                    record(&shared, &notes);
                    ob = shared.outbox.lock().unwrap();
                }
                if let Some(job) = job {
                    break job;
                }
                let wait = ob.next_wake_ms().map_or(3_600_000, |t| t.saturating_sub(now)).clamp(20, 3_600_000);
                ob = shared.wake.wait_timeout(ob, Duration::from_millis(wait)).unwrap().0;
            }
        };
        let sub = shared.companion.lock().unwrap().store.device(&job.device).and_then(|d| d.subscription.clone());
        let now = wall_ms();
        let (mut outcome, status, mut detail) = match sub.as_ref() {
            None => (Outcome::Rejected, None, "no subscription".to_string()),
            Some(sub) => {
                let mut sent = send_one(&shared, &mut vapid_cache, sub, &job, now, false);
                // A cached VAPID token the push service refuses (clock change, key rotation):
                // drop it and try once more with a fresh one instead of failing for an hour.
                if matches!(sent, Ok((400 | 401 | 403, true))) {
                    testhook::log("phone push token refused; signing a new one");
                    sent = send_one(&shared, &mut vapid_cache, sub, &job, wall_ms(), true);
                }
                match sent {
                    Ok((code, _)) => (webpush::classify(Some(code)), Some(code), String::new()),
                    Err(e) => (Outcome::Retry, None, e),
                }
            }
        };
        if outcome == Outcome::Gone {
            // The phone may have subscribed again while this send was under way: then "gone"
            // is about the old endpoint, and the reminder should go to the new one.
            let current = shared.companion.lock().unwrap().store.device(&job.device).and_then(|d| d.subscription.as_ref().map(|s| s.endpoint.clone()));
            if current.is_some() && current != sub.as_ref().map(|s| s.endpoint.clone()) {
                outcome = Outcome::Retry;
                detail = "old subscription gone; resending to the new one".into();
            }
        }
        testhook::log(&format!(
            "phone push {} event={} attempt={} status={} outcome={outcome:?} {detail}",
            job.kind.key(),
            job.event,
            job.attempts,
            status.map_or("-".to_string(), |s| s.to_string())
        ));
        let notes = shared.outbox.lock().unwrap().finish(job.id, outcome, status, wall_ms());
        record(&shared, &notes);
    }
}

/// Send one push. Returns the HTTP status and whether a cached VAPID token was used.
fn send_one(
    shared: &Shared,
    cache: &mut HashMap<String, (u64, String)>,
    sub: &unseat::companion::store::Subscription,
    job: &unseat::companion::outbox::Job,
    now: u64,
    fresh_token: bool,
) -> Result<(u16, bool), String> {
    let body = webpush::encrypt(&job.payload, &sub.p256dh, &sub.auth, None).map_err(|e| e.to_string())?;
    let origin = webpush::origin(&sub.endpoint).ok_or("bad endpoint")?;
    let (auth, cached) = match cache.get(&origin) {
        Some((at, h)) if !fresh_token && now.saturating_sub(*at) < webpush::JWT_REUSE_SECS * 1000 => (h.clone(), true),
        _ => {
            let h = shared.vapid.authorization(&sub.endpoint, VAPID_SUBJECT, now / 1000).map_err(|e| e.to_string())?;
            cache.insert(origin, (now, h.clone()));
            (h, false)
        }
    };
    let headers = [
        ("TTL", job.remaining_ttl_secs(now).max(1).to_string()),
        ("Urgency", "high".to_string()),
        ("Topic", job.kind.topic().to_string()),
        ("Content-Encoding", "aes128gcm".to_string()),
        ("Content-Type", "application/octet-stream".to_string()),
        ("Authorization", auth),
    ];
    winhttp::post(&sub.endpoint, &headers, &body, PUSH_TIMEOUT_MS).map(|code| (code, cached))
}

/// Fold queue notes into the device records (counts, recovery flag) and save.
fn record(shared: &Shared, notes: &[Note]) {
    if notes.is_empty() {
        return;
    }
    let mut c = shared.companion.lock().unwrap();
    let mut changed = false;
    for n in notes {
        testhook::log(&format!("phone note {n:?}").replace(['\n', '\r'], " "));
        match n {
            Note::Delivered { device, .. } => {
                if let Some(d) = c.store.device_mut(device) {
                    d.delivered += 1;
                    d.last_delivered_ms = wall_ms();
                    d.last_error = None;
                    changed = true;
                }
            }
            Note::Missed { device, .. } => {
                if let Some(d) = c.store.device_mut(device) {
                    d.missed += 1;
                    changed = true;
                }
            }
            Note::Gone { device } => {
                if let Some(d) = c.store.device_mut(device) {
                    d.needs_resubscribe = true;
                    d.last_error = Some("Apple said the subscription is gone; open the app to reconnect".into());
                    changed = true;
                }
            }
            Note::Rejected { device, status, .. } => {
                if let Some(d) = c.store.device_mut(device) {
                    d.last_error = Some(format!("push refused ({})", status.map_or("no status".into(), |s| s.to_string())));
                    changed = true;
                }
            }
            Note::Retrying { .. } | Note::Replaced { .. } | Note::Cancelled { .. } => {}
        }
    }
    if changed {
        let _ = write_atomic(&shared.store_path, &c.store.to_json());
    }
}

// ---------------------------------------------------------------- listeners

fn listen(shared: &Arc<Shared>, port: u16, admin: bool) -> Option<Listener> {
    // Loopback only. Tailscale Serve proxies to this address; nothing else can reach it.
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            testhook::log(&format!("phone bind {port} failed: {e}"));
            return None;
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (s, flag) = (shared.clone(), stop.clone());
    let thread = std::thread::Builder::new()
        .name(if admin { "unseat-phone-admin" } else { "unseat-phone" }.into())
        .spawn(move || {
            for conn in listener.incoming() {
                if flag.load(Ordering::SeqCst) || s.stop.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = conn {
                    serve(&s, stream, admin);
                }
            }
        })
        .ok()?;
    Some(Listener { stop, port, thread })
}

fn serve(shared: &Arc<Shared>, mut stream: TcpStream, admin: bool) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let request = loop {
        match http::parse(&buf) {
            Ok(r) => break r,
            Err(http::ParseError::Incomplete) if Instant::now() < deadline && buf.len() <= http::MAX_HEAD + http::MAX_BODY => match stream.read(&mut chunk) {
                Ok(0) | Err(_) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            },
            Err(e) => {
                let _ = stream.write_all(&http::Response::error(e.status(), "bad request").to_bytes());
                let _ = stream.shutdown(Shutdown::Both);
                return;
            }
        }
    };
    let (response, effects) = {
        let mut c = shared.companion.lock().unwrap();
        if admin {
            c.handle_admin(&request, wall_ms())
        } else {
            c.handle_phone(&request, wall_ms())
        }
    };
    let _ = stream.write_all(&response.to_bytes());
    let _ = stream.shutdown(Shutdown::Both);
    if !admin && response.status == 421 {
        // Probably this PC's tailnet name isn't known yet (or changed): look it up again.
        refresh_tailscale(shared, 10_000);
    }
    for effect in effects {
        match effect {
            Effect::Persist => {
                let c = shared.companion.lock().unwrap();
                let _ = write_atomic(&shared.store_path, &c.store.to_json());
            }
            Effect::SetPhone { enabled, alerts } => unsafe {
                let wp = usize::from(enabled) | (usize::from(alerts == PhoneAlerts::IdleOnly) << 1);
                let _ = PostMessageW(HWND(shared.hidden as *mut _), MSG_PHONE_SETTINGS, WPARAM(wp), LPARAM(0));
            },
            Effect::SendTest { device } => enqueue(shared, &reminder::test_reminder(), Some(&device)),
            Effect::DropJobs { device } => shared.outbox.lock().unwrap().drop_device(&device),
            Effect::Receipt { device, event, after_ms } => {
                testhook::log(&format!("phone receipt event={event} after_ms={after_ms} device={device}"));
            }
        }
    }
}

// ---------------------------------------------------------------- Tailscale (read-only)

fn tailscale(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("tailscale").args(args).creation_flags(CREATE_NO_WINDOW).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(if text.is_empty() { String::from_utf8_lossy(&out.stderr).trim().to_string() } else { text })
}

/// Read this PC's tailnet name and the Serve/Funnel status, off the UI thread, unless it was
/// read less than `min_gap_ms` ago. Read-only: Unseat never changes Tailscale's configuration.
fn refresh_tailscale(shared: &Arc<Shared>, min_gap_ms: u64) {
    if testhook::enabled() {
        return;
    }
    let now = wall_ms();
    let last = shared.tailscale_checked_ms.load(Ordering::SeqCst);
    if last != 0 && now.saturating_sub(last) < min_gap_ms {
        return;
    }
    shared.tailscale_checked_ms.store(now, Ordering::SeqCst);
    let shared = shared.clone();
    let _ = std::thread::Builder::new().name("unseat-tailscale".into()).spawn(move || {
        let host = tailscale(&["status", "--json"])
            .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
            .and_then(|v| v["Self"]["DNSName"].as_str().map(|s| s.trim_end_matches('.').to_string()))
            .filter(|h| h.ends_with(".ts.net"));
        let serve = tailscale(&["serve", "status"]).unwrap_or_else(|| "Tailscale not found".into());
        let funnel = tailscale(&["funnel", "status"]).unwrap_or_else(|| "Tailscale not found".into());
        let mut c = shared.companion.lock().unwrap();
        if host.is_some() {
            c.public_host = host;
        }
        c.app.serve_status = serve;
        c.app.funnel_status = funnel;
    });
}

// ---------------------------------------------------------------- storage

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Encrypt for the current Windows user (DPAPI).
fn protect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        CryptProtectData(&input, &HSTRING::from("Unseat iPhone key"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData.cast()));
        Some(v)
    }
}

fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out).ok()?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(out.pbData.cast()));
        Some(v)
    }
}
