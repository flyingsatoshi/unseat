// Phase 1 spike server. Two listeners, both on 127.0.0.1 only:
//   phone (8780): the test web app and its API. `tailscale serve` publishes this one to the
//                 tailnet over HTTPS. Nothing else does.
//   admin (8781): the PC-only control page (pairing QR, approvals, sending tests, results).
// Not part of Unseat. Keys, device hashes and results live in ./data (gitignored).

import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { Vapid, generateVapidKeys, isAllowedEndpoint, sendPush } from "./lib/webpush.mjs";
import { Pairing, hashToken, newToken, tokenMatches } from "./lib/pairing.mjs";
import { summarize } from "./lib/stats.mjs";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const qrcode = createRequire(import.meta.url)("./vendor/qrcode.cjs");

export function createSpike(opts = {}) {
  const cfg = {
    phonePort: opts.phonePort ?? Number(process.env.SPIKE_PHONE_PORT ?? 8780),
    adminPort: opts.adminPort ?? Number(process.env.SPIKE_ADMIN_PORT ?? 8781),
    dataDir: opts.dataDir ?? process.env.SPIKE_DATA ?? path.join(ROOT, "data"),
    publicHost: opts.publicHost ?? process.env.SPIKE_PUBLIC_HOST ?? tailscaleHost(),
    // Tests only: accept requests without Tailscale identity headers and extra push origins.
    allowNoTailscale: opts.allowNoTailscale ?? false,
    extraPushOrigins: opts.extraPushOrigins ?? [],
    subject: "https://github.com/flyingsatoshi/unseat",
    now: opts.now ?? (() => Date.now()),
  };
  const now = cfg.now;
  fs.mkdirSync(cfg.dataDir, { recursive: true });
  const file = (name) => path.join(cfg.dataDir, name);

  const vapidKeys = loadJson(file("vapid.json")) ?? saveJson(file("vapid.json"), generateVapidKeys());
  const vapid = new Vapid(vapidKeys, cfg.subject, now);
  const state = loadJson(file("state.json")) ?? { devices: [], sends: [], notes: [], nextSeq: 1 };
  const pairing = new Pairing(now);
  const requests = new Map(); // reqId -> { name, login, createdAt, status, token?, deviceId? } (memory only)
  let pairHits = [];

  const persist = () => {
    saveJson(file("state.json"), state);
    saveJson(file("results.json"), exportResults());
  };
  const log = (event, detail = {}) => {
    fs.appendFileSync(file("log.jsonl"), JSON.stringify({ at: new Date(now()).toISOString(), event, ...detail }) + "\n");
  };

  // ---------------------------------------------------------------- phone API
  const phoneHosts = new Set([`127.0.0.1:${cfg.phonePort}`, `localhost:${cfg.phonePort}`]);
  if (cfg.publicHost) phoneHosts.add(cfg.publicHost);

  const PHONE_FILES = {
    "/": ["public/index.html", "text/html; charset=utf-8"],
    "/index.html": ["public/index.html", "text/html; charset=utf-8"],
    "/app.js": ["public/app.js", "text/javascript; charset=utf-8"],
    "/sw.js": ["public/sw.js", "text/javascript; charset=utf-8"],
    "/manifest.webmanifest": ["public/manifest.webmanifest", "application/manifest+json"],
    "/icon-180.png": ["public/icon-180.png", "image/png"],
    "/icon-512.png": ["public/icon-512.png", "image/png"],
  };

  function deviceFor(req, res) {
    const auth = req.headers.authorization ?? "";
    const token = auth.startsWith("Bearer ") ? auth.slice(7) : "";
    const device = state.devices.find((d) => tokenMatches(token, d.tokenHash));
    if (!device) {
      send(res, 401, { error: "not paired or revoked" });
      return null;
    }
    if (device.login !== tsLogin(req)) {
      send(res, 403, { error: "tailnet login does not match the paired login" });
      return null;
    }
    device.lastSeen = now();
    return device;
  }

  function tsLogin(req) {
    const login = req.headers["tailscale-user-login"];
    if (login) return String(login);
    return cfg.allowNoTailscale ? "local-test" : null;
  }

  async function phone(req, res) {
    if (!phoneHosts.has(req.headers.host ?? "")) return send(res, 421, { error: "wrong host" });
    if (!tsLogin(req)) return send(res, 403, { error: "only reachable through the tailnet" });
    const url = new URL(req.url, "http://x");
    const route = `${req.method} ${url.pathname}`;

    if (req.method === "GET" && PHONE_FILES[url.pathname]) return serveFile(res, ...PHONE_FILES[url.pathname]);

    switch (route) {
      case "GET /api/config":
        return send(res, 200, { vapidPublicKey: vapidKeys.publicKey, serverTime: now() });

      case "POST /api/pair/request": {
        const body = await readJson(req, res);
        if (!body) return;
        pairHits = pairHits.filter((t) => now() - t < 60_000);
        if (pairHits.length >= 10) return send(res, 429, { error: "too many pairing attempts; wait a minute" });
        pairHits.push(now());
        const result = pairing.redeem(body.code);
        log("pair-attempt", { result });
        if (result !== "ok") return send(res, 403, { error: result === "expired" ? "no active code; make a new one on the PC" : "wrong code" });
        const reqId = crypto.randomUUID();
        requests.set(reqId, { name: cleanName(body.name), login: tsLogin(req), createdAt: now(), status: "pending" });
        return send(res, 200, { reqId });
      }

      case "GET /api/pair/poll": {
        const r = requests.get(url.searchParams.get("reqId") ?? "");
        if (!r) return send(res, 404, { error: "unknown request" });
        if (r.status === "approved" && r.token) {
          const token = r.token;
          delete r.token; // handed out once
          return send(res, 200, { status: "approved", deviceId: r.deviceId, token });
        }
        return send(res, 200, { status: r.status });
      }

      case "POST /api/subscribe": {
        const device = deviceFor(req, res);
        if (!device) return;
        const body = await readJson(req, res);
        if (!body) return;
        const sub = body.subscription;
        if (!sub || !isAllowedEndpoint(sub.endpoint, cfg.extraPushOrigins) || !sub.keys?.p256dh || !sub.keys?.auth)
          return send(res, 400, { error: "subscription must point at Apple's push service" });
        device.subscription = { endpoint: sub.endpoint, keys: { p256dh: sub.keys.p256dh, auth: sub.keys.auth } };
        device.subscribedAt = now();
        log("subscribed", { device: device.id, endpointHost: new URL(sub.endpoint).host });
        persist();
        return send(res, 200, { ok: true });
      }

      case "POST /api/receipt": {
        const device = deviceFor(req, res);
        if (!device) return;
        const body = await readJson(req, res);
        if (!body) return;
        const items = Array.isArray(body.receipts) ? body.receipts.slice(0, 200) : [body];
        for (const item of items) {
          const s = state.sends.find((x) => x.seq === Number(item.seq) && x.deviceId === device.id);
          if (!s) continue;
          if (!s.delivered) {
            s.delivered = true;
            s.via = item.via === "flush" ? "flush" : "sw";
            s.phoneRecvAt = Number.isFinite(item.phoneRecvAt) ? item.phoneRecvAt : null;
            // A flushed receipt was queued on the phone (e.g. tailnet down) and sent later, so the
            // PC-clock delay would be wrong; only the phone clock knows when that push landed.
            if (s.via === "sw") s.receiptAt = now();
            else s.flushedAt = now();
          }
        }
        persist();
        return send(res, 200, { ok: true });
      }

      case "POST /api/opened": {
        const device = deviceFor(req, res);
        if (!device) return;
        const body = await readJson(req, res);
        if (!body) return;
        const s = state.sends.find((x) => x.seq === Number(body.seq) && x.deviceId === device.id);
        if (s && !Number.isFinite(s.openedAt)) s.openedAt = now();
        persist();
        return send(res, 200, { ok: true });
      }

      case "POST /api/diag": {
        const device = deviceFor(req, res);
        if (!device) return;
        const body = await readJson(req, res);
        if (!body) return;
        device.diag = {
          at: now(),
          userAgent: String(body.userAgent ?? "").slice(0, 300),
          standalone: Boolean(body.standalone),
          permission: String(body.permission ?? "").slice(0, 20),
          windowPushManager: Boolean(body.windowPushManager),
          serviceWorker: String(body.serviceWorker ?? "").slice(0, 20),
          subscribed: Boolean(body.subscribed),
          online: Boolean(body.online),
        };
        persist();
        return send(res, 200, { ok: true });
      }

      case "GET /api/status": {
        const device = deviceFor(req, res);
        if (!device) return;
        const recent = state.sends
          .filter((s) => s.deviceId === device.id)
          .slice(-15)
          .map(({ seq, scenario, sentAt, status, delivered, receiptAt, phoneRecvAt, openedAt }) => ({ seq, scenario, sentAt, status, delivered, receiptAt, phoneRecvAt, openedAt }));
        persist();
        return send(res, 200, { name: device.name, serverTime: now(), subscribed: Boolean(device.subscription), recent });
      }
    }
    return send(res, 404, { error: "not found" });
  }

  // ---------------------------------------------------------------- admin (PC only)
  const adminHosts = new Set([`127.0.0.1:${cfg.adminPort}`, `localhost:${cfg.adminPort}`]);

  async function admin(req, res) {
    // Host check stops DNS rebinding; the custom header stops cross-site form posts.
    if (!adminHosts.has(req.headers.host ?? "")) return send(res, 421, { error: "wrong host" });
    if (req.method === "POST" && req.headers["x-spike-admin"] !== "1") return send(res, 403, { error: "missing admin header" });
    const url = new URL(req.url, "http://x");
    const route = `${req.method} ${url.pathname}`;

    switch (route) {
      case "GET /":
        return serveFile(res, "admin/admin.html", "text/html; charset=utf-8");
      case "GET /admin.js":
        return serveFile(res, "admin/admin.js", "text/javascript; charset=utf-8");

      case "GET /admin/state": {
        const live = pairing.current();
        return send(res, 200, {
          publicUrl: cfg.publicHost ? `https://${cfg.publicHost}/` : null,
          pairing: live ? { code: live.code, expiresAt: live.expiresAt, attemptsLeft: 5 - live.attempts } : null,
          requests: [...requests].filter(([, r]) => r.status === "pending").map(([reqId, r]) => ({ reqId, name: r.name, login: r.login, createdAt: r.createdAt })),
          devices: state.devices.map(publicDevice),
          sends: state.sends.slice(-200),
          summary: summarize(state.sends),
          notes: state.notes.slice(-50),
          serverTime: now(),
        });
      }

      case "GET /admin/qr.svg": {
        const live = pairing.current();
        if (!live || !cfg.publicHost) return send(res, 404, { error: "no active code or no public URL" });
        const qr = qrcode(0, "M");
        qr.addData(`https://${cfg.publicHost}/#pair=${live.code}`);
        qr.make();
        res.writeHead(200, { "Content-Type": "image/svg+xml", "Cache-Control": "no-store", ...SECURITY_HEADERS });
        return res.end(qr.createSvgTag({ cellSize: 6, margin: 12, scalable: true }));
      }

      case "POST /admin/pair/new": {
        const p = pairing.start();
        log("pair-code");
        return send(res, 200, { code: p.code, expiresAt: p.expiresAt });
      }

      case "POST /admin/pair/decide": {
        const body = await readJson(req, res);
        if (!body) return;
        const r = requests.get(String(body.reqId));
        if (!r || r.status !== "pending") return send(res, 404, { error: "no such pending request" });
        if (!body.approve) {
          r.status = "denied";
          log("pair-denied");
          return send(res, 200, { ok: true });
        }
        const token = newToken();
        const device = { id: crypto.randomUUID(), name: r.name, login: r.login, tokenHash: hashToken(token), createdAt: now(), lastSeen: null, subscription: null };
        state.devices.push(device);
        Object.assign(r, { status: "approved", token, deviceId: device.id });
        log("paired", { device: device.id, name: device.name });
        persist();
        return send(res, 200, { ok: true, deviceId: device.id });
      }

      case "POST /admin/revoke": {
        const body = await readJson(req, res);
        if (!body) return;
        const before = state.devices.length;
        state.devices = state.devices.filter((d) => d.id !== body.deviceId);
        log("revoked", { device: body.deviceId });
        persist();
        return send(res, 200, { ok: before !== state.devices.length });
      }

      case "POST /admin/send": {
        const body = await readJson(req, res);
        if (!body) return;
        const device = state.devices.find((d) => d.id === body.deviceId);
        if (!device?.subscription) return send(res, 400, { error: "device has no push subscription" });
        const count = clampInt(body.count, 1, 50, 1);
        const intervalSec = clampInt(body.intervalSec, 0, 600, 0);
        const job = {
          scenario: String(body.scenario || "manual").slice(0, 40),
          kind: body.kind === "legacy" ? "legacy" : "declarative",
          ttl: clampInt(body.ttl, 0, 86400, 300),
          urgency: ["very-low", "low", "normal", "high"].includes(body.urgency) ? body.urgency : "high",
          topic: /^[A-Za-z0-9_-]{1,32}$/.test(body.topic ?? "") ? body.topic : undefined,
        };
        for (let i = 0; i < count; i++) setTimeout(() => pushOne(device.id, job), i * intervalSec * 1000);
        return send(res, 200, { ok: true, queued: count });
      }

      case "POST /admin/seen": {
        const body = await readJson(req, res);
        if (!body) return;
        const s = state.sends.find((x) => x.seq === Number(body.seq));
        if (!s) return send(res, 404, { error: "no such send" });
        s.seen = body.seen === null ? null : Boolean(body.seen);
        persist();
        return send(res, 200, { ok: true });
      }

      case "POST /admin/note": {
        const body = await readJson(req, res);
        if (!body) return;
        state.notes.push({ at: now(), text: String(body.text ?? "").slice(0, 500) });
        persist();
        return send(res, 200, { ok: true });
      }

      case "GET /admin/export":
        return send(res, 200, exportResults());

      case "GET /admin/tailscale":
        return send(res, 200, { serve: tailscaleText(["serve", "status"]), funnel: tailscaleText(["funnel", "status"]) });
    }
    return send(res, 404, { error: "not found" });
  }

  async function pushOne(deviceId, job) {
    const device = state.devices.find((d) => d.id === deviceId);
    if (!device?.subscription) return;
    const seq = state.nextSeq++;
    const sentAt = now();
    const time = new Date(sentAt).toLocaleTimeString("en-GB");
    const title = `Time to stand up (test #${seq})`;
    const body = `Unseat Phase 1 test. Sent ${time}. Scenario: ${job.scenario}.`;
    const navigate = `https://${cfg.publicHost ?? "localhost"}/?seq=${seq}`;
    // Declarative Web Push (iOS 18.4+) shows this even without the service worker; older iOS
    // hands the same JSON to the service worker, which shows it. Either way every push is shown.
    const message = job.kind === "declarative"
      ? { web_push: 8030, notification: { title, body, navigate, silent: false } }
      : { title, body, navigate, seq };
    const record = { seq, deviceId, ...job, topic: job.topic ?? null, sentAt, status: null, reason: null, apnsId: null, error: null, delivered: false, via: null, receiptAt: null, phoneRecvAt: null, openedAt: null, seen: null };
    state.sends.push(record);
    try {
      const r = await sendPush(device.subscription, message, vapid, { ttl: job.ttl, urgency: job.urgency, topic: job.topic });
      Object.assign(record, { status: r.status, reason: r.reason, apnsId: r.apnsId, respondedAt: now() });
      if (r.status === 404 || r.status === 410) {
        device.subscription = null;
        log("subscription-gone", { device: device.id, status: r.status, reason: r.reason });
      }
    } catch (err) {
      record.error = String(err?.cause?.code ?? err?.name ?? err).slice(0, 120);
    }
    log("push", { seq, scenario: job.scenario, status: record.status, reason: record.reason, error: record.error });
    persist();
  }

  function exportResults() {
    return {
      generatedAt: new Date(now()).toISOString(),
      node: process.version,
      devices: state.devices.map(publicDevice),
      summary: summarize(state.sends),
      sends: state.sends.map(({ deviceId, apnsId, ...rest }) => rest),
      notes: state.notes,
    };
  }

  const phoneServer = http.createServer((req, res) => phone(req, res).catch((e) => fail(res, e)));
  const adminServer = http.createServer((req, res) => admin(req, res).catch((e) => fail(res, e)));
  for (const s of [phoneServer, adminServer]) {
    s.headersTimeout = 10_000;
    s.requestTimeout = 15_000;
    s.maxHeadersCount = 50;
  }

  return {
    cfg,
    state,
    pairing,
    pushOne,
    async listen() {
      await new Promise((r) => phoneServer.listen(cfg.phonePort, "127.0.0.1", r));
      await new Promise((r) => adminServer.listen(cfg.adminPort, "127.0.0.1", r));
      cfg.phonePort = phoneServer.address().port;
      cfg.adminPort = adminServer.address().port;
      phoneHosts.add(`127.0.0.1:${cfg.phonePort}`);
      adminHosts.add(`127.0.0.1:${cfg.adminPort}`);
      return this;
    },
    async close() {
      await Promise.all([phoneServer, adminServer].map((s) => new Promise((r) => s.close(r))));
    },
  };
}

