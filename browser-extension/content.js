// KGet content script: inspects the DOM for media elements (<video>, <audio>, <source>)
// and notifies background.js to populate the Video Grabber panel.
"use strict";

(function () {
  const api = globalThis.browser || globalThis.chrome;
  if (!api || !api.runtime) return;

  const reportedUrls = new Set();

  function reportMedia(url, type) {
    if (!url || typeof url !== "string") return;
    const cleanUrl = url.trim();
    if (!cleanUrl || cleanUrl.startsWith("blob:") || cleanUrl.startsWith("data:")) return;
    if (reportedUrls.has(cleanUrl)) return;
    reportedUrls.add(cleanUrl);

    api.runtime.sendMessage({
      type: "media_detected",
      media: {
        url: cleanUrl,
        title: document.title || "Video Stream",
        mediaType: type || "video",
        pageUrl: window.location.href,
      },
    }).catch(() => {});
  }

  function scanElement(el) {
    if (!el) return;
    if (el.src) reportMedia(el.src, el.tagName.toLowerCase());
    if (el.currentSrc) reportMedia(el.currentSrc, el.tagName.toLowerCase());

    const sources = el.querySelectorAll("source");
    for (const s of sources) {
      if (s.src) reportMedia(s.src, s.type || "video");
    }
  }

  function scanPage() {
    const mediaElements = document.querySelectorAll("video, audio");
    for (const el of mediaElements) {
      scanElement(el);
    }
  }

  // Scan immediately and after DOM content loads
  scanPage();
  window.addEventListener("DOMContentLoaded", scanPage);
  window.addEventListener("load", scanPage);

  // Monitor dynamic video player insertions and src changes
  const observer = new MutationObserver((mutations) => {
    for (const m of mutations) {
      if (m.type === "childList") {
        for (const node of m.addedNodes) {
          if (node.nodeType === Node.ELEMENT_NODE) {
            if (node.tagName === "VIDEO" || node.tagName === "AUDIO") {
              scanElement(node);
            } else if (node.querySelectorAll) {
              const inside = node.querySelectorAll("video, audio");
              for (const el of inside) scanElement(el);
            }
          }
        }
      } else if (m.type === "attributes" && (m.attributeName === "src" || m.attributeName === "currentSrc")) {
        scanElement(m.target);
      }
    }
  });

  observer.observe(document.documentElement || document.body, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["src", "currentSrc"],
  });

  // Listen for media play events (often sets src just in time)
  document.addEventListener(
    "play",
    (e) => {
      if (e.target && (e.target.tagName === "VIDEO" || e.target.tagName === "AUDIO")) {
        scanElement(e.target);
      }
    },
    true
  );
})();
