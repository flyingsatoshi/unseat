"use strict";
// The per-run key comes in the URL fragment (never sent to a server or logged) and is kept
// only in memory.

const $ = (id) => document.getElementById(id);
const TOKEN = new URLSearchParams(location.hash.slice(1)).get("t") || "";
history.replaceState(null, "", "/");

const fmt = (ms) => (ms ? new Date(ms).toLocaleString() : "—");

async function call(path, body) {
  const res = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    headers: { "X-Unseat-Admin": TOKEN, ...(body === undefined ? {} : { "Content-Type": "application/json" }) },
    body: body === undefined ? undefined : JSON.stringify(body),
    cache: "no-store",
  });
  const data = await res.json().catch(() => ({}));
  if (res.status === 403) {
    $("fatal").style.display = "block";
    $("app").style.display = "none";
  }
  if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
  return data;
}

function el(tag, text, cls) {
  const e = document.createElement(tag);
  if (text !== undefined) e.textContent = text;
  if (cls) e.className = cls;
  return e;
}

function button(label, cls, onClick) {
  const b = el("button", label, cls);
  b.type = "button";
  b.addEventListener("click", onClick);
  return b;
}

let lastCode = null;
let busy = false;

function render(s) {
  if (document.activeElement !== $("enabled")) $("enabled").checked = s.enabled;
  if (document.activeElement !== $("alerts")) $("alerts").value = s.alerts;
  $("pending").textContent = s.pending ? `${s.pending} reminder(s) waiting to be delivered.` : "";
  $("listenerError").hidden = !s.listenerError;
  $("listenerError").textContent = s.listenerError || "";
  $("phoneUrl").textContent = s.phoneUrl || "unknown — is Tailscale running and signed in?";
  $("serveCmd").textContent = s.serveCommand;
  $("tsStatus").textContent = `serve:\n${s.serveStatus || "unknown"}\n\nfunnel:\n${s.funnelStatus || "unknown"}`;

  if (s.pairing) {
    $("code").textContent = s.pairing.code;
    const left = Math.max(0, Math.round((s.pairing.expiresMs - s.nowMs) / 1000));
    $("codeInfo").textContent = `Expires in ${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")} · ${s.pairing.attemptsLeft} tries left`;
    if (lastCode !== s.pairing.code) {
      $("qr").src = `/admin/qr.svg?t=${encodeURIComponent(TOKEN)}&c=${s.pairing.code}`;
      lastCode = s.pairing.code;
    }
    $("qr").hidden = !s.phoneUrl;
  } else {
    $("code").textContent = "—";
    $("codeInfo").textContent = "No active code.";
    $("qr").hidden = true;
    lastCode = null;
  }

  const req = $("requests");
  req.replaceChildren();
  for (const r of s.requests) {
    const row = el("div", undefined, "flex");
    row.style.marginTop = "8px";
    const who = el("span");
    who.append(el("b", r.name), ` wants to pair (${r.login}) `);
    row.append(who,
      button("Approve", "", () => act("/admin/pair/decide", { reqId: r.reqId, approve: true })),
      button("Decline", "danger", () => act("/admin/pair/decide", { reqId: r.reqId, approve: false })));
    req.appendChild(row);
  }

  const tbody = $("devices");
  tbody.replaceChildren();
  for (const d of s.devices) {
    const tr = el("tr");
    const state = d.needsResubscribe ? ["needs reconnect (open the app)", "warn"] : d.subscribed ? ["on", "ok"] : ["not turned on yet", "warn"];
    tr.append(el("td", d.name), el("td", d.login), el("td", state[0], state[1]), el("td", String(d.delivered)), el("td", d.received ? String(d.received) : "not reported", d.received ? "" : "muted"), el("td", String(d.missed), d.missed ? "warn" : ""),
      el("td", fmt(d.lastSeenMs)), el("td", fmt(d.lastDeliveredMs)));
    const actions = el("td");
    actions.append(button("Send test", "ghost", () => act("/admin/test", { deviceId: d.id }, "Test sent.")), " ",
      button("Remove", "danger", () => confirm(`Remove ${d.name}? It will stop getting reminders.`) && act("/admin/revoke", { deviceId: d.id })));
    tr.appendChild(actions);
    tbody.appendChild(tr);
  }
  if (!s.devices.length) {
    const tr = el("tr");
    const td = el("td", "No phones paired yet.", "muted");
    td.colSpan = 9;
    tr.appendChild(td);
    tbody.appendChild(tr);
  }
}

async function act(path, body, done) {
  try {
    await call(path, body);
    $("msg").textContent = done || "";
  } catch (e) {
    $("msg").textContent = e.message;
  }
  poll();
}

async function poll() {
  if (busy) return;
  busy = true;
  try {
    render(await call("/admin/state"));
  } catch (e) {
    $("msg").textContent = e.message;
  } finally {
    busy = false;
  }
}

function saveSettings() {
  act("/admin/settings", { enabled: $("enabled").checked, alerts: $("alerts").value });
}

if (!TOKEN) {
  $("fatal").style.display = "block";
  $("app").style.display = "none";
} else {
  $("enabled").addEventListener("change", saveSettings);
  $("alerts").addEventListener("change", saveSettings);
  $("newCode").addEventListener("click", () => act("/admin/pair/new", {}));
  poll();
  setInterval(poll, 2000);
}
