"use strict";
// Phase 1 test app. Pairs with the PC, subscribes to push, reports what the phone sees.

const $ = (id) => document.getElementById(id);
const TOKEN_KEY = "spike-token";
const standalone = navigator.standalone === true || matchMedia("(display-mode: standalone)").matches;
let config = null; // fetched before any tap so subscribe() runs straight from the gesture
let swReg = null; // likewise: no await between the tap and subscribe()
let backoff = 1000;
let retryTimer = null;

const getToken = () => {
  try { return localStorage.getItem(TOKEN_KEY); } catch { return null; }
};

async function setToken(token) {
  try { token ? localStorage.setItem(TOKEN_KEY, token) : localStorage.removeItem(TOKEN_KEY); } catch {}
  // The service worker can't read localStorage, so it gets its own copy for receipts.
  const cache = await caches.open("spike-auth");
  if (token) await cache.put("/__token", new Response(token));
  else await cache.delete("/__token");
}

function show(id) {
  for (const s of ["install", "pair", "paired", "revoked"]) $(s).classList.toggle("hidden", s !== id);
}

async function api(path, { method = "GET", body } = {}) {
  const headers = {};
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await fetch(path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), cache: "no-store", signal: AbortSignal.timeout(8000) });
  if (res.status === 401 && token) {
    await setToken(null);
    show("revoked");
    throw new Error("revoked");
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

const fmtTime = (ms) => (Number.isFinite(ms) ? new Date(ms).toLocaleTimeString() : "—");

// ---------------------------------------------------------------- pairing

async function pair() {
  const code = $("code").value;
  const name = $("name").value;
  $("pairBtn").disabled = true;
  $("pairMsg").textContent = "Sending code…";
  try {
    const { reqId } = await api("/api/pair/request", { method: "POST", body: { code, name } });
    $("pairMsg").textContent = "Code accepted. Approve this phone on the PC…";
    const started = Date.now();
    while (Date.now() - started < 5 * 60_000) {
      await new Promise((r) => setTimeout(r, 2000));
      const r = await api(`/api/pair/poll?reqId=${encodeURIComponent(reqId)}`);
      if (r.status === "approved" && r.token) {
        await setToken(r.token);
        $("pairMsg").textContent = "";
        return startPaired();
      }
      if (r.status === "denied") throw new Error("The PC denied this phone.");
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
  // Apple: call the subscribe method immediately from the tap handler. No awaits before it.
  if (!config || !swReg) {
    $("statusMsg").textContent = "Not ready yet; tap Check again.";
    return;
  }
  $("statusMsg").textContent = "Asking for permission…";
  swReg.pushManager
    .subscribe({ userVisibleOnly: true, applicationServerKey: b64uToBytes(config.vapidPublicKey) })
    .then((sub) => api("/api/subscribe", { method: "POST", body: { subscription: sub.toJSON() } }))
    .then(() => { $("statusMsg").textContent = "Notifications on. Lock the phone and send a test from the PC."; })
    .catch((e) => { $("statusMsg").textContent = `Could not enable: ${e.message}`; })
    .finally(refresh);
}

async function currentSubscription() {
  if (!("serviceWorker" in navigator)) return null;
  const reg = await navigator.serviceWorker.ready;
  return reg.pushManager.getSubscription();
}

// ---------------------------------------------------------------- receipts queued by the service worker

async function queuedReceipts() {
  const cache = await caches.open("spike-pending");
  const res = await cache.match("/__pending");
  return res ? res.json() : [];
}

async function flushReceipts() {
  const items = await queuedReceipts();
  if (!items.length) return 0;
  await api("/api/receipt", { method: "POST", body: { receipts: items.map((r) => ({ ...r, via: "flush" })) } });
  await (await caches.open("spike-pending")).delete("/__pending");
  return items.length;
}

// ---------------------------------------------------------------- status

async function refresh() {
  clearTimeout(retryTimer);
  const perm = typeof Notification === "undefined" ? "unsupported" : Notification.permission;
  $("perm").textContent = perm;
  $("perm").className = perm === "granted" ? "ok" : perm === "denied" ? "bad" : "warn";
  $("dStandalone").textContent = standalone ? "yes" : "no";
  $("dDecl").textContent = "pushManager" in window ? "available (iOS 18.4+)" : "not available";
  $("dSw").textContent = navigator.serviceWorker?.controller ? "active" : "not controlling yet";
  $("dOnline").textContent = navigator.onLine ? "yes" : "no";
  $("dUa").textContent = navigator.userAgent;
  $("queued").textContent = String((await queuedReceipts().catch(() => [])).length);

  let sub = null;
  try {
    sub = await currentSubscription();
  } catch {}
  $("sub").textContent = sub ? new URL(sub.endpoint).host : "none";
  $("sub").className = sub ? "ok" : "warn";
  $("enableBtn").classList.toggle("hidden", Boolean(sub) && perm === "granted");

  const t0 = performance.now();
  try {
    if (!config) config = await api("/api/config");
    const status = await api("/api/status");
    const rtt = Math.round(performance.now() - t0);
    $("conn").textContent = `connected · ${rtt} ms`;
    $("conn").className = "ok";
    backoff = 1000;
    // Keep the PC's copy of the subscription current (it is dropped after a 410 from Apple).
    if (sub && !status.subscribed) await api("/api/subscribe", { method: "POST", body: { subscription: sub.toJSON() } });
    const flushed = await flushReceipts().catch(() => 0);
    if (flushed) $("queued").textContent = "0";
    await api("/api/diag", {
      method: "POST",
      body: {
        userAgent: navigator.userAgent,
        standalone,
        permission: perm,
        windowPushManager: "pushManager" in window,
        serviceWorker: navigator.serviceWorker?.controller ? "active" : "none",
        subscribed: Boolean(sub),
        online: navigator.onLine,
      },
    });
    $("recent").innerHTML = "";
    for (const s of status.recent.slice().reverse()) {
      const tr = document.createElement("tr");
      const got = s.delivered ? fmtTime(s.phoneRecvAt ?? s.receiptAt) : s.status && s.status !== 201 ? `failed (${s.status})` : "—";
      for (const v of [s.seq, s.scenario, fmtTime(s.sentAt), got]) {
        const td = document.createElement("td");
        td.textContent = String(v);
        tr.appendChild(td);
      }
      $("recent").appendChild(tr);
    }
  } catch (e) {
    if (e.message === "revoked") return;
    $("conn").textContent = `not reachable · retry in ${Math.round(backoff / 1000)} s`;
    $("conn").className = "bad";
    retryTimer = setTimeout(refresh, backoff);
    backoff = Math.min(backoff * 2, 30_000) + Math.round(Math.random() * 500);
  }
}

async function reportOpened() {
  const seq = new URLSearchParams(location.search).get("seq");
  if (!seq) return;
  await api("/api/opened", { method: "POST", body: { seq: Number(seq) } }).catch(() => {});
  history.replaceState(null, "", "/");
}

function startPaired() {
  show("paired");
  reportOpened();
  refresh();
}

// ---------------------------------------------------------------- start

async function start() {
  if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {});
    navigator.serviceWorker.ready.then((reg) => { swReg = reg; });
  }
  $("pairBtn").addEventListener("click", pair);
  $("enableBtn").addEventListener("click", enableNotifications);
  $("refreshBtn").addEventListener("click", () => { backoff = 1000; refresh(); });
  $("repairBtn").addEventListener("click", () => show("pair"));
  addEventListener("online", () => { backoff = 1000; refresh(); });
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible" && getToken()) { backoff = 1000; refresh(); }
  });

  const code = new URLSearchParams(location.hash.slice(1)).get("pair");
  if (!standalone) {
    if (code) {
      $("installCode").classList.remove("hidden");
      $("installCodeText").textContent = code;
    }
    show("install");
    return;
  }
  if (getToken()) {
    await setToken(getToken()); // make sure the service worker has it too
    return startPaired();
  }
  if (code) $("code").value = code;
  show("pair");
}

start();
