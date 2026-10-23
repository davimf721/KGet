// Pure helpers shared by the background script, the popup and the options
// page. No browser APIs here, so `node --test` can load this file.
(function (root) {
  "use strict";

  const HOST_NAME = "com.kget.native_host";

  const DEFAULT_SETTINGS = Object.freeze({
    intercept: false,
    // Extensions to intercept (empty = any)
    interceptExtensions:
      "zip, 7z, rar, tar, gz, bz2, xz, zst, iso, img, dmg, pkg, exe, msi, deb, rpm, appimage, apk, mp4, mkv, avi, mov, webm, mp3, flac",
    // Smallest size to intercept in MB (0 = any; unknown sizes pass)
    interceptMinSizeMb: 0,
    // Only intercept on these domains (empty = all), never on the excluded ones
    interceptDomains: "",
    excludeDomains: "",
    sendCookies: false,
    turbo: false,
  });

  const INSTALL_HINT =
    "KGet's native host is not installed. Install KGet, then run:\n  kget native-host install";

  // URL schemes KGet accepts from the browser (the host checks them again).
  const ALLOWED_SCHEMES = ["http:", "https:", "ftp:", "ftps:", "magnet:"];

  const ACTIVE_STATES = ["queued", "running", "verifying", "paused"];

  function isSupportedUrl(url) {
    if (typeof url !== "string" || url === "") return false;
    try {
      return ALLOWED_SCHEMES.includes(new URL(url).protocol);
    } catch (_) {
      return false;
    }
  }

  // "a, .B ,c" -> ["a", "b", "c"]
  function parseList(text) {
    return String(text || "")
      .split(/[\s,;]+/)
      .map((s) => s.trim().toLowerCase().replace(/^\*?\./, ""))
      .filter(Boolean);
  }

  // Last path component of a POSIX or Windows path.
  function basename(path) {
    const parts = String(path || "").split(/[\\/]/);
    return parts[parts.length - 1] || "";
  }

  // Lowercase extension of a file name or URL path ("" if none).
  function fileExtension(nameOrUrl) {
    let name = String(nameOrUrl || "");
    try {
      name = new URL(name).pathname;
    } catch (_) {
      /* a plain file name */
    }
    const base = basename(name);
    const dot = base.lastIndexOf(".");
    return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
  }

  // "example.com" matches example.com and its subdomains.
  function hostMatches(host, pattern) {
    host = String(host || "").toLowerCase();
    pattern = String(pattern || "").toLowerCase();
    return pattern !== "" && (host === pattern || host.endsWith("." + pattern));
  }

  function hostOf(url) {
    try {
      return new URL(url).hostname;
    } catch (_) {
      return "";
    }
  }

  // Whether a browser download (a `downloads.DownloadItem`) should be
  // handed to KGet. Returns {intercept, reason}.
  function shouldIntercept(item, settings) {
    const s = Object.assign({}, DEFAULT_SETTINGS, settings || {});
    if (!s.intercept) return { intercept: false, reason: "disabled" };
    const url = item.finalUrl || item.url || "";
    if (!/^(https?|ftps?):/i.test(url)) return { intercept: false, reason: "scheme" };
    const host = hostOf(url);
    if (parseList(s.excludeDomains).some((d) => hostMatches(host, d))) {
      return { intercept: false, reason: "excluded domain" };
    }
    const only = parseList(s.interceptDomains);
    if (only.length && !only.some((d) => hostMatches(host, d))) {
      return { intercept: false, reason: "domain" };
    }
    const exts = parseList(s.interceptExtensions);
    if (exts.length) {
      const ext = fileExtension(basename(item.filename)) || fileExtension(url);
      if (!exts.includes(ext)) return { intercept: false, reason: "extension" };
    }
    const minBytes = Number(s.interceptMinSizeMb || 0) * 1024 * 1024;
    const size = Number(item.totalBytes > 0 ? item.totalBytes : item.fileSize || 0);
    if (minBytes > 0 && size > 0 && size < minBytes) {
      return { intercept: false, reason: "size" };
    }
    return { intercept: true, reason: "match" };
  }

  // `cookies.Cookie` objects -> "a=1; b=2"
  function cookieHeader(cookies) {
    return (cookies || [])
      .filter((c) => c && c.name)
      .map((c) => c.name + "=" + c.value)
      .join("; ");
  }

  // The `download` message for the native host. `opts`: {filename,
  // referrer, cookies}. Cookies are only included with sendCookies on.
  function buildDownloadMessage(url, opts, settings) {
    const s = Object.assign({}, DEFAULT_SETTINGS, settings || {});
    const o = opts || {};
    const msg = { type: "download", url: url, turbo: !!s.turbo };
    const name = basename(o.filename);
    if (name) msg.filename = name;
    if (o.referrer && /^https?:/i.test(o.referrer)) msg.referrer = o.referrer;
    if (s.sendCookies && o.cookies) {
      msg.send_cookies = true;
      msg.cookies = o.cookies;
    }
    return msg;
  }

  function isActive(state) {
    return ACTIVE_STATES.includes(state);
  }

  // Apply a `job.event` to a map of job snapshots keyed by id. Returns the
  // updated job (or null when it was removed / unknown).
  function applyEvent(jobs, event) {
    if (!event || event.id == null) return null;
    if (event.type === "removed") {
      delete jobs[event.id];
      return null;
    }
    const job = jobs[event.id] || (jobs[event.id] = { id: event.id, state: "queued", progress: {} });
    if (event.type === "state") {
      job.state = event.state;
      job.error = event.error || null;
    } else if (event.type === "download" && event.event) {
      const e = event.event;
      if (e.event === "progress") {
        job.progress = {
          bytes: e.bytes,
          total: e.total,
          percent: e.percent,
          speed_bps: e.speed_bps,
          eta_secs: e.eta_secs,
        };
      } else if (e.event === "saved") {
        job.path = e.path;
      } else if (e.event === "started" && e.output) {
        job.job = Object.assign({}, job.job, { output: e.output });
      }
    }
    return job;
  }

  // Name shown for a job.
  function jobName(job) {
    const out = (job.path || (job.job && job.job.output) || "").replace(/[\\/]+$/, "");
    if (out) return basename(out);
    const url = (job.job && job.job.url) || "";
    return basename(url.split(/[?#]/)[0]) || url || "download #" + job.id;
  }

  function formatBytes(n) {
    n = Number(n || 0);
    const units = ["B", "KB", "MB", "GB", "TB"];
    let i = 0;
    while (n >= 1024 && i < units.length - 1) {
      n /= 1024;
      i++;
    }
    return (i === 0 ? n : n.toFixed(1)) + " " + units[i];
  }

  function formatEta(secs) {
    if (secs == null) return "";
    secs = Math.round(secs);
    if (secs < 60) return secs + "s";
    if (secs < 3600) return Math.floor(secs / 60) + "m " + (secs % 60) + "s";
    return Math.floor(secs / 3600) + "h " + Math.floor((secs % 3600) / 60) + "m";
  }

  const api = {
    HOST_NAME,
    DEFAULT_SETTINGS,
    INSTALL_HINT,
    isSupportedUrl,
    parseList,
    basename,
    fileExtension,
    hostMatches,
    hostOf,
    shouldIntercept,
    cookieHeader,
    buildDownloadMessage,
    isActive,
    applyEvent,
    jobName,
    formatBytes,
    formatEta,
  };

  if (typeof module !== "undefined" && module.exports) {
    module.exports = api;
  } else {
    root.KGetLib = api;
  }
})(typeof self !== "undefined" ? self : this);
