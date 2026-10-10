import { test } from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import { Vapid, b64u, encrypt, generateVapidKeys, isAllowedEndpoint, sendPush, MAX_BODY } from "../lib/webpush.mjs";
import { decrypt, newSubscriber } from "./helpers.mjs";

// RFC 8291 section 5 example values.
const RFC = {
  plaintext: "When I grow up, I want to be a watermelon",
  auth: "BTBZMqHH6r4Tts7J_aSIgg",
  uaPrivate: "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94",
  uaPublic: "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
  asPrivate: "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw",
  body:
    "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml" +
    "mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT" +
    "pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN",
};

test("matches the RFC 8291 example byte for byte", () => {
  const expected = b64u.dec(RFC.body);
  // 86-byte header + 41-byte text + 1 delimiter + 16-byte tag. (The RFC's example request says
  // Content-Length: 145, which doesn't match its own body.)
  assert.equal(expected.length, 144);
  const out = encrypt(Buffer.from(RFC.plaintext), { p256dh: RFC.uaPublic, auth: RFC.auth }, {
    asPrivateKey: b64u.dec(RFC.asPrivate),
    salt: expected.subarray(0, 16),
  });
  assert.equal(b64u.enc(out), RFC.body);
});

test("the RFC example decrypts with the receiver's key", () => {
  const pt = decrypt(b64u.dec(RFC.body), b64u.dec(RFC.uaPrivate), b64u.dec(RFC.auth));
  assert.equal(pt.toString(), RFC.plaintext);
});

test("random encryptions round-trip and never reuse the salt or sender key", () => {
  const sub = newSubscriber();
  const a = encrypt(Buffer.from("hello"), sub.keys);
  const b = encrypt(Buffer.from("hello"), sub.keys);
  assert.equal(decrypt(a, sub.privateKey, sub.auth).toString(), "hello");
  assert.notDeepEqual(a.subarray(0, 16), b.subarray(0, 16));
  assert.notDeepEqual(a.subarray(21, 86), b.subarray(21, 86));
});

test("rejects malformed subscription keys", () => {
  const sub = newSubscriber();
  assert.throws(() => encrypt(Buffer.from("x"), { ...sub.keys, auth: b64u.enc(Buffer.alloc(8)) }));
  assert.throws(() => encrypt(Buffer.from("x"), { ...sub.keys, p256dh: b64u.enc(Buffer.alloc(33, 2)) }));
});

test("VAPID header is a valid ES256 JWT for the push origin", () => {
  const keys = generateVapidKeys();
  let t = 1_800_000_000_000;
  const vapid = new Vapid(keys, "https://github.com/flyingsatoshi/unseat", () => t);
  const h = vapid.header("https://web.push.apple.com/QGuQyavXutnMPQ2eCi1d8Ft6V8pnTO5L");
  const m = /^vapid t=([^.]+)\.([^.]+)\.([^,]+), k=(.+)$/.exec(h);
  assert.ok(m, h);
  assert.deepEqual(JSON.parse(b64u.dec(m[1])), { typ: "JWT", alg: "ES256" });
  const claims = JSON.parse(b64u.dec(m[2]));
  assert.equal(claims.aud, "https://web.push.apple.com");
  assert.equal(claims.sub, "https://github.com/flyingsatoshi/unseat");
  assert.ok(claims.exp > t / 1000 && claims.exp <= t / 1000 + 24 * 3600, "exp within a day (Apple BadJwtToken rule)");
  assert.equal(m[4], keys.publicKey);
  const raw = b64u.dec(keys.publicKey);
  const pub = crypto.createPublicKey({ key: { kty: "EC", crv: "P-256", x: b64u.enc(raw.subarray(1, 33)), y: b64u.enc(raw.subarray(33)) }, format: "jwk" });
  assert.ok(crypto.verify("sha256", Buffer.from(`${m[1]}.${m[2]}`), { key: pub, dsaEncoding: "ieee-p1363" }, b64u.dec(m[3])));

  // Reused within the hour (Apple: don't refresh more than hourly), renewed after.
  t += 30 * 60_000;
  assert.equal(vapid.header("https://web.push.apple.com/other"), h);
  t += 31 * 60_000;
  assert.notEqual(vapid.header("https://web.push.apple.com/other"), h);
});

test("VAPID subject must be https: or mailto:", () => {
  assert.throws(() => new Vapid(generateVapidKeys(), "unseat"));
});

test("only Apple's push service is an allowed endpoint", () => {
  assert.ok(isAllowedEndpoint("https://web.push.apple.com/abc"));
  assert.ok(!isAllowedEndpoint("http://web.push.apple.com/abc"));
  assert.ok(!isAllowedEndpoint("https://push.apple.com.evil.example/abc"));
  assert.ok(!isAllowedEndpoint("https://fcm.googleapis.com/fcm/send/abc"));
  assert.ok(!isAllowedEndpoint("https://127.0.0.1:8781/admin/revoke"));
  assert.ok(!isAllowedEndpoint("not a url"));
  assert.ok(isAllowedEndpoint("http://127.0.0.1:9999/push/1", ["http://127.0.0.1:9999"]));
});

test("sendPush sets Apple's headers and reports the reason", async () => {
  const sub = newSubscriber();
  const vapid = new Vapid(generateVapidKeys(), "https://github.com/flyingsatoshi/unseat");
  let seen;
  const fetchImpl = async (url, init) => {
    seen = { url, init };
    return new Response(JSON.stringify({ reason: "BadUrgency" }), { status: 400, headers: { "apns-id": "abc" } });
  };
  const r = await sendPush({ endpoint: "https://web.push.apple.com/x", keys: sub.keys }, { web_push: 8030 }, vapid, { ttl: 60, urgency: "normal", topic: "unseat-alert", fetchImpl });
  assert.deepEqual(r, { status: 400, reason: "BadUrgency", apnsId: "abc" });
  assert.equal(seen.init.headers.TTL, "60");
  assert.equal(seen.init.headers.Urgency, "normal");
  assert.equal(seen.init.headers.Topic, "unseat-alert");
  assert.equal(seen.init.headers["Content-Encoding"], "aes128gcm");
  assert.match(seen.init.headers.Authorization, /^vapid t=/);
  assert.equal(JSON.parse(decrypt(seen.init.body, sub.privateKey, sub.auth)).web_push, 8030);
});

test("sendPush refuses bad options and oversized payloads", async () => {
  const sub = { endpoint: "https://web.push.apple.com/x", keys: newSubscriber().keys };
  const vapid = new Vapid(generateVapidKeys(), "https://github.com/flyingsatoshi/unseat");
  const fetchImpl = async () => new Response("", { status: 201 });
  await assert.rejects(sendPush(sub, {}, vapid, { ttl: -1, fetchImpl }));
  await assert.rejects(sendPush(sub, {}, vapid, { ttl: 1, urgency: "now", fetchImpl }));
  await assert.rejects(sendPush(sub, {}, vapid, { ttl: 1, topic: "has spaces", fetchImpl }));
  await assert.rejects(sendPush(sub, { big: "x".repeat(MAX_BODY) }, vapid, { ttl: 1, fetchImpl }));
  assert.equal((await sendPush(sub, { ok: 1 }, vapid, { ttl: 1, fetchImpl })).status, 201);
});
