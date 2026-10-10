"use strict";
// Every push must show a notification at once (Safari revokes the subscription otherwise), so
// the notification is shown first and the receipt to the PC is best effort afterwards.

const SHELL = "spike-shell-1";
const SHELL_FILES = ["/", "/app.js", "/manifest.webmanifest", "/icon-180.png"];

self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(SHELL).then((c) => c.addAll(SHELL_FILES)).catch(() => {}));
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const keep = new Set([SHELL, "spike-auth", "spike-pending"]);
      for (const name of await caches.keys()) if (!keep.has(name)) await caches.delete(name);
      await self.clients.claim();
    })()
  );
});

// Shell: network first so updates show, cache when offline. API calls always go to the network.
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
  const phoneRecvAt = Date.now();
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch {}
  // Declarative messages ({"web_push": 8030, notification: {...}}) and plain ones look the same here.
  const n = data && data.web_push === 8030 ? data.notification || {} : data || {};
  const navigate = n.navigate || "/";
  let seq = Number(data.seq);
  if (!Number.isFinite(seq)) {
    try {
      seq = Number(new URL(navigate, self.location.origin).searchParams.get("seq"));
    } catch {}
  }
  event.waitUntil(
    (async () => {
      await self.registration.showNotification(n.title || "Unseat test", {
        body: n.body || "",
        tag: `unseat-test-${seq}`,
        icon: "/icon-180.png",
        data: { navigate, seq },
      });
      if (Number.isFinite(seq)) await report({ seq, phoneRecvAt });
    })()
  );
});

async function report(receipt) {
  try {
    const tokenRes = await (await caches.open("spike-auth")).match("/__token");
    if (!tokenRes) throw new Error("no token");
    const res = await fetch("/api/receipt", {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${await tokenRes.text()}` },
      body: JSON.stringify({ ...receipt, via: "sw" }),
      signal: AbortSignal.timeout(4000),
    });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
  } catch {
    // PC unreachable (tailnet off, PC asleep): keep it and send it when the app next opens.
    const cache = await caches.open("spike-pending");
    const prev = await cache.match("/__pending");
    const list = prev ? await prev.json() : [];
    list.push(receipt);
    await cache.put("/__pending", new Response(JSON.stringify(list.slice(-200))));
  }
}

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const url = (event.notification.data && event.notification.data.navigate) || "/";
  event.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: "window", includeUncontrolled: true });
      for (const w of wins) {
        if ("focus" in w) {
          if ("navigate" in w) await w.navigate(url).catch(() => {});
          return w.focus();
        }
      }
      return self.clients.openWindow(url);
    })()
  );
});
