import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import http from "node:http";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createSpike } from "../server.mjs";
import { decrypt, newSubscriber } from "./helpers.mjs";

const LOGIN = "owner@example.test";
let spike, dataDir, fakePush, fakeOrigin;
const pushes = [];
let pushStatus = 201;

function request(port, method, urlPath, { host, headers = {}, body, raw } = {}) {
  return new Promise((resolve, reject) => {
    const payload = raw ?? (body === undefined ? undefined : JSON.stringify(body));
    const req = http.request(
      { host: "127.0.0.1", port, method, path: urlPath, headers: { host: host ?? `127.0.0.1:${port}`, ...(payload !== undefined && !raw ? { "content-type": "application/json" } : {}), ...headers } },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => {
          const text = Buffer.concat(chunks).toString();
          let json = null;
          try {
            json = JSON.parse(text);
          } catch {}
          resolve({ status: res.statusCode, headers: res.headers, text, json });
        });
      }
    );
    req.on("error", reject);
    if (payload !== undefined) req.write(payload);
    req.end();
  });
}

const phone = (method, p, opts = {}) =>
  request(spike.cfg.phonePort, method, p, { ...opts, headers: { "tailscale-user-login": LOGIN, ...(opts.token ? { authorization: `Bearer ${opts.token}` } : {}), ...opts.headers } });
const admin = (method, p, opts = {}) => request(spike.cfg.adminPort, method, p, { ...opts, headers: { "x-spike-admin": "1", ...opts.headers } });
const waitFor = async (fn, ms = 3000) => {
  const start = Date.now();
  while (Date.now() - start < ms) {
    if (await fn()) return;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error("timed out");
};

before(async () => {
  dataDir = fs.mkdtempSync(path.join(os.tmpdir(), "unseat-spike-"));
  fakePush = http.createServer((req, res) => {
    const chunks = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => {
      pushes.push({ headers: req.headers, body: Buffer.concat(chunks), path: req.url });
      res.writeHead(pushStatus, { "content-type": "application/json", "apns-id": "test-id" });
      res.end(pushStatus === 201 ? "" : JSON.stringify({ reason: "Unregistered" }));
    });
  });
  await new Promise((r) => fakePush.listen(0, "127.0.0.1", r));
  fakeOrigin = `http://127.0.0.1:${fakePush.address().port}`;
  spike = await createSpike({ phonePort: 0, adminPort: 0, dataDir, publicHost: "pc.example.ts.net", extraPushOrigins: [fakeOrigin] }).listen();
});

after(async () => {
  await spike.close();
  await new Promise((r) => fakePush.close(r));
  fs.rmSync(dataDir, { recursive: true, force: true });
});

test("both listeners are bound to loopback only", async () => {
  // listen() resolved with 127.0.0.1; a request to the LAN address must fail.
  const lan = Object.values(os.networkInterfaces()).flat().find((i) => i && i.family === "IPv4" && !i.internal);
  if (!lan) return;
  for (const port of [spike.cfg.phonePort, spike.cfg.adminPort]) {
    await assert.rejects(
      new Promise((resolve, reject) => {
        const req = http.request({ host: lan.address, port, path: "/", timeout: 1500 }, resolve);
        req.on("error", reject);
        req.on("timeout", () => req.destroy(new Error("timeout")));
        req.end();
      })
    );
  }
});

test("phone side refuses requests that did not come through the tailnet", async () => {
  const r = await request(spike.cfg.phonePort, "GET", "/");
  assert.equal(r.status, 403);
});

test("phone side refuses unknown Host headers (DNS rebinding)", async () => {
  assert.equal((await phone("GET", "/", { host: "evil.example" })).status, 421);
});

test("phone side serves the app with a strict CSP through the tailnet host", async () => {
  const r = await phone("GET", "/", { host: "pc.example.ts.net" });
  assert.equal(r.status, 200);
  assert.match(r.headers["content-security-policy"], /default-src 'self'/);
  assert.equal((await phone("GET", "/../server.mjs")).status, 404);
  assert.equal((await phone("GET", "/data/vapid.json")).status, 404);
});

test("admin side needs the local Host and the admin header", async () => {
  assert.equal((await admin("GET", "/admin/state", { host: "pc.example.ts.net" })).status, 421);
  assert.equal((await request(spike.cfg.adminPort, "POST", "/admin/pair/new", { body: {} })).status, 403);
  assert.equal((await admin("GET", "/admin/state")).status, 200);
});

test("admin page is not reachable through the phone listener", async () => {
  assert.equal((await phone("GET", "/admin/state")).status, 404);
  assert.equal((await phone("POST", "/admin/pair/new", { body: {} })).status, 404);
});

test("JSON only, 8 KB max", async () => {
  assert.equal((await phone("POST", "/api/pair/request", { raw: "code=1", headers: { "content-type": "text/plain" } })).status, 415);
  assert.equal((await phone("POST", "/api/pair/request", { body: { code: "x".repeat(9000) } })).status, 413);
});

test("five wrong codes burn the pairing code", async () => {
  await admin("POST", "/admin/pair/new", { body: {} });
  for (let i = 0; i < 5; i++) assert.equal((await phone("POST", "/api/pair/request", { body: { code: "ZZZZZZZZ" } })).status, 403);
  const r = await phone("POST", "/api/pair/request", { body: { code: "ZZZZZZZZ" } });
  assert.match(r.json.error, /no active code/);
});

let token, deviceId;
const sub = newSubscriber();

