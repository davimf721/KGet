// KGet background: context menus, optional download interception, and the
// connection to the native host (`kget native-host`), shared with the popup.
//
// Chrome runs this file as a service worker (lib.js is imported below);
// Firefox loads lib.js and this file as background scripts.
"use strict";

if (typeof KGetLib === "undefined" && typeof importScripts === "function") {
  importScripts("lib.js");
}

const api = globalThis.browser || globalThis.chrome;
const L = globalThis.KGetLib;

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

async function getSettings() {
  const stored = await api.storage.local.get(Object.keys(L.DEFAULT_SETTINGS));
  return Object.assign({}, L.DEFAULT_SETTINGS, stored);
}

// ---------------------------------------------------------------------------
// Native host connection
// ---------------------------------------------------------------------------

const REQUEST_TIMEOUT_MS = 30000;
const IDLE_DISCONNECT_MS = 30000;

let port = null;
let nextId = 1;
const pending = new Map();
let jobs = {};
// {state: "idle"|"connected"|"not_installed"|"forbidden"|"error", message}
let status = { state: "idle", message: "" };
const popups = new Set();
let idleTimer = null;

function broadcast() {
  const msg = { type: "state", status: status, jobs: Object.values(jobs) };
  for (const p of popups) {
    try {
      p.postMessage(msg);
    } catch (_) {
      popups.delete(p);
    }
  }
  updateBadge();
  scheduleIdleDisconnect();
}

function updateBadge() {
  if (!api.action || !api.action.setBadgeText) return;
  let text = "";
  if (status.state === "not_installed" || status.state === "forbidden") {
    text = "!";
  } else {
    const n = Object.values(jobs).filter((j) => j.state === "running" || j.state === "queued").length;
    text = n ? String(n) : "";
  }
  api.action.setBadgeText({ text: text });
}

function classifyDisconnect(message) {
  const m = String(message || "");
  if (/not found|no such native application/i.test(m)) {
    return { state: "not_installed", message: L.INSTALL_HINT };
  }
  if (/forbidden/i.test(m)) {
    return {
      state: "forbidden",
      message:
        "The KGet native host does not allow this extension. Run:\n  kget native-host install --extension-id " +
        api.runtime.id,
    };
  }
  return { state: "error", message: m || "The KGet native host exited." };
}

function connect() {
  if (port) return port;
  try {
    port = api.runtime.connectNative(L.HOST_NAME);
  } catch (e) {
    status = classifyDisconnect(e && e.message);
    broadcast();
    throw e;
  }
  status = { state: "connected", message: "" };
  port.onMessage.addListener(onHostMessage);
  port.onDisconnect.addListener((p) => {
    const err = (p && p.error) || api.runtime.lastError;
    status = classifyDisconnect(err && err.message);
    port = null;
    for (const [, req] of pending) req.reject(new Error(status.message));
    pending.clear();
    broadcast();
  });
  // Seed the job list and follow changes.
  request({ type: "subscribe" }).catch(() => {});
  request({ type: "list" })
    .then((r) => {
      jobs = {};
      for (const j of r.jobs || []) jobs[j.id] = j;
      broadcast();
    })
    .catch(() => {});
  return port;
}

function disconnect() {
  if (port) {
    port.disconnect();
    port = null;
  }
  status = { state: "idle", message: "" };
}

function scheduleIdleDisconnect() {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = null;
  const busy = popups.size > 0 || Object.values(jobs).some((j) => L.isActive(j.state));
  if (port && !busy) {
    idleTimer = setTimeout(disconnect, IDLE_DISCONNECT_MS);
  }
}

function onHostMessage(msg) {
  if (msg.type === "event") {
    L.applyEvent(jobs, msg.event);
    broadcast();
    return;
  }
  if (msg.type === "event_stream_closed") {
    request({ type: "subscribe" }).catch(() => {});
    return;
  }
  const req = pending.get(msg.id);
  if (!req) return;
  pending.delete(msg.id);
  clearTimeout(req.timer);
  if (msg.ok) req.resolve(msg.result);
  else req.reject(new Error((msg.error && msg.error.message) || "KGet error"));
}

function request(msg) {
  return new Promise((resolve, reject) => {
    let p;
    try {
      p = connect();
    } catch (e) {
      reject(new Error(status.message || String(e)));
      return;
    }
    const id = nextId++;
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("KGet did not answer in time"));
    }, REQUEST_TIMEOUT_MS);
    pending.set(id, { resolve, reject, timer });
    p.postMessage(Object.assign({}, msg, { id: id }));
  });
}

// ---------------------------------------------------------------------------
// Sending downloads
// ---------------------------------------------------------------------------

async function cookiesFor(url) {
  if (!api.cookies) return "";
  try {
    return L.cookieHeader(await api.cookies.getAll({ url: url }));
  } catch (_) {
    return "";
  }
}

