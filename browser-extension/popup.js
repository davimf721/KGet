"use strict";

const api = globalThis.browser || globalThis.chrome;
const L = globalThis.KGetLib;
const $ = (id) => document.getElementById(id);

const port = api.runtime.connect({ name: "popup" });
let jobs = [];

port.onMessage.addListener((msg) => {
  if (msg.type === "state") {
    renderStatus(msg.status);
    jobs = msg.jobs || [];
    renderJobs();
  } else if (msg.type === "result" && !msg.ok) {
    showError(msg.error);
  } else if (msg.type === "result" && msg.request === "add") {
    $("add-url").value = "";
  }
});

function showError(text) {
  const el = $("error");
  el.textContent = text || "";
  el.hidden = !text;
}

function renderStatus(status) {
  const blocked = status.state === "not_installed" || status.state === "forbidden";
  $("not-installed").hidden = !blocked;
  $("add-form").hidden = blocked;
  $("hint").textContent = blocked ? status.message : "";
  if (status.state === "error") showError(status.message);
  else if (status.lastError) showError(status.lastError);
}

function button(label, action, job) {
  const b = document.createElement("button");
  b.textContent = label;
  b.className = "small";
  b.addEventListener("click", () => {
    showError("");
    port.postMessage({ type: action, job: job.id });
  });
  return b;
}

function renderJobs() {
  const list = $("jobs");
  list.textContent = "";
  const sorted = jobs.slice().sort((a, b) => b.id - a.id);
  $("empty").hidden = sorted.length > 0;
  for (const job of sorted) {
    const li = document.createElement("li");
    li.className = "job state-" + job.state;

    const name = document.createElement("div");
    name.className = "name";
    name.textContent = L.jobName(job);
    name.title = (job.job && job.job.url) || "";
    li.appendChild(name);

    const p = job.progress || {};
    const bar = document.createElement("progress");
    bar.max = 100;
    if (p.percent != null) bar.value = p.percent;
    else if (job.state === "completed") bar.value = 100;
    else bar.removeAttribute("value");
    if (L.isActive(job.state) || job.state === "completed") li.appendChild(bar);

    const meta = document.createElement("div");
    meta.className = "meta";
    const parts = [job.state];
    if (p.bytes) parts.push(L.formatBytes(p.bytes) + (p.total ? " / " + L.formatBytes(p.total) : ""));
    if (job.state === "running" && p.speed_bps) parts.push(L.formatBytes(p.speed_bps) + "/s");
    if (job.state === "running" && p.eta_secs != null) parts.push(L.formatEta(p.eta_secs) + " left");
    if (job.error) parts.push(job.error);
    meta.textContent = parts.join(" · ");
    li.appendChild(meta);

    const actions = document.createElement("div");
    actions.className = "actions";
    if (job.state === "running" || job.state === "queued") actions.appendChild(button("Pause", "pause", job));
    if (job.state === "paused" || job.state === "cancelled" || job.state === "failed") {
      actions.appendChild(button("Resume", "resume", job));
    }
    if (L.isActive(job.state)) actions.appendChild(button("Cancel", "cancel", job));
    if (job.state === "completed") actions.appendChild(button("Open", "open", job));
    if (job.path || (job.job && job.job.output)) actions.appendChild(button("Show in folder", "reveal", job));
    if (!L.isActive(job.state)) actions.appendChild(button("Remove", "remove", job));
    li.appendChild(actions);

    list.appendChild(li);
  }
}

$("add-form").addEventListener("submit", (e) => {
  e.preventDefault();
  showError("");
  port.postMessage({ type: "add", url: $("add-url").value });
});

$("retry").addEventListener("click", () => port.postMessage({ type: "retry" }));

$("clear").addEventListener("click", () => {
  for (const job of jobs) {
    if (!L.isActive(job.state)) port.postMessage({ type: "remove", job: job.id });
  }
});

$("open-options").addEventListener("click", (e) => {
  e.preventDefault();
  api.runtime.openOptionsPage();
});
