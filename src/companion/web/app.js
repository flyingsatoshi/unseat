"use strict";
// Unseat iPhone app: pairs with the PC, keeps the push subscription healthy, shows status.

const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "unseat-token";
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
};

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

async function pair() {
  $("pairBtn").disabled = true;
  $("pairMsg").textContent = "Sending code…";
  try {
    const { reqId } = await api("/api/pair/request", { method: "POST", body: { code: $("code").value, name: $("name").value } });
    $("pairMsg").textContent = "Code accepted. Approve this phone on the PC…";
    const started = Date.now();
    while (Date.now() - started < 5 * 60_000) {
      await new Promise((r) => setTimeout(r, 2000));
      const r = await api(`/api/pair/poll?reqId=${encodeURIComponent(reqId)}`);
      if (r.status === "approved" && r.token) {
        setToken(r.token);
        $("pairMsg").textContent = "";
        show("main");
        return refresh();
      }
      if (r.status === "denied") throw new Error("The PC declined this phone.");
    }
    throw new Error("Timed out waiting for approval.");
  } catch (e) {
    $("pairMsg").textContent = e.message;
  } finally {
    $("pairBtn").disabled = false;
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
    if (document.visibilityState === "visible" && getToken()) { backoff = 1000; refresh(); }
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
    show("main");
    return refresh();
  }
  if (code) $("code").value = code;
  show("pair");
}

start();