// ------------------------------------------------------------------ helpers

const SECURITY_HEADERS = {
  "X-Content-Type-Options": "nosniff",
  "Referrer-Policy": "no-referrer",
  "X-Frame-Options": "DENY",
  "Content-Security-Policy":
    "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; manifest-src 'self'; worker-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
};

function send(res, status, obj) {
  res.writeHead(status, { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store", ...SECURITY_HEADERS });
  res.end(JSON.stringify(obj));
}

function serveFile(res, rel, type) {
  let data;
  try {
    data = fs.readFileSync(path.join(ROOT, rel));
  } catch {
    return send(res, 404, { error: "missing file" });
  }
  res.writeHead(200, { "Content-Type": type, "Cache-Control": "no-cache", ...SECURITY_HEADERS });
  res.end(data);
}

function fail(res, err) {
  console.error(err);
  if (!res.headersSent) send(res, 500, { error: "internal error" });
}

async function readJson(req, res, limit = 8192) {
  if (!String(req.headers["content-type"] ?? "").startsWith("application/json")) {
    send(res, 415, { error: "JSON only" });
    return null;
  }
  const chunks = [];
  let size = 0;
  for await (const chunk of req) {
    size += chunk.length;
    if (size > limit) {
      send(res, 413, { error: "body too large" });
      req.destroy();
      return null;
    }
    chunks.push(chunk);
  }
  try {
    const v = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    if (v && typeof v === "object") return v;
  } catch {}
  send(res, 400, { error: "bad JSON" });
  return null;
}

function publicDevice(d) {
  return {
    id: d.id,
    name: d.name,
    login: d.login,
    createdAt: d.createdAt,
    lastSeen: d.lastSeen,
    subscribed: Boolean(d.subscription),
    subscribedAt: d.subscribedAt ?? null,
    endpointHost: d.subscription ? new URL(d.subscription.endpoint).host : null,
    diag: d.diag ?? null,
  };
}

const cleanName = (n) => String(n ?? "").replace(/[^\p{L}\p{N} '._-]/gu, "").trim().slice(0, 40) || "iPhone";
const clampInt = (v, lo, hi, dflt) => (Number.isFinite(Number(v)) ? Math.min(hi, Math.max(lo, Math.round(Number(v)))) : dflt);

function loadJson(p) {
  try {
    return JSON.parse(fs.readFileSync(p, "utf8"));
  } catch {
    return null;
  }
}

function saveJson(p, v) {
  const tmp = `${p}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(v, null, 2));
  fs.renameSync(tmp, p);
  return v;
}

function tailscaleText(args) {
  try {
    return execFileSync("tailscale", args, { encoding: "utf8", timeout: 5000, windowsHide: true }).trim();
  } catch (e) {
    return `unavailable: ${String(e.stderr || e.message).trim().slice(0, 200)}`;
  }
}

function tailscaleHost() {
  try {
    const out = execFileSync("tailscale", ["status", "--json"], { encoding: "utf8", timeout: 5000, windowsHide: true });
    const name = JSON.parse(out)?.Self?.DNSName;
    return name ? name.replace(/\.$/, "") : null;
  } catch {
    return null;
  }
}

// ------------------------------------------------------------------ main

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const spike = await createSpike().listen();
  const { cfg } = spike;
  console.log(`Phone app: http://127.0.0.1:${cfg.phonePort}  (publish with: tailscale serve --bg ${cfg.phonePort})`);
  console.log(`Tailnet URL: ${cfg.publicHost ? `https://${cfg.publicHost}/` : "unknown (is Tailscale running?)"}`);
  console.log(`PC control page: http://127.0.0.1:${cfg.adminPort}/  (this PC only)`);
  console.log("Press Ctrl+C to stop.");
}
