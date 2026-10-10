// Pairing codes and device credentials. The PC keeps only a SHA-256 hash of each device secret.

import crypto from "node:crypto";

const ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"; // Crockford base32: no I, L, O, U
export const CODE_LENGTH = 8; // 40 bits
export const CODE_TTL_MS = 5 * 60_000;
export const CODE_MAX_ATTEMPTS = 5;

export function newCode(random = crypto.randomBytes) {
  const bytes = random(CODE_LENGTH);
  return Array.from(bytes, (b) => ALPHABET[b & 31]).join("");
}

/** Accept what people type: lower case, spaces, dashes, and O/I/L for 0/1. */
export function normalizeCode(input) {
  return String(input ?? "")
    .toUpperCase()
    .replace(/[\s-]/g, "")
    .replace(/O/g, "0")
    .replace(/[IL]/g, "1");
}

export const newToken = () => crypto.randomBytes(32).toString("base64url");
export const hashToken = (token) => crypto.createHash("sha256").update(String(token)).digest("hex");

export function tokenMatches(token, hashHex) {
  if (typeof token !== "string" || typeof hashHex !== "string") return false;
  const a = Buffer.from(hashToken(token), "hex");
  const b = Buffer.from(hashHex, "hex");
  return a.length === b.length && crypto.timingSafeEqual(a, b);
}

/**
 * One active pairing code at a time. A code is single use, expires after five minutes and is
 * burned after five wrong tries. A correct code only creates a request: the PC still approves it.
 */
export class Pairing {
  constructor(now = () => Date.now()) {
    this.now = now;
    this.active = null;
  }

  start() {
    this.active = { code: newCode(), expiresAt: this.now() + CODE_TTL_MS, attempts: 0 };
    return this.active;
  }

  current() {
    if (this.active && this.now() >= this.active.expiresAt) this.active = null;
    return this.active;
  }

  /** Returns "ok", "expired" (no live code) or "wrong". A correct code is consumed. */
  redeem(input) {
    const live = this.current();
    if (!live) return "expired";
    const given = Buffer.from(normalizeCode(input).padEnd(CODE_LENGTH).slice(0, 64));
    const want = Buffer.from(live.code.padEnd(CODE_LENGTH));
    const same = given.length === want.length && crypto.timingSafeEqual(given, want);
    if (same) {
      this.active = null;
      return "ok";
    }
    live.attempts += 1;
    if (live.attempts >= CODE_MAX_ATTEMPTS) this.active = null;
    return "wrong";
  }
}
