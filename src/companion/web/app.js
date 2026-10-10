"use strict";
// Unseat iPhone app: pairs with the PC, keeps the push subscription healthy, shows status.

const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "unseat-token";
// A pairing in progress: { reqId, secret, at }. Kept so pairing survives the app being
// suspended (locked phone, app switch) while the user walks to the PC to approve it.
const PENDING_KEY = "unseat-pending";
const PENDING_FOR_MS = 10 * 60_000;
const standalone = navigator.standalone === true || matchMedia("(display-mode: standalone)").matches;
let config = null; // fetched before any tap, so subscribe() runs straight from the gesture
let swReg = null;
let backoff = 1000;
let retryTimer = null;

const getToken = () => {
  try { return localStorage.getItem(TOKEN_KEY); } catch { return null; }
};
const setToken = (token) => {
  try { token ? localStorage.setItem(TOKEN_KEY, token) : localStorage.removeItem(TOKEN_KEY); } catch {}
  // The service worker can't read localStorage; it needs the token to re-send a subscription
  // that the browser renewed in the background.
  idbSet(TOKEN_KEY, token || null);
};
const getPending = () => {
  try { return JSON.parse(localStorage.getItem(PENDING_KEY) || "null"); } catch { return null; }
};
const setPending = (p) => {
  try { p ? localStorage.setItem(PENDING_KEY, JSON.stringify(p)) : localStorage.removeItem(PENDING_KEY); } catch {}
};

