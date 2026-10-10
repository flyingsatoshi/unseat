// Web Push sender with no dependencies: VAPID (RFC 8292) and aes128gcm payload encryption
// (RFC 8291 / RFC 8188), using only node:crypto and fetch.

import crypto from "node:crypto";

export const b64u = {
  enc: (buf) => Buffer.from(buf).toString("base64url"),
  dec: (s) => Buffer.from(String(s), "base64url"),
};

// Apple: payloads over 4 KB are rejected (PayloadTooLarge).
export const MAX_BODY = 4096;
const RECORD_SIZE = 4096;

export function rawPublicKey(keyObject) {
  const jwk = keyObject.export({ format: "jwk" });
  return b64u.enc(Buffer.concat([Buffer.from([4]), b64u.dec(jwk.x), b64u.dec(jwk.y)]));
}

/** A new VAPID key pair: the private key as a JWK and the public key as base64url X9.62 bytes. */
export function generateVapidKeys() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync("ec", { namedCurve: "prime256v1" });
  return { privateJwk: privateKey.export({ format: "jwk" }), publicKey: rawPublicKey(publicKey) };
}

/**
 * Encrypt one push message for a subscription (single aes128gcm record).
 * `asPrivateKey` and `salt` are only passed by tests that reproduce RFC 8291's example.
 */
export function encrypt(plaintext, keys, { asPrivateKey, salt } = {}) {
  const uaPublic = b64u.dec(keys.p256dh);
  const authSecret = b64u.dec(keys.auth);
  if (uaPublic.length !== 65 || uaPublic[0] !== 4) throw new Error("p256dh must be an uncompressed P-256 point");
  if (authSecret.length !== 16) throw new Error("auth secret must be 16 bytes");

  const ecdh = crypto.createECDH("prime256v1");
  if (asPrivateKey) ecdh.setPrivateKey(asPrivateKey);
  else ecdh.generateKeys();
  const asPublic = ecdh.getPublicKey();
  const ecdhSecret = ecdh.computeSecret(uaPublic);

  // RFC 8291 section 3.4: IKM = HKDF(salt = auth_secret, ikm = ecdh_secret, info = key_info, 32)
  const keyInfo = Buffer.concat([Buffer.from("WebPush: info\0"), uaPublic, asPublic]);
  const ikm = Buffer.from(crypto.hkdfSync("sha256", ecdhSecret, authSecret, keyInfo, 32));

  // RFC 8188: content key and nonce from the record salt.
  salt = salt ?? crypto.randomBytes(16);
  const cek = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
  const nonce = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));

  const padded = Buffer.concat([Buffer.from(plaintext), Buffer.from([2])]); // last-record delimiter
  if (padded.length + 16 > RECORD_SIZE) throw new Error("payload does not fit one record");
  const cipher = crypto.createCipheriv("aes-128-gcm", cek, nonce);
  const ciphertext = Buffer.concat([cipher.update(padded), cipher.final(), cipher.getAuthTag()]);

  const header = Buffer.alloc(21);
  salt.copy(header, 0);
  header.writeUInt32BE(RECORD_SIZE, 16);
  header[20] = asPublic.length;
  return Buffer.concat([header, asPublic, ciphertext]);
}

/**
 * Builds VAPID Authorization headers. Apple asks senders not to refresh the JWT more than
 * once an hour and rejects an `exp` more than a day ahead, so each audience's token is
 * reused for an hour and expires after 12.
 */
export class Vapid {
  constructor({ privateJwk, publicKey }, subject, now = () => Date.now()) {
    if (!/^(https:|mailto:)/.test(subject)) throw new Error("VAPID subject must be an https: URL or mailto:");
    this.key = crypto.createPrivateKey({ key: privateJwk, format: "jwk" });
    this.publicKey = publicKey;
    this.subject = subject;
    this.now = now;
    this.cache = new Map();
  }

  header(endpoint) {
    const aud = new URL(endpoint).origin;
    const nowMs = this.now();
    const hit = this.cache.get(aud);
    if (hit && nowMs - hit.at < 3600_000) return hit.value;
    const head = b64u.enc(JSON.stringify({ typ: "JWT", alg: "ES256" }));
    const claims = b64u.enc(JSON.stringify({ aud, exp: Math.floor(nowMs / 1000) + 12 * 3600, sub: this.subject }));
    const sig = crypto.sign("sha256", Buffer.from(`${head}.${claims}`), { key: this.key, dsaEncoding: "ieee-p1363" });
    const value = `vapid t=${head}.${claims}.${b64u.enc(sig)}, k=${this.publicKey}`;
    this.cache.set(aud, { at: nowMs, value });
    return value;
  }
}

const TOPIC = /^[A-Za-z0-9_-]{1,32}$/;
const URGENCY = new Set(["very-low", "low", "normal", "high"]);

/** Only Apple's push service is ever contacted, so a phone can't make the PC call other hosts. */
export function isAllowedEndpoint(endpoint, extraOrigins = []) {
  let url;
  try {
    url = new URL(endpoint);
  } catch {
    return false;
  }
  if (extraOrigins.includes(url.origin)) return true;
  return url.protocol === "https:" && (url.hostname === "push.apple.com" || url.hostname.endsWith(".push.apple.com"));
}

/** Send one encrypted push. Resolves with the push service's status and Apple's error reason, if any. */
export async function sendPush(subscription, message, vapid, { ttl, urgency = "high", topic, fetchImpl = fetch, timeoutMs = 15000 } = {}) {
  if (!Number.isInteger(ttl) || ttl < 0) throw new Error("ttl must be a non-negative integer");
  if (!URGENCY.has(urgency)) throw new Error("bad urgency");
  if (topic && !TOPIC.test(topic)) throw new Error("topic must be 1-32 base64url characters");
  const body = encrypt(Buffer.from(JSON.stringify(message)), subscription.keys);
  if (body.length > MAX_BODY) throw new Error(`encrypted body is ${body.length} bytes; Apple's limit is ${MAX_BODY}`);
  const headers = {
    TTL: String(ttl),
    Urgency: urgency,
    "Content-Encoding": "aes128gcm",
    "Content-Type": "application/octet-stream",
    Authorization: vapid.header(subscription.endpoint),
  };
  if (topic) headers.Topic = topic;
  const res = await fetchImpl(subscription.endpoint, { method: "POST", headers, body, signal: AbortSignal.timeout(timeoutMs) });
  const text = await res.text().catch(() => "");
  let reason;
  try {
    reason = JSON.parse(text).reason;
  } catch {}
  return { status: res.status, reason: reason ?? null, apnsId: res.headers.get("apns-id") };
}
