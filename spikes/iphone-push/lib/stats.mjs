// Summaries for the Phase 1 report. Delays use the PC clock only (sent → receipt arrived back),
// so they are an upper bound on delivery time that includes waking the service worker and the
// receipt's trip over the tailnet. The phone-clock figure is shown separately because the two
// clocks can differ.

export function percentile(values, p) {
  const v = values.filter(Number.isFinite).sort((a, b) => a - b);
  if (!v.length) return null;
  const rank = (p / 100) * (v.length - 1);
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  return Math.round(v[lo] + (v[hi] - v[lo]) * (rank - lo));
}

export function summarize(sends) {
  const by = new Map();
  for (const s of sends) {
    const key = s.scenario || "unlabelled";
    if (!by.has(key)) by.set(key, []);
    by.get(key).push(s);
  }
  return [...by].map(([scenario, list]) => {
    const accepted = list.filter((s) => s.status === 201);
    const received = list.filter((s) => s.delivered);
    const pcDelay = list.filter((s) => Number.isFinite(s.receiptAt)).map((s) => s.receiptAt - s.sentAt);
    const phoneDelay = received.filter((s) => Number.isFinite(s.phoneRecvAt)).map((s) => s.phoneRecvAt - s.sentAt);
    return {
      scenario,
      sent: list.length,
      accepted: accepted.length,
      receipts: received.length,
      queuedReceipts: received.filter((s) => s.via === "flush").length,
      seen: list.filter((s) => s.seen === true).length,
      notSeen: list.filter((s) => s.seen === false).length,
      opened: list.filter((s) => Number.isFinite(s.openedAt)).length,
      pcDelayMs: { p50: percentile(pcDelay, 50), p95: percentile(pcDelay, 95), max: pcDelay.length ? Math.max(...pcDelay) : null },
      phoneDelayMs: { p50: percentile(phoneDelay, 50), p95: percentile(phoneDelay, 95) },
    };
  });
}