async function sendToKGet(url, opts) {
  if (!L.isSupportedUrl(url)) throw new Error("KGet cannot download this kind of link");
  const settings = await getSettings();
  const o = Object.assign({}, opts);
  if (settings.sendCookies && !url.startsWith("magnet:")) o.cookies = await cookiesFor(url);
  const job = await request(L.buildDownloadMessage(url, o, settings));
  if (job && job.id != null) jobs[job.id] = job;
  broadcast();
  return job;
}

// ---------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------

function createMenus() {
  api.contextMenus.removeAll(() => {
    const title = "Download with KGet";
    api.contextMenus.create({ id: "kget-link", title: title, contexts: ["link"] });
    api.contextMenus.create({ id: "kget-media", title: title, contexts: ["image", "video", "audio"] });
    api.contextMenus.create({ id: "kget-page", title: "Download page with KGet", contexts: ["page"] });
  });
}

api.runtime.onInstalled.addListener(createMenus);
if (api.runtime.onStartup) api.runtime.onStartup.addListener(createMenus);

api.contextMenus.onClicked.addListener((info, tab) => {
  const url = info.linkUrl || info.srcUrl || info.pageUrl || (tab && tab.url);
  const referrer = info.pageUrl || (tab && tab.url) || "";
  sendToKGet(url, { referrer: referrer }).catch((e) => {
    status = Object.assign({}, status, { lastError: e.message });
    broadcast();
    if (api.action && api.action.setBadgeText) api.action.setBadgeText({ text: "!" });
  });
});

// ---------------------------------------------------------------------------
// Download interception (optional "downloads" permission, off by default)
// ---------------------------------------------------------------------------

// URLs given back to the browser after KGet failed; not intercepted again.
const bypass = new Set();
let interceptRegistered = false;

async function onDownloadCreated(item) {
  const url = item.finalUrl || item.url;
  if (bypass.has(url)) {
    bypass.delete(url);
    return;
  }
  const settings = await getSettings();
  if (!L.shouldIntercept(item, settings).intercept) return;
  try {
    await api.downloads.cancel(item.id);
    await api.downloads.erase({ id: item.id });
  } catch (_) {
    return; // already finished or gone: leave it to the browser
  }
  try {
    await sendToKGet(url, { filename: L.basename(item.filename), referrer: item.referrer || "" });
  } catch (e) {
    // KGet unavailable: give the download back to the browser.
    bypass.add(url);
    api.downloads.download({ url: url });
  }
}

function registerIntercept() {
  if (interceptRegistered || !api.downloads || !api.downloads.onCreated) return;
  api.downloads.onCreated.addListener(onDownloadCreated);
  interceptRegistered = true;
}

registerIntercept();
if (api.permissions && api.permissions.onAdded) {
  api.permissions.onAdded.addListener(registerIntercept);
}

// ---------------------------------------------------------------------------
// Video sniffer & media tracking
// ---------------------------------------------------------------------------

const detectedMedia = new Map();

api.runtime.onMessage.addListener((msg, sender) => {
  if (msg.type === "media_detected" && sender.tab && sender.tab.id) {
    const tabId = sender.tab.id;
    let list = detectedMedia.get(tabId);
    if (!list) {
      list = [];
      detectedMedia.set(tabId, list);
    }
    if (!list.some((m) => m.url === msg.media.url)) {
      list.push(msg.media);
    }
  }
});

if (api.tabs && api.tabs.onRemoved) {
  api.tabs.onRemoved.addListener((tabId) => {
    detectedMedia.delete(tabId);
  });
}

// ---------------------------------------------------------------------------
// Popup / options bridge
// ---------------------------------------------------------------------------

const POPUP_ACTIONS = ["pause", "resume", "cancel", "remove", "open", "reveal"];

api.runtime.onConnect.addListener((p) => {
  if (p.name !== "popup") return;
  popups.add(p);
  p.onDisconnect.addListener(() => {
    popups.delete(p);
    scheduleIdleDisconnect();
  });
  p.onMessage.addListener(async (msg) => {
    let reply;
    try {
      let result;
      if (msg.type === "add") {
        result = await sendToKGet(String(msg.url || "").trim(), {});
      } else if (msg.type === "get_detected_media") {
        const tabs = await api.tabs.query({ active: true, currentWindow: true });
        const activeTabId = tabs && tabs[0] ? tabs[0].id : null;
        const media = activeTabId ? (detectedMedia.get(activeTabId) || []) : [];
        result = { media: media };
      } else if (msg.type === "retry") {
        disconnect();
        connect();
        result = {};
      } else if (POPUP_ACTIONS.includes(msg.type)) {
        result = await request({ type: msg.type, job: msg.job });
      } else {
        throw new Error("Unknown action " + msg.type);
      }
      reply = { type: "result", request: msg.type, ok: true, result: result };
    } catch (e) {
      reply = { type: "result", request: msg.type, ok: false, error: e.message };
    }
    try {
      p.postMessage(reply);
    } catch (_) {
      /* popup closed */
    }
  });
  try {
    connect();
  } catch (_) {
    /* status already set */
  }
  broadcast();
});
