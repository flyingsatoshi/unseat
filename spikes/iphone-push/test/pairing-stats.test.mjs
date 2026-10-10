import { test } from "node:test";
import assert from "node:assert/strict";
import { CODE_LENGTH, CODE_MAX_ATTEMPTS, CODE_TTL_MS, Pairing, hashToken, newCode, newToken, normalizeCode, tokenMatches } from "../lib/pairing.mjs";
import { percentile, summarize } from "../lib/stats.mjs";

test("codes use Crockford base32 without look-alike letters", () => {
  for (let i = 0; i < 200; i++) assert.match(newCode(), new RegExp(`^[0-9A-HJKMNP-TV-Z]{${CODE_LENGTH}}$`));
});

test("typed codes are normalized", () => {
  assert.equal(normalizeCode(" ab-cd o1l i "), "ABCD0111");
});

test("a code is single use", () => {
  const p = new Pairing();
  const { code } = p.start();
  assert.equal(p.redeem(code.toLowerCase()), "ok");
  assert.equal(p.redeem(code), "expired");
});

test("a code expires after five minutes", () => {
  let t = 0;
  const p = new Pairing(() => t);
  const { code } = p.start();
  t = CODE_TTL_MS;
  assert.equal(p.redeem(code), "expired");
});

test("five wrong tries burn the code", () => {
  const p = new Pairing();
  const { code } = p.start();
  for (let i = 0; i < CODE_MAX_ATTEMPTS; i++) assert.equal(p.redeem("WRONG000"), "wrong");
  assert.equal(p.redeem(code), "expired");
});

test("over-long input never matches", () => {
  const p = new Pairing();
  const { code } = p.start();
  assert.equal(p.redeem(code + "X".repeat(100)), "wrong");
});

test("tokens are 256-bit and only their hash is compared", () => {
  const t = newToken();
  assert.equal(Buffer.from(t, "base64url").length, 32);
  assert.ok(tokenMatches(t, hashToken(t)));
  assert.ok(!tokenMatches(newToken(), hashToken(t)));
  assert.ok(!tokenMatches(undefined, hashToken(t)));
});

test("percentile interpolates and ignores missing values", () => {
  assert.equal(percentile([], 50), null);
  assert.equal(percentile([5], 95), 5);
  assert.equal(percentile([1, 2, 3, 4, null, 5], 50), 3);
  assert.equal(percentile([0, 100], 95), 95);
});

test("summary counts deliveries and keeps queued receipts out of PC-clock delays", () => {
  const sends = [
    { scenario: "a", status: 201, sentAt: 0, delivered: true, via: "sw", receiptAt: 1000, phoneRecvAt: 900, seen: true },
    { scenario: "a", status: 201, sentAt: 0, delivered: true, via: "flush", receiptAt: null, phoneRecvAt: 2000, seen: true },
    { scenario: "a", status: 201, sentAt: 0, delivered: false, seen: false },
    { scenario: "b", status: 410, sentAt: 0, delivered: false },
  ];
  const [a, b] = summarize(sends);
  assert.equal(a.sent, 3);
  assert.equal(a.accepted, 3);
  assert.equal(a.receipts, 2);
  assert.equal(a.queuedReceipts, 1);
  assert.equal(a.seen, 2);
  assert.equal(a.notSeen, 1);
  assert.equal(a.pcDelayMs.p50, 1000);
  assert.equal(a.pcDelayMs.max, 1000);
  assert.equal(a.phoneDelayMs.p95, 1945);
  assert.equal(b.accepted, 0);
});
