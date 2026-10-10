"use strict";
// Shows every push at once (Safari cancels the subscription otherwise). On iOS 18.4+ the
// declarative messages Unseat sends are shown by iOS itself; older iOS hands them here.

const SHELL = "unseat-shell-2";
const SHELL_FILES = ["/", "/app.js", "/manifest.webmanifest", "/icon-180.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(SHELL).then((c) => c.addAll(SHELL_FILES)).catch(() => {}));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const name of await caches.keys()) if (name !== SHELL) await caches.delete(name);
      await self.clients.claim();
    })()
  );
});

// The app shell opens offline; API calls always go to the PC.
self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== "GET" || url.origin !== self.location.origin || url.pathname.startsWith("/api/")) return;
  const key = event.request.mode === "navigate" ? "/" : url.pathname;
  if (!SHELL_FILES.includes(key)) return;
  event.respondWith(
    (async () => {
      try {
        const res = await fetch(event.request);
        if (res.ok) (await caches.open(SHELL)).put(key, res.clone());
        return res;
      } catch {
        return (await caches.match(key)) || Response.error();
      }
    })()
  );
});

self.addEventListener("push", (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch {}
  const n = data && data.web_push === 8030 ? data.notification || {} : data || {};
  event.waitUntil(
    Promise.all([
      self.registration.showNotification(n.title || "Unseat", {
        body: n.body || "",
        tag: n.tag || "unseat",
        icon: "/icon-180.png",
        data: { navigate: n.navigate || "/" },
      }),
      receipt(n.navigate),
    ])
  );
});

// Tell the PC this reminder reached the phone (best effort; needs the tailnet).
async function receipt(navigate) {
  try {
    const m = /#e=(\d+)/.exec(navigate || "");
    const token = m && (await idbGet("unseat-token"));
    if (!token) return;
    await fetch("/api/receipt", {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ event: Number(m[1]) }),
      signal: AbortSignal.timeout(5000),
    });
  } catch {}
}

// The browser renewed the subscription in the background: send the new one to the PC, if
// the app has stored its token here. (Without it, the app re-sends it when next opened.)
self.addEventListener("pushsubscriptionchange", (event) => {
  event.waitUntil(
    (async () => {
      const token = await idbGet("unseat-token");
      if (!token) return;
      let sub = event.newSubscription;
      if (!sub) {
        const key = event.oldSubscription && event.oldSubscription.options.applicationServerKey;
        if (!key) return;
        sub = await self.registration.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: key });
      }
      await fetch("/api/subscribe", {
        method: "POST",
        headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
        body: JSON.stringify({ subscription: sub.toJSON() }),
      });
    })().catch(() => {})
  );
});

function idbGet(key) {
  return new Promise((resolve) => {
    try {
      const open = indexedDB.open("unseat", 1);
      open.onupgradeneeded = () => open.result.createObjectStore("kv");
      open.onerror = () => resolve(null);
      open.onsuccess = () => {
        const req = open.result.transaction("kv").objectStore("kv").get(key);
        req.onsuccess = () => { resolve(req.result || null); open.result.close(); };
        req.onerror = () => { resolve(null); open.result.close(); };
      };
    } catch {
      resolve(null);
    }
  });
}

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = (event.notification.data && event.notification.data.navigate) || "/";
  event.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const w of wins) if ("focus" in w) return w.focus();
      return self.clients.openWindow(url);
    })()
  );
});
