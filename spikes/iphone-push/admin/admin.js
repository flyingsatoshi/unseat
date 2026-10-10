"use strict";

const $ = (id) => document.getElementById(id);

// Each scenario maps to one Phase 1 check. Numbers are this spike's defaults, not Apple limits.
const SCENARIOS = [
  { id: "locked", label: "1. Locked phone", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Lock the iPhone and wait 10 s, then Send. Mark whether it appeared on the lock screen." },
  { id: "latency", label: "2. Latency & reliability batch (20 × 30 s)", count: 20, interval: 30, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Phone locked, on normal Wi-Fi with Tailscale on. Takes 10 minutes. Then mark each push seen / not seen." },
  { id: "latency-normal", label: "2b. Urgency normal (5 × 30 s)", count: 5, interval: 30, ttl: 300, urgency: "normal", kind: "declarative",
    hint: "Same as 2 with Urgency: normal (checks the old BadUrgency report and any delay difference)." },
  { id: "legacy-sw", label: "3. Plain JSON via service worker (3 × 20 s)", count: 3, interval: 20, ttl: 300, urgency: "high", kind: "legacy",
    hint: "Tests the iOS 16.4–18.3 path (service worker must show it). Watch for lost subscriptions." },
  { id: "airplane-long", label: "4. Phone offline, TTL 15 min (3 × 10 s)", count: 3, interval: 10, ttl: 900, urgency: "high", kind: "declarative",
    hint: "Airplane mode ON, then Send. Wait 2 min. Airplane mode OFF. Note when they arrive (Apple should hold them)." },
  { id: "airplane-short", label: "5. Phone offline, TTL 30 s (1)", count: 1, interval: 0, ttl: 30, urgency: "high", kind: "declarative",
    hint: "Airplane mode ON, Send, wait 2 min, airplane OFF. Expected: never arrives (expired). Mark not seen if so." },
  { id: "tailnet-off", label: "6. Tailscale off on phone (3 × 10 s)", count: 3, interval: 10, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Turn the Tailscale VPN off on the iPhone (keep internet). Pushes should still arrive via Apple; receipts queue. Turn Tailscale on, open the app: receipts flush." },
  { id: "network-switch", label: "7. Wi-Fi → cellular (3 × 20 s)", count: 3, interval: 20, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Turn Wi-Fi off on the phone right after Send (cellular on, Tailscale on)." },
  { id: "pc-offline", label: "8. PC offline (1)", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Disconnect this PC from the network, Send (expect an error), reconnect, Send again with scenario 'pc-back'." },
  { id: "pc-back", label: "8b. PC back online (1)", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative",
    hint: "After reconnecting the PC." },
  { id: "tap-open", label: "9. Tap opens app (1)", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Tap the notification. The app should open and 'Opened' fill in." },
  { id: "after-restart", label: "10. After phone restart / next day (1)", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative",
    hint: "Restart the iPhone (or wait a day) and send WITHOUT opening the app first." },
  { id: "manual", label: "Manual", count: 1, interval: 0, ttl: 300, urgency: "high", kind: "declarative", hint: "" },
];

const CHECKLIST = [
  "<b>Tailscale HTTPS:</b> phone opens the tailnet address in Safari with no certificate warning; Funnel shows off.",
  "<b>Install &amp; pair:</b> QR → Add to Home Screen → app opens full-screen → code → Approve. Device shows Home Screen = yes.",
  "<b>Enable notifications:</b> tap the button in the app, allow. Push column shows web.push.apple.com.",
  "<b>Scenarios 1–10</b> on the left, in order. Mark every push seen / not seen.",
  "<b>Revoke:</b> Revoke the device here, open the app: it should say this phone was removed.",
  "Export results JSON when done.",
];

const fmt = (ms) => (Number.isFinite(ms) ? new Date(ms).toLocaleTimeString("en-GB") : "—");
const secs = (ms) => (Number.isFinite(ms) ? `${(ms / 1000).toFixed(1)} s` : "—");
const iosVersion = (ua) => (/OS (\d+)_(\d+)(?:_(\d+))? like Mac OS X/.exec(ua || "") || []).slice(1).filter(Boolean).join(".") || "?";

async function post(path, body) {
  const res = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json", "X-Spike-Admin": "1" }, body: JSON.stringify(body ?? {}) });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

function cell(tr, text, cls) {
  const td = document.createElement("td");
  td.textContent = text;
  if (cls) td.className = cls;
  tr.appendChild(td);
  return td;
}

function button(label, cls, onClick) {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = label;
  if (cls) b.className = cls;
  b.addEventListener("click", onClick);
  return b;
}

let lastCode = null;

function render(s) {
  $("publicUrl").textContent = s.publicUrl ?? "unknown — is Tailscale running?";

  // pairing
  if (s.pairing) {
    $("code").textContent = s.pairing.code;
    const left = Math.max(0, Math.round((s.pairing.expiresAt - s.serverTime) / 1000));
    $("codeInfo").textContent = `Expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")} · ${s.pairing.attemptsLeft} tries left`;
    if (lastCode !== s.pairing.code) {
      $("qr").src = `/admin/qr.svg?c=${s.pairing.code}`;
      lastCode = s.pairing.code;
    }
    $("qr").hidden = !s.publicUrl;
  } else {
    $("code").textContent = "—";
    $("codeInfo").textContent = "No active code.";
    $("qr").hidden = true;
    lastCode = null;
  }
  const reqBox = $("requests");
  reqBox.innerHTML = "";
  for (const r of s.requests) {
    const div = document.createElement("div");
    div.className = "flex";
    div.style.marginTop = "8px";
    const t = document.createElement("span");
    t.innerHTML = `<b></b> wants to pair <span class="muted"></span>`;
    t.querySelector("b").textContent = r.name;
    t.querySelector("span").textContent = `(${r.login})`;
    div.append(t, button("Approve", "", () => post("/admin/pair/decide", { reqId: r.reqId, approve: true }).then(poll)),
      button("Deny", "danger", () => post("/admin/pair/decide", { reqId: r.reqId, approve: false }).then(poll)));
    reqBox.appendChild(div);
  }

  // devices
  const sel = $("sDevice");
  const chosen = sel.value;
  sel.innerHTML = "";
  $("devices").innerHTML = "";
  for (const d of s.devices) {
    const tr = document.createElement("tr");
    cell(tr, d.name);
    cell(tr, d.login);
    cell(tr, iosVersion(d.diag?.userAgent));
    cell(tr, d.diag ? (d.diag.standalone ? "yes" : "no") : "—", d.diag?.standalone ? "ok" : "warn");
    cell(tr, d.diag?.permission ?? "—", d.diag?.permission === "granted" ? "ok" : "warn");
    cell(tr, d.diag ? (d.diag.windowPushManager ? "yes" : "no") : "—");
    cell(tr, d.subscribed ? d.endpointHost : "none", d.subscribed ? "ok" : "warn");
    cell(tr, fmt(d.lastSeen));
    const td = cell(tr, "");
    td.appendChild(button("Revoke", "danger", () => confirm(`Revoke ${d.name}?`) && post("/admin/revoke", { deviceId: d.id }).then(poll)));
    $("devices").appendChild(tr);
    const o = document.createElement("option");
    o.value = d.id;
    o.textContent = `${d.name}${d.subscribed ? "" : " (no push yet)"}`;
    sel.appendChild(o);
  }
  if (chosen) sel.value = chosen;
  if (!s.devices.length) $("devices").innerHTML = `<tr><td colspan="9" class="muted">No paired devices yet.</td></tr>`;

  // summary
  $("summary").innerHTML = "";
  for (const r of s.summary) {
    const tr = document.createElement("tr");
    cell(tr, r.scenario);
    cell(tr, r.sent);
    cell(tr, r.accepted, r.accepted === r.sent ? "ok" : "bad");
    cell(tr, r.receipts);
    cell(tr, r.queuedReceipts);
    cell(tr, r.seen, "ok");
    cell(tr, r.notSeen, r.notSeen ? "bad" : "");
    cell(tr, r.opened);
    cell(tr, `${secs(r.pcDelayMs.p50)} / ${secs(r.pcDelayMs.p95)} / ${secs(r.pcDelayMs.max)}`);
    cell(tr, `${secs(r.phoneDelayMs.p50)} / ${secs(r.phoneDelayMs.p95)}`);
    $("summary").appendChild(tr);
  }

  // sends
  $("sends").innerHTML = "";
  for (const x of s.sends.slice().reverse()) {
    const tr = document.createElement("tr");
    cell(tr, x.seq);
    cell(tr, x.scenario);
    cell(tr, x.kind);
    cell(tr, x.ttl);
    cell(tr, x.urgency);
    cell(tr, fmt(x.sentAt));
    cell(tr, x.error ? `error: ${x.error}` : x.status === null ? "sending…" : `${x.status}${x.reason ? ` ${x.reason}` : ""}`, x.status === 201 ? "ok" : x.status || x.error ? "bad" : "");
    cell(tr, x.delivered ? (x.via === "flush" ? `queued → ${fmt(x.flushedAt)}` : fmt(x.receiptAt)) : "—", x.delivered ? "ok" : "");
    cell(tr, Number.isFinite(x.receiptAt) ? secs(x.receiptAt - x.sentAt) : x.via === "flush" && Number.isFinite(x.phoneRecvAt) ? `${secs(x.phoneRecvAt - x.sentAt)} (phone)` : "—");
    cell(tr, fmt(x.openedAt));
    const td = cell(tr, "");
    const mark = (v) => () => post("/admin/seen", { seq: x.seq, seen: v }).then(poll);
    const yes = button("✓ seen", x.seen === true ? "" : "ghost", mark(true));
    const no = button("✗ not seen", x.seen === false ? "danger" : "ghost", mark(false));
    td.append(yes, " ", no);
    $("sends").appendChild(tr);
  }

  $("notes").textContent = s.notes.map((n) => `${new Date(n.at).toLocaleString("en-GB")}  ${n.text}`).join("\n") || "No notes yet.";
}

async function poll() {
  try {
    const res = await fetch("/admin/state", { cache: "no-store" });
    render(await res.json());
  } catch (e) {
    $("sendMsg").textContent = `Lost the local server: ${e.message}`;
  }
}

function applyScenario() {
  const sc = SCENARIOS.find((x) => x.id === $("sScenario").value);
  $("sCount").value = sc.count;
  $("sInterval").value = sc.interval;
  $("sTtl").value = sc.ttl;
  $("sUrgency").value = sc.urgency;
  $("sKind").value = sc.kind;
  $("sHint").textContent = sc.hint;
}

function init() {
  for (const sc of SCENARIOS) {
    const o = document.createElement("option");
    o.value = sc.id;
    o.textContent = sc.label;
    $("sScenario").appendChild(o);
  }
  $("checklist").innerHTML = CHECKLIST.map((c) => `<li>${c}</li>`).join("");
  $("sScenario").addEventListener("change", applyScenario);
  applyScenario();

  $("newCode").addEventListener("click", () => post("/admin/pair/new").then(poll));
  $("sendBtn").addEventListener("click", async () => {
    $("sendBtn").disabled = true;
    try {
      const r = await post("/admin/send", {
        deviceId: $("sDevice").value,
        scenario: $("sScenario").value,
        count: Number($("sCount").value),
        intervalSec: Number($("sInterval").value),
        ttl: Number($("sTtl").value),
        urgency: $("sUrgency").value,
        kind: $("sKind").value,
        topic: $("sTopic").value.trim() || undefined,
      });
      $("sendMsg").textContent = `Queued ${r.queued} at ${new Date().toLocaleTimeString("en-GB")}.`;
    } catch (e) {
      $("sendMsg").textContent = e.message;
    } finally {
      $("sendBtn").disabled = false;
      poll();
    }
  });
  $("noteBtn").addEventListener("click", async () => {
    const text = $("note").value.trim();
    if (!text) return;
    await post("/admin/note", { text });
    $("note").value = "";
    poll();
  });
  $("tsBtn").addEventListener("click", async () => {
    $("tsStatus").textContent = "Checking…";
    const res = await fetch("/admin/tailscale", { cache: "no-store" });
    const r = await res.json();
    $("tsStatus").textContent = `serve:\n${r.serve}\n\nfunnel:\n${r.funnel}`;
  });
  poll();
  setInterval(poll, 2000);
}

init();