test("pair: code → PC approval → token handed out once", async () => {
  const { json: created } = await admin("POST", "/admin/pair/new", { body: {} });
  const typed = created.code.toLowerCase().replace(/(....)/, "$1-");
  const { json: req } = await phone("POST", "/api/pair/request", { body: { code: typed, name: "Test <iPhone>" } });
  assert.ok(req.reqId);
  assert.equal((await phone("GET", `/api/pair/poll?reqId=${req.reqId}`)).json.status, "pending");
  const state = (await admin("GET", "/admin/state")).json;
  assert.equal(state.requests[0].login, LOGIN);
  assert.equal(state.pairing, null, "code consumed");
  await admin("POST", "/admin/pair/decide", { body: { reqId: req.reqId, approve: true } });
  const first = (await phone("GET", `/api/pair/poll?reqId=${req.reqId}`)).json;
  assert.equal(first.status, "approved");
  assert.ok(first.token);
  ({ token, deviceId } = first);
  const second = (await phone("GET", `/api/pair/poll?reqId=${req.reqId}`)).json;
  assert.equal(second.token, undefined);
  assert.equal((await admin("GET", "/admin/state")).json.devices[0].name, "Test iPhone");
});

test("device API needs the token and the same tailnet login", async () => {
  assert.equal((await phone("GET", "/api/status")).status, 401);
  assert.equal((await phone("GET", "/api/status", { token: "nope" })).status, 401);
  assert.equal((await phone("GET", "/api/status", { token, headers: { "tailscale-user-login": "someone@else.test" } })).status, 403);
  assert.equal((await phone("GET", "/api/status", { token })).status, 200);
});

test("subscriptions must point at Apple (or the test push origin)", async () => {
  const bad = { endpoint: "https://evil.example/push", keys: sub.keys };
  assert.equal((await phone("POST", "/api/subscribe", { token, body: { subscription: bad } })).status, 400);
  const good = { endpoint: `${fakeOrigin}/push/device-1`, keys: sub.keys };
  assert.equal((await phone("POST", "/api/subscribe", { token, body: { subscription: good } })).status, 200);
});

test("a test push is encrypted for the phone, declarative, and within Apple's limits", async () => {
  await admin("POST", "/admin/send", { body: { deviceId, scenario: "locked", ttl: 300, urgency: "high", topic: "unseat-alert" } });
  await waitFor(() => pushes.length === 1);
  const p = pushes[0];
  assert.equal(p.headers.ttl, "300");
  assert.equal(p.headers.urgency, "high");
  assert.equal(p.headers.topic, "unseat-alert");
  assert.equal(p.headers["content-encoding"], "aes128gcm");
  assert.match(p.headers.authorization, /^vapid t=.+, k=.+$/);
  assert.ok(p.body.length <= 4096);
  const msg = JSON.parse(decrypt(p.body, sub.privateKey, sub.auth));
  assert.equal(msg.web_push, 8030);
  assert.match(msg.notification.title, /Time to stand up \(test #1\)/);
  assert.equal(msg.notification.navigate, "https://pc.example.ts.net/?seq=1");
  await waitFor(async () => (await admin("GET", "/admin/state")).json.sends[0]?.status === 201);
});

test("receipts record delay; queued receipts count without a PC-clock delay", async () => {
  await phone("POST", "/api/receipt", { token, body: { seq: 1, phoneRecvAt: Date.now(), via: "sw" } });
  await admin("POST", "/admin/send", { body: { deviceId, scenario: "tailnet-off" } });
  await waitFor(() => pushes.length === 2);
  await phone("POST", "/api/receipt", { token, body: { receipts: [{ seq: 2, phoneRecvAt: Date.now(), via: "flush" }] } });
  await phone("POST", "/api/opened", { token, body: { seq: 1 } });
  await admin("POST", "/admin/seen", { body: { seq: 1, seen: true } });
  const { summary, sends } = (await admin("GET", "/admin/state")).json;
  const locked = summary.find((s) => s.scenario === "locked");
  assert.equal(locked.receipts, 1);
  assert.equal(locked.seen, 1);
  assert.equal(locked.opened, 1);
  assert.ok(Number.isFinite(locked.pcDelayMs.p50));
  const off = summary.find((s) => s.scenario === "tailnet-off");
  assert.equal(off.receipts, 1);
  assert.equal(off.queuedReceipts, 1);
  assert.equal(off.pcDelayMs.p50, null);
  assert.equal(sends.find((s) => s.seq === 2).via, "flush");
});

test("a receipt for another device's push is ignored", async () => {
  const before = (await admin("GET", "/admin/state")).json.sends.length;
  await phone("POST", "/api/receipt", { token, body: { seq: 999, via: "sw" } });
  assert.equal((await admin("GET", "/admin/state")).json.sends.length, before);
});

test("410 from the push service drops the subscription", async () => {
  pushStatus = 410;
  await admin("POST", "/admin/send", { body: { deviceId, scenario: "gone" } });
  await waitFor(async () => (await admin("GET", "/admin/state")).json.devices[0].subscribed === false);
  const s = (await admin("GET", "/admin/state")).json.sends.find((x) => x.scenario === "gone");
  assert.equal(s.status, 410);
  assert.equal(s.reason, "Unregistered");
  pushStatus = 201;
});

test("stored files hold no device secret in plain text", async () => {
  const stateText = fs.readFileSync(path.join(dataDir, "state.json"), "utf8");
  const results = fs.readFileSync(path.join(dataDir, "results.json"), "utf8");
  assert.ok(!stateText.includes(token));
  assert.ok(!results.includes(token));
  assert.ok(!results.includes("tokenHash"));
  assert.ok(!results.includes(sub.keys.auth), "results export leaves out push keys");
});

test("revoking a device locks it out at once", async () => {
  await admin("POST", "/admin/revoke", { body: { deviceId } });
  assert.equal((await phone("GET", "/api/status", { token })).status, 401);
  assert.equal((await admin("GET", "/admin/state")).json.devices.length, 0);
});