function idbSet(key, value) {
  try {
    const open = indexedDB.open("unseat", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("kv");
    open.onsuccess = () => {
      const tx = open.result.transaction("kv", "readwrite");
      value === null ? tx.objectStore("kv").delete(key) : tx.objectStore("kv").put(value, key);
      tx.oncomplete = () => open.result.close();
    };
  } catch {}
}

function show(id) {
  for (const s of ["install", "pair", "main", "removed"]) $(s).classList.toggle("hidden", s !== id);
}

async function api(path, { method = "GET", body } = {}) {
  const headers = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store", signal: AbortSignal.timeout(8000) });
  if (res.status === 401 && token) {
    setToken(null);
    show("removed");
    throw new Error("removed");
  }
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

function b64uToBytes(s) {
  const pad = "=".repeat((4 - (s.length % 4)) % 4);
  const bin = atob((s + pad).replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

// ---------------------------------------------------------------- pairing

function bytesToB64u(bytes) {
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function sha256Hex(text) {
  const d = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
  return Array.from(d, (b) => b.toString(16).padStart(2, "0")).join("");
}

// The phone makes its own secret and sends only its hash; the PC never hands a secret out.
async function pair() {
  $("pairBtn").disabled = true;
  $("pairMsg").textContent = "Sending code…";
  try {
    const secret = bytesToB64u(crypto.getRandomValues(new Uint8Array(32)));
    const body = { code: $("code").value, name: $("name").value, secretHash: await sha256Hex(secret) };
    const { reqId } = await api("/api/pair/request", { method: "POST", body });
    setPending({ reqId, secret, at: Date.now() });
    await waitForApproval();
  } catch (e) {
    $("pairMsg").textContent = e.message;
  } finally {
    $("pairBtn").disabled = false;
  }
}

let waiting = false;

// Poll until the PC decides. Network errors and timeouts just mean "try again"; the pending
// pairing is kept, so locking the phone or switching apps doesn't lose it.
async function waitForApproval() {
  if (waiting) return;
  waiting = true;
  try {
    let delay = 2000;
    for (;;) {
      const p = getPending();
      if (!p) return;
      if (Date.now() - p.at > PENDING_FOR_MS) return finishPending(p);
      $("pairMsg").textContent = "Code accepted. Approve this phone on the PC…";
      await new Promise((r) => setTimeout(r, delay));
      if (document.visibilityState !== "visible") continue;
      try {
        const r = await api(`/api/pair/poll?reqId=${encodeURIComponent(p.reqId)}`);
        if (r.status === "approved") return approved(p);
        if (r.status === "denied") {
          setPending(null);
          $("pairMsg").textContent = "The PC declined this phone.";
          return;
        }
        delay = 2000;
      } catch (e) {
        // The PC forgets a request after 10 minutes; the secret then tells whether it was approved.
        if (/unknown request/.test(e.message)) return finishPending(p);
        $("pairMsg").textContent = `Waiting for the PC (${e.message}); still trying…`;
        delay = Math.min(delay * 2, 15_000);
      }
    }
  } finally {
    waiting = false;
  }
}

function approved(p) {
  setToken(p.secret);
  setPending(null);
  $("pairMsg").textContent = "";
  show("main");
  return refresh();
}

// The request is gone from the PC (or too old): the phone is paired if its secret works.
async function finishPending(p) {
  try {
    const res = await fetch("/api/status", { headers: { Authorization: `Bearer ${p.secret}` }, cache: "no-store", signal: AbortSignal.timeout(8000) });
    if (res.ok) return approved(p);
    if (res.status === 401) {
      setPending(null);
      $("pairMsg").textContent = "Pairing wasn't approved in time. Make a new code on the PC and try again.";
    }
  } catch {
    $("pairMsg").textContent = "Can't reach the PC to finish pairing. Open the app again when it's online.";
  }
}

// ---------------------------------------------------------------- notifications

function enableNotifications() {
  // Apple: call subscribe immediately from the tap handler; no awaits before it.
  if (!config || !swReg) {
    $("statusMsg").textContent = "Not ready yet. Check the connection and try again.";
    return;
  }
  $("statusMsg").textContent = "Asking for permission…";
  swReg.pushManager
    .subscribe({ userVisibleOnly: true, applicationServerKey: b64uToBytes(config.vapidPublicKey) })
    .then((sub) => api("/api/subscribe", { method: "POST", body: { subscription: sub.toJSON() } }))
    .then(() => { $("statusMsg").textContent = "Notifications are on."; })
    .catch((e) => { $("statusMsg").textContent = `Couldn't turn on notifications: ${e.message}`; })
    .finally(refresh);
}

async function sendTest() {
  $("testBtn").disabled = true;
  try {
    await api("/api/test", { method: "POST", body: {} });
    $("statusMsg").textContent = "Test sent. It should arrive in a few seconds.";
  } catch (e) {
    $("statusMsg").textContent = e.message;
  } finally {
    setTimeout(() => { $("testBtn").disabled = false; }, 3000);
  }
}

async function currentSubscription() {
  return swReg ? swReg.pushManager.getSubscription() : null;
}

// ---------------------------------------------------------------- status and recovery

async function refresh() {
  clearTimeout(retryTimer);
  const perm = typeof Notification === "undefined" ? "unsupported" : Notification.permission;
  $("perm").textContent = { granted: "allowed", denied: "blocked in Settings", default: "not asked yet" }[perm] || perm;
  $("perm").className = perm === "granted" ? "ok" : perm === "denied" ? "bad" : "warn";
  let sub = null;
  try { sub = await currentSubscription(); } catch {}

  try {
    if (!config) config = await api("/api/config");
    let status = await api("/api/status");
    if (sub && status.subscribed && status.endpoint && status.endpoint !== sub.endpoint && !status.needsResubscribe) {
      // The browser renewed the subscription and the PC still has the old one.
      await api("/api/subscribe", { method: "POST", body: { subscription: sub.toJSON() } });
      status = await api("/api/status");
    }
    if (sub && status.needsResubscribe) {
      // Apple dropped the old subscription. Clear it here so the next tap makes a fresh one.
      await sub.unsubscribe().catch(() => {});
      sub = null;
    } else if (sub && !status.subscribed) {
      // The PC lost its copy (for example after removing and re-adding): send it again.
      await api("/api/subscribe", { method: "POST", body: { subscription: sub.toJSON() } });
      status = await api("/api/status");
    }
    backoff = 1000;
    $("conn").textContent = "connected";
    $("conn").className = "ok";
    $("timer").textContent = status.timer || "—";
    $("today").textContent = status.today || "";
    $("mode").textContent = status.enabled ? (status.alerts === "idle_only" ? "only while you're away from the PC" : "all reminders") : "off";
    $("offBanner").classList.toggle("hidden", status.enabled);
    $("resubBanner").classList.toggle("hidden", !status.needsResubscribe);
    $("delivered").textContent = String(status.delivered);
    // iOS only reports some push types back (not the default declarative ones on iOS 18.7).
    $("received").textContent = status.received ? String(status.received) : "not reported by iOS";
    $("missed").textContent = String(status.missed);
    const working = Boolean(sub) && status.subscribed && !status.needsResubscribe && perm === "granted";
    $("sub").textContent = working ? "receiving" : "not receiving";
    $("sub").className = working ? "ok" : "warn";
    $("enableBtn").classList.toggle("hidden", working);
    $("testBtn").classList.toggle("hidden", !working || !status.enabled);
  } catch (e) {
    if (e.message === "removed") return;
    $("conn").textContent = `can't reach the PC · retry in ${Math.round(backoff / 1000)} s`;
    $("conn").className = "bad";
    $("sub").textContent = sub ? "subscribed (PC unreachable)" : "not receiving";
    retryTimer = setTimeout(refresh, backoff);
    backoff = Math.min(backoff * 2, 30_000) + Math.round(Math.random() * 500);
  }
}

// ---------------------------------------------------------------- start

function start() {
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {});
    navigator.serviceWorker.ready.then((reg) => { swReg = reg; if (getToken()) refresh(); });
  }
  $("pairBtn").addEventListener("click", pair);
  $("enableBtn").addEventListener("click", enableNotifications);
  $("testBtn").addEventListener("click", sendTest);
  $("repairBtn").addEventListener("click", () => show("pair"));
  addEventListener("online", () => { backoff = 1000; if (getToken()) refresh(); });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "visible") return;
    if (getToken()) { backoff = 1000; refresh(); } else if (getPending()) waitForApproval();
  });

  const code = new URLSearchParams(location.hash.slice(1)).get("pair");
  if (!standalone) {
    if (code) {
      $("installCode").classList.remove("hidden");
      $("installCodeText").textContent = code;
    }
    return show("install");
  }
  if (getToken()) {
    idbSet(TOKEN_KEY, getToken());
    show("main");
    return refresh();
  }
  if (code) $("code").value = code;
  show("pair");
  if (getPending()) waitForApproval();
}

start();
