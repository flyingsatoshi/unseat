// Receiver-side helpers for tests (not a test file itself).
import assert from "node:assert/strict";
import crypto from "node:crypto";
import { b64u } from "../lib/webpush.mjs";

/** Receiver side of RFC 8291, used to check what the sender produced. */
export function decrypt(body, uaPrivate, auth) {
  const ecdh = crypto.createECDH("prime256v1");
  ecdh.setPrivateKey(uaPrivate);
  const uaPublic = ecdh.getPublicKey();
  const salt = body.subarray(0, 16);
  const idlen = body[20];
  const asPublic = body.subarray(21, 21 + idlen);
  const ct = body.subarray(21 + idlen);
  const secret = ecdh.computeSecret(asPublic);
  const keyInfo = Buffer.concat([Buffer.from("WebPush: info\0"), uaPublic, asPublic]);
  const ikm = Buffer.from(crypto.hkdfSync("sha256", secret, auth, keyInfo, 32));
  const cek = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
  const nonce = Buffer.from(crypto.hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));
  const d = crypto.createDecipheriv("aes-128-gcm", cek, nonce);
  d.setAuthTag(ct.subarray(-16));
  const padded = Buffer.concat([d.update(ct.subarray(0, -16)), d.final()]);
  let end = padded.length - 1;
  while (end >= 0 && padded[end] === 0) end--;
  assert.equal(padded[end], 2, "last-record delimiter");
  return padded.subarray(0, end);
}

export function newSubscriber() {
  const ecdh = crypto.createECDH("prime256v1");
  ecdh.generateKeys();
  const auth = crypto.randomBytes(16);
  return { privateKey: ecdh.getPrivateKey(), auth, keys: { p256dh: b64u.enc(ecdh.getPublicKey()), auth: b64u.enc(auth) } };
}
