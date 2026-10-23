// Unit tests for lib.js: `node --test browser-extension/tests/`
// (no npm dependencies).
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const L = require("../lib.js");

test("parseList normalizes separators, case and leading dots", () => {
  assert.deepEqual(L.parseList(" ZIP, .iso;*.tar  gz,"), ["zip", "iso", "tar", "gz"]);
  assert.deepEqual(L.parseList(""), []);
});

test("basename and fileExtension", () => {
  assert.equal(L.basename("/Users/me/Downloads/a.zip"), "a.zip");
  assert.equal(L.basename("C:\\Users\\me\\b.iso"), "b.iso");
  assert.equal(L.fileExtension("https://x.org/dl/file.TAR.GZ?token=1"), "gz");
  assert.equal(L.fileExtension("README"), "");
  assert.equal(L.fileExtension(".bashrc"), "");
});

test("hostMatches includes subdomains only", () => {
  assert.ok(L.hostMatches("example.com", "example.com"));
  assert.ok(L.hostMatches("dl.example.com", "example.com"));
  assert.ok(!L.hostMatches("badexample.com", "example.com"));
  assert.ok(!L.hostMatches("example.com", ""));
});

test("isSupportedUrl allows only download schemes", () => {
  assert.ok(L.isSupportedUrl("https://example.com/a"));
  assert.ok(L.isSupportedUrl("magnet:?xt=urn:btih:abc"));
  assert.ok(!L.isSupportedUrl("file:///etc/passwd"));
  assert.ok(!L.isSupportedUrl("javascript:alert(1)"));
  assert.ok(!L.isSupportedUrl("blob:https://example.com/x"));
  assert.ok(!L.isSupportedUrl("not a url"));
});

test("shouldIntercept honours the toggle and the filters", () => {
  const item = { url: "https://dl.example.com/big.iso", filename: "", totalBytes: 50 * 1024 * 1024 };
  assert.equal(L.shouldIntercept(item, {}).intercept, false, "off by default");
  const on = { intercept: true };
  assert.equal(L.shouldIntercept(item, on).intercept, true);
  assert.equal(L.shouldIntercept({ ...item, url: "https://x.org/page.html" }, on).reason, "extension");
  assert.equal(L.shouldIntercept({ ...item, url: "blob:https://x.org/1" }, on).reason, "scheme");
  assert.equal(L.shouldIntercept(item, { ...on, interceptMinSizeMb: 100 }).reason, "size");
  assert.equal(
    L.shouldIntercept({ ...item, totalBytes: -1 }, { ...on, interceptMinSizeMb: 100 }).intercept,
    true,
    "unknown size passes"
  );
  assert.equal(L.shouldIntercept(item, { ...on, excludeDomains: "example.com" }).reason, "excluded domain");
  assert.equal(L.shouldIntercept(item, { ...on, interceptDomains: "other.org" }).reason, "domain");
  assert.equal(L.shouldIntercept(item, { ...on, interceptDomains: "example.com" }).intercept, true);
  // The browser's target file name decides when the URL has no extension
  const named = { url: "https://x.org/download?id=3", filename: "/home/u/Downloads/setup.exe" };
  assert.equal(L.shouldIntercept(named, on).intercept, true);
  assert.equal(L.shouldIntercept(named, { ...on, interceptExtensions: "" }).intercept, true);
});

test("buildDownloadMessage sends a file name only and cookies only on opt-in", () => {
  const opts = { filename: "/Users/me/Downloads/a.zip", referrer: "https://example.com/p", cookies: "sid=1" };
  const off = L.buildDownloadMessage("https://example.com/a.zip", opts, {});
  assert.deepEqual(off, {
    type: "download",
    url: "https://example.com/a.zip",
    turbo: false,
    filename: "a.zip",
    referrer: "https://example.com/p",
  });
  const on = L.buildDownloadMessage("https://example.com/a.zip", opts, { sendCookies: true, turbo: true });
  assert.equal(on.send_cookies, true);
  assert.equal(on.cookies, "sid=1");
  assert.equal(on.turbo, true);
  const noRef = L.buildDownloadMessage("https://e.com/a", { referrer: "chrome://newtab" }, {});
  assert.equal(noRef.referrer, undefined);
});

test("cookieHeader joins name=value pairs", () => {
  assert.equal(L.cookieHeader([{ name: "a", value: "1" }, { name: "b", value: "2" }]), "a=1; b=2");
  assert.equal(L.cookieHeader([]), "");
});

test("applyEvent tracks state, progress, saved path and removal", () => {
  const jobs = {};
  L.applyEvent(jobs, { type: "state", id: 1, state: "running", error: null });
  L.applyEvent(jobs, {
    type: "download",
    id: 1,
    event: { event: "progress", bytes: 10, total: 100, percent: 10, speed_bps: 5, eta_secs: 18 },
  });
  assert.equal(jobs[1].state, "running");
  assert.equal(jobs[1].progress.percent, 10);
  L.applyEvent(jobs, { type: "download", id: 1, event: { event: "saved", path: "/d/a.zip", bytes: 100 } });
  assert.equal(L.jobName(jobs[1]), "a.zip");
  L.applyEvent(jobs, { type: "removed", id: 1 });
  assert.deepEqual(jobs, {});
});

test("formatting helpers", () => {
  assert.equal(L.formatBytes(512), "512 B");
  assert.equal(L.formatBytes(1536), "1.5 KB");
  assert.equal(L.formatEta(75), "1m 15s");
  assert.equal(L.formatEta(null), "");
});

test("manifest.json is a cross-browser MV3 manifest", () => {
  const dir = path.join(__dirname, "..");
  const m = JSON.parse(fs.readFileSync(path.join(dir, "manifest.json"), "utf8"));
  assert.equal(m.manifest_version, 3);
  assert.deepEqual(m.permissions.sort(), ["contextMenus", "nativeMessaging", "storage"]);
  assert.deepEqual(m.optional_permissions.sort(), ["cookies", "downloads"]);
  assert.equal(m.background.service_worker, "background.js");
  assert.deepEqual(m.background.scripts, ["lib.js", "background.js"]);
  assert.equal(m.browser_specific_settings.gecko.id, "kget@davimf721.github.io");
  for (const f of [m.action.default_popup, m.options_ui.page, ...Object.values(m.icons)]) {
    assert.ok(fs.existsSync(path.join(dir, f)), f);
  }
});
