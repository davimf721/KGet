"use strict";

const api = globalThis.browser || globalThis.chrome;
const L = globalThis.KGetLib;
const $ = (id) => document.getElementById(id);

const CHECKBOXES = ["turbo", "intercept", "sendCookies"];
const FIELDS = ["interceptExtensions", "interceptMinSizeMb", "interceptDomains", "excludeDomains"];

// Optional permissions each toggle needs.
const PERMISSIONS = {
  intercept: { permissions: ["downloads"] },
  sendCookies: { permissions: ["cookies"], origins: ["<all_urls>"] },
};

let savedTimer = null;

function flashSaved() {
  $("saved").hidden = false;
  clearTimeout(savedTimer);
  savedTimer = setTimeout(() => ($("saved").hidden = true), 1200);
}

function showError(text) {
  $("error").textContent = text || "";
  $("error").hidden = !text;
}

async function save(values) {
  await api.storage.local.set(values);
  flashSaved();
}

async function load() {
  const s = Object.assign(
    {},
    L.DEFAULT_SETTINGS,
    await api.storage.local.get(Object.keys(L.DEFAULT_SETTINGS))
  );
  // A permission removed in the browser's settings turns its toggle off.
  for (const key of Object.keys(PERMISSIONS)) {
    if (s[key] && !(await api.permissions.contains(PERMISSIONS[key]))) {
      s[key] = false;
      await api.storage.local.set({ [key]: false });
    }
  }
  for (const id of CHECKBOXES) $(id).checked = !!s[id];
  for (const id of FIELDS) $(id).value = s[id];
}

for (const id of CHECKBOXES) {
  $(id).addEventListener("change", (e) => {
    showError("");
    const box = e.target;
    const perms = PERMISSIONS[id];
    if (!box.checked || !perms) {
      save({ [id]: box.checked });
      return;
    }
    // Ask right away: permission prompts need the click's user gesture.
    api.permissions.request(perms).then(
      (granted) => {
        box.checked = granted;
        save({ [id]: granted });
        if (!granted) showError("Permission not granted; the option stays off.");
      },
      (err) => {
        box.checked = false;
        showError(String((err && err.message) || err));
      }
    );
  });
}

for (const id of FIELDS) {
  $(id).addEventListener("change", (e) => {
    const value = id === "interceptMinSizeMb" ? Math.max(0, Number(e.target.value) || 0) : e.target.value;
    save({ [id]: value });
  });
}

load();
