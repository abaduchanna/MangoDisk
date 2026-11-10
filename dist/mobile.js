/* MangoDisk Android Edition — frontend logic (zero-cloud, local only) */
"use strict";

const $ = (id) => document.getElementById(id);
const CATS = {
  temp_logs: { label: "Temp & log files", hint: ".tmp .temp .log .bak .old .crdownload .part …", def: true },
  caches: { label: "Cache contents", hint: "files inside .cache / .thumbnails / .recycle folders", def: true },
  junk_meta: { label: "System junk metadata", hint: "thumbs.db, .ds_store, desktop.ini", def: true },
  empty_dirs: { label: "Empty folders", hint: "directories with nothing inside", def: true },
  large_files: { label: "Large files (50 MB+)", hint: "review carefully — NOT selected by default", def: false },
};

let selection = new Set();
let pollTimer = null;

function invoke(cmd, args) {
  const t = window.__TAURI_INTERNALS__;
  if (!t || typeof t.invoke !== "function") throw new Error("Tauri IPC unavailable");
  return t.invoke(cmd, args || {});
}

function fmt(b) {
  if (!b || b < 0) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0, n = b;
  while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
  return (i === 0 ? n : n.toFixed(n >= 100 ? 0 : n >= 10 ? 1 : 2)) + " " + u[i];
}

function banner(text, ok) {
  const b = $("banner");
  b.textContent = text;
  b.className = "banner " + (ok ? "ok" : "err");
  b.classList.remove("hidden");
  clearTimeout(b._t);
  b._t = setTimeout(() => b.classList.add("hidden"), 8000);
}

/* ---------- permission ---------- */
function hasBridge() { return typeof window.MangoDiskBridge !== "undefined"; }

function refreshPerm() {
  const pill = $("permPill"), card = $("permCard");
  let ok = false;
  if (hasBridge()) {
    try { ok = !!window.MangoDiskBridge.hasAllFilesAccess(); } catch (e) { ok = false; }
  }
  pill.textContent = ok ? "storage access granted" : "storage access missing";
  pill.className = "pill " + (ok ? "on" : "off");
  card.classList.toggle("hidden", ok);
  return ok;
}

/* ---------- overview ---------- */
async function refreshOverview() {
  try {
    const o = await invoke("mobile_storage_overview");
    const pct = o.total_bytes ? Math.round((o.used_bytes / o.total_bytes) * 100) : 0;
    $("barFill").style.width = pct + "%";
    $("stUsed").textContent = fmt(o.used_bytes) + " (" + pct + "%)";
    $("stFree").textContent = fmt(o.free_bytes);
    $("stTotal").textContent = fmt(o.total_bytes);
    $("appCacheSize").textContent = fmt(o.app_cache_bytes);
    if (!o.permission_ok) refreshPerm();
  } catch (e) {
    banner("Backend error: " + e, false);
  }
}

/* ---------- scan ---------- */
async function startScan() {
  if (!(await guardPerm())) return;
  try {
    const started = await invoke("mobile_scan_start");
    if (!started) { banner("A scan is already running…", true); return; }
    selection = new Set();
    $("results").classList.add("hidden");
    $("scanBtn").disabled = true;
    $("scanProg").classList.remove("hidden");
    $("scanHint").textContent = "Walking shared storage — this can take a while on big SD cards.";
    pollTimer = setInterval(pollScan, 700);
  } catch (e) { banner("Scan failed to start: " + e, false); resetScanUi(); }
}

function resetScanUi() {
  $("scanBtn").disabled = false;
  $("scanProg").classList.add("hidden");
}

async function pollScan() {
  try {
    const s = await invoke("mobile_scan_status");
    $("scanCount").textContent = s.scanned.toLocaleString();
    if (!s.running && s.result && s.result.done) {
      clearInterval(pollTimer); pollTimer = null;
      resetScanUi();
      $("scanHint").textContent = "Scan finished — review the list below. Nothing is deleted until you press Clean.";
      renderResults(s.result);
    }
  } catch (e) { clearInterval(pollTimer); pollTimer = null; resetScanUi(); banner("Poll error: " + e, false); }
}

function renderResults(res) {
  const wrap = $("catList");
  wrap.innerHTML = "";
  let any = false;
  for (const [key, meta] of Object.entries(CATS)) {
    const entries = res.categories[key] || [];
    const cnt = res.counts[key] || 0;
    const bytes = res.bytes[key] || 0;
    if (!cnt) continue;
    any = true;
    const card = document.createElement("section");
    card.className = "cat";

    const head = document.createElement("div");
    head.className = "row";
    head.innerHTML =
      '<h3><input type="checkbox" class="catchk" ' + (meta.def ? "checked" : "") + '></h3>' +
      '<span class="size">' + fmt(bytes) + "</span>";
    head.querySelector(".catchk").addEventListener("change", (ev) => {
      card.querySelectorAll(".item input").forEach((c) => {
        c.checked = ev.target.checked;
        togglePath(c.dataset.path, c.checked);
      });
    });
    card.appendChild(head);

    const title = document.createElement("div");
    title.innerHTML =
      '<div class="row"><h2>' + meta.label + "</h2></div>" +
      '<p class="cnt">' + cnt.toLocaleString() + " items · " + meta.hint + "</p>";
    card.appendChild(title);

    const det = document.createElement("details");
    const sum = document.createElement("summary");
    sum.textContent = "Show files (" + Math.min(entries.length, 400) + (cnt > entries.length ? "+ of " + cnt.toLocaleString() : "") + ")";
    det.appendChild(sum);
    entries.forEach((it) => {
      const row = document.createElement("label");
      row.className = "item";
      const chk = document.createElement("input");
      chk.type = "checkbox";
      chk.dataset.path = it.path;
      chk.checked = meta.def;
      if (meta.def) selection.add(it.path);
      chk.addEventListener("change", () => togglePath(it.path, chk.checked));
      const p = document.createElement("span");
      p.className = "p";
      p.textContent = it.path.replace(/^\/storage\/emulated\/0\//, "");
      const s = document.createElement("span");
      s.className = "s";
      s.textContent = fmt(it.size);
      row.appendChild(chk); row.appendChild(p); row.appendChild(s);
      det.appendChild(row);
    });
    card.appendChild(det);
    wrap.appendChild(card);
  }

  if (res.stopped_early) banner("Scan hit the file-count cap — results may be partial. Run again after cleaning.", false);
  else if (!any) banner("Nothing reclaimable found — your storage is already clean. 🥭", true);
  else {
    $("totalFree").textContent = fmt(res.total_reclaimable);
    $("scanMeta").textContent =
      res.scanned_files.toLocaleString() + " files scanned" +
      (res.truncated ? " · list capped at " + 400 + " items per category" : "");
    $("results").classList.remove("hidden");
  }
}

function togglePath(p, on) {
  if (on) selection.add(p); else selection.delete(p);
  $("cleanBtn").textContent = "Clean Selected Items (" + selection.size + ")";
}

/* ---------- clean ---------- */
async function guardPerm() {
  if (refreshPerm()) return true;
  banner("Grant All-Files Access first.", false);
  return false;
}

async function cleanSelected() {
  if (!(await guardPerm())) return;
  const paths = [...selection];
  if (!paths.length) { banner("Nothing selected.", false); return; }
  if (!confirm("Delete " + paths.length + " selected items from storage?\nThis cannot be undone.")) return;
  $("cleanBtn").disabled = true;
  try {
    const r = await invoke("mobile_clean_files", { paths });
    selection = new Set();
    if (r.deleted) logHistory("clean", "Junk cleaned", r.deleted + " items", r.freed_bytes);
    banner("Deleted " + r.deleted + " items · freed " + fmt(r.freed_bytes) + (r.failed.length ? " · " + r.failed.length + " failed" : ""), r.failed.length === 0);
    $("results").classList.add("hidden");
    $("scanHint").textContent = "Press Start Deep Scan again to see the updated numbers.";
    refreshOverview();
  } catch (e) { banner("Clean failed: " + e, false); }
  $("cleanBtn").disabled = false;
  $("cleanBtn").textContent = "Clean Selected Items";
}

async function cleanAppCache() {
  try {
    const r = await invoke("mobile_clean_app_cache");
    if (r.ok && r.freed_bytes) logHistory("clean", "MangoDisk cache cleared", "", r.freed_bytes);
    banner(r.ok ? "App cache cleared · freed " + fmt(r.freed_bytes) : "Could not clear cache.", r.ok);
    refreshOverview();
  } catch (e) { banner("App cache clean failed: " + e, false); }
}

/* ---------- wiring ---------- */
$("grantBtn").addEventListener("click", () => {
  try { window.MangoDiskBridge.requestAllFilesAccess(); }
  catch (e) { banner("Bridge error: " + e, false); }
  // Android 7-10 shows an in-app dialog (no visibility change) - poll until granted
  let n = 0; const t = setInterval(() => { if (refreshPerm() || ++n > 60) { clearInterval(t); refreshOverview(); } }, 1000);
});
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) { refreshPerm(); refreshOverview(); }
});
$("refreshBtn").addEventListener("click", refreshOverview);
$("scanBtn").addEventListener("click", startScan);
$("cleanBtn").addEventListener("click", cleanSelected);
$("cacheBtn").addEventListener("click", cleanAppCache);


/* =====================  v1.2: tabs, analysis, duplicates, apps, history  ===================== */
const HKEY = "md_history_v1";
function loadHistory() { try { return JSON.parse(localStorage.getItem(HKEY) || "[]"); } catch (e) { return []; } }
function logHistory(type, title, detail, bytes) {
  try {
    const h = loadHistory();
    h.unshift({ t: Date.now(), type, title, detail: detail || "", bytes: bytes || 0 });
    localStorage.setItem(HKEY, JSON.stringify(h.slice(0, 500)));
  } catch (e) { /* storage unavailable: history just won't persist */ }
  if (!$("tab-history").classList.contains("hidden")) renderHistory();
}
function renderHistory() {
  const w = $("histList"); w.innerHTML = "";
  const h = loadHistory();
  if (!h.length) { w.innerHTML = '<p class="muted">Nothing yet.</p>'; return; }
  const icon = { clean: "🧹", dupes: "🗂️", uninstall: "🗑️", analyze: "📊" };
  h.forEach((x) => {
    const d = document.createElement("div"); d.className = "hist";
    const t = document.createElement("div"); t.className = "t row";
    const a = document.createElement("span"); a.textContent = (icon[x.type] || "•") + " " + x.title;
    const b = document.createElement("span"); b.className = "mono"; b.textContent = x.bytes ? fmt(x.bytes) : "";
    t.appendChild(a); t.appendChild(b);
    const s = document.createElement("div"); s.className = "sub";
    s.textContent = new Date(x.t).toLocaleString() + (x.detail ? " · " + x.detail : "");
    d.appendChild(t); d.appendChild(s); w.appendChild(d);
  });
}
$("histClear").addEventListener("click", () => {
  if (!confirm("Clear all history?")) return;
  try { localStorage.removeItem(HKEY); } catch (e) {}
  renderHistory();
});

/* ---- tabs ---- */
function showTab(name) {
  document.querySelectorAll(".tab").forEach((b) => b.classList.toggle("active", b.dataset.tab === name));
  document.querySelectorAll(".panel").forEach((p) => p.classList.toggle("hidden", p.id !== "tab-" + name));
  if (name === "apps" && !appsLoaded) loadApps();
  if (name === "history") renderHistory();
}
document.querySelectorAll(".tab").forEach((b) => b.addEventListener("click", () => showTab(b.dataset.tab)));

function shortPath(p) { return p.replace(/^\/storage\/emulated\/0\/?/, ""); }
function el(tag, cls, text) { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }

/* ---- storage analysis ---- */
const CAT_LABEL = { images: "🖼️ Images", videos: "🎬 Videos", audio: "🎵 Audio", documents: "📄 Documents", archives: "🗜️ Archives", apks: "📦 APK installers", other: "📁 Other" };
let anTimer = null;

async function startAnalyze() {
  if (!(await guardPerm())) return;
  try {
    const ok = await invoke("mobile_analyze_start");
    if (!ok) { banner("Analysis already running…", true); }
    $("anBtn").disabled = true;
    $("anProg").classList.remove("hidden");
    $("anOut").classList.add("hidden");
    clearInterval(anTimer);
    anTimer = setInterval(pollAnalyze, 700);
  } catch (e) { banner("Analysis failed: " + e, false); }
}
async function pollAnalyze() {
  try {
    const s = await invoke("mobile_analyze_status");
    $("anCount").textContent = s.scanned.toLocaleString();
    if (!s.running && s.result) {
      clearInterval(anTimer); anTimer = null;
      $("anBtn").disabled = false; $("anProg").classList.add("hidden");
      renderAnalysis(s.result);
    } else if (!s.running && !s.result) {
      clearInterval(anTimer); anTimer = null;
      $("anBtn").disabled = false; $("anProg").classList.add("hidden");
      banner("Analysis did not finish. Try again.", false);
    }
  } catch (e) { clearInterval(anTimer); $("anBtn").disabled = false; banner("Poll error: " + e, false); }
}
function barRow(label, bytes, max, sub) {
  const d = el("div", "stat");
  const t = el("div", "t"); t.appendChild(el("span", "", label)); t.appendChild(el("span", "v", fmt(bytes)));
  const m = el("div", "mini"); const f = document.createElement("div");
  f.style.width = (max ? Math.max(2, Math.round((bytes / max) * 100)) : 0) + "%"; m.appendChild(f);
  d.appendChild(t); d.appendChild(m);
  if (sub) d.appendChild(el("div", "sub", sub));
  return d;
}
function renderAnalysis(r) {
  const cats = $("anCats"); cats.innerHTML = "";
  const cmax = Math.max(1, ...r.categories.map((c) => c.bytes));
  r.categories.forEach((c) => { if (c.count) cats.appendChild(barRow(CAT_LABEL[c.key] || c.key, c.bytes, cmax, c.count.toLocaleString() + " files")); });
  const fo = $("anFolders"); fo.innerHTML = "";
  const fmax = Math.max(1, ...r.top_folders.map((c) => c.bytes));
  r.top_folders.forEach((f) => fo.appendChild(barRow(f.path, f.bytes, fmax, f.files.toLocaleString() + " files")));
  const fl = $("anFiles"); fl.innerHTML = "";
  r.largest.forEach((f) => {
    const d = el("div", "stat");
    const t = el("div", "t"); t.appendChild(el("span", "", shortPath(f.path))); t.appendChild(el("span", "v", fmt(f.size)));
    const b = el("button", "btn small", "Delete"); b.style.marginTop = "6px";
    b.addEventListener("click", async () => {
      if (!confirm("Delete this file?\n" + shortPath(f.path))) return;
      try {
        const x = await invoke("mobile_clean_files", { paths: [f.path] });
        if (x.deleted) { d.remove(); logHistory("clean", "File deleted", shortPath(f.path), x.freed_bytes); banner("Deleted · freed " + fmt(x.freed_bytes), true); refreshOverview(); }
        else banner("Could not delete that file.", false);
      } catch (e) { banner("Delete failed: " + e, false); }
    });
    d.appendChild(t); d.appendChild(b); fl.appendChild(d);
  });
  $("anOut").classList.remove("hidden");
  if (r.stopped_early) banner("Analysis hit the file cap — numbers are partial.", false);
}
$("anBtn").addEventListener("click", startAnalyze);

/* ---- duplicates ---- */
let dpTimer = null;
let dpSel = new Set();
let dpSizes = new Map();

async function startDupes() {
  if (!(await guardPerm())) return;
  try {
    const ok = await invoke("mobile_dupes_start");
    if (!ok) banner("A duplicate scan is already running…", true);
    $("dpBtn").disabled = true; $("dpRes").classList.add("hidden");
    $("dpProg").classList.remove("hidden"); $("dpCancel").classList.remove("hidden");
    clearInterval(dpTimer); dpTimer = setInterval(pollDupes, 800);
  } catch (e) { banner("Could not start: " + e, false); }
}
function dpDone() {
  clearInterval(dpTimer); dpTimer = null;
  $("dpBtn").disabled = false; $("dpProg").classList.add("hidden"); $("dpCancel").classList.add("hidden");
}
async function pollDupes() {
  try {
    const s = await invoke("mobile_dupes_status");
    const txt = { 1: "Step 1/3 · listing files: ", 2: "Step 2/3 · comparing samples: ", 3: "Step 3/3 · verifying contents: " };
    $("dpProg").textContent = (txt[s.phase] || "Working: ") + s.progress.toLocaleString();
    if (!s.running) {
      dpDone();
      if (s.result) renderDupes(s.result); else banner("Scan did not finish.", false);
    }
  } catch (e) { dpDone(); banner("Poll error: " + e, false); }
}
function updateDpBtn() {
  let bytes = 0; dpSel.forEach((p) => { bytes += dpSizes.get(p) || 0; });
  $("dpDel").textContent = "Delete " + dpSel.size + " selected copies (" + fmt(bytes) + ")";
  $("dpDel").disabled = dpSel.size === 0;
}
function renderDupes(r) {
  dpSel = new Set(); dpSizes = new Map();
  const w = $("dpList"); w.innerHTML = "";
  if (!r.groups.length) { banner("No duplicates found. 🥭", true); return; }
  r.groups.forEach((g) => {
    const card = el("div", "group");
    const gh = el("div", "gh"); gh.appendChild(el("span", "", g.files.length + " copies"));
    gh.appendChild(el("span", "mono", fmt(g.size) + " each")); card.appendChild(gh);
    g.files.forEach((p, i) => {
      const row = el("label", "item");
      const c = document.createElement("input"); c.type = "checkbox";
      dpSizes.set(p, g.size);
      if (i === 0) { c.disabled = true; } else { c.checked = true; dpSel.add(p); }
      c.addEventListener("change", () => { if (c.checked) dpSel.add(p); else dpSel.delete(p); updateDpBtn(); });
      row.appendChild(c); row.appendChild(el("span", "p", shortPath(p)));
      if (i === 0) row.appendChild(el("span", "keep", "KEEP"));
      card.appendChild(row);
    });
    w.appendChild(card);
  });
  $("dpWasted").textContent = fmt(r.wasted_bytes);
  $("dpMeta").textContent = r.total_groups.toLocaleString() + " groups · " + r.scanned_files.toLocaleString() + " files scanned" +
    (r.total_groups > r.groups.length ? " · showing top " + r.groups.length : "") + (r.stopped_early ? " · partial" : "");
  $("dpRes").classList.remove("hidden"); updateDpBtn();
}
async function deleteDupes() {
  if (!(await guardPerm())) return;
  const paths = [...dpSel];
  if (!paths.length) return;
  if (!confirm("Delete " + paths.length + " duplicate copies?\nThe oldest copy in each group is kept.\nThis cannot be undone.")) return;
  $("dpDel").disabled = true;
  try {
    const r = await invoke("mobile_clean_files", { paths });
    if (r.deleted) logHistory("dupes", "Duplicates removed", r.deleted + " files", r.freed_bytes);
    banner("Deleted " + r.deleted + " copies · freed " + fmt(r.freed_bytes) + (r.failed.length ? " · " + r.failed.length + " failed" : ""), !r.failed.length);
    $("dpRes").classList.add("hidden"); refreshOverview();
  } catch (e) { banner("Delete failed: " + e, false); $("dpDel").disabled = false; }
}
$("dpBtn").addEventListener("click", startDupes);
$("dpCancel").addEventListener("click", () => invoke("mobile_dupes_cancel").catch(() => {}));
$("dpDel").addEventListener("click", deleteDupes);

/* ---- apps ---- */
let apps = [], appsLoaded = false, pendingUninstall = null, pendingTimer = null;

function loadApps() {
  if (!hasBridge() || typeof window.MangoDiskBridge.getInstalledApps !== "function") {
    $("appList").innerHTML = '<p class="muted">App list unavailable (bridge not ready). Reopen MangoDisk.</p>'; return;
  }
  $("appCount").textContent = "loading…";
  setTimeout(() => {
    try { apps = JSON.parse(window.MangoDiskBridge.getInstalledApps()); appsLoaded = true; }
    catch (e) { banner("Could not read app list: " + e, false); apps = []; }
    renderApps();
  }, 30);
}
function renderApps() {
  const q = $("appSearch").value.trim().toLowerCase();
  const sys = $("appSys").checked, sort = $("appSort").value;
  let list = apps.filter((a) => (sys || !a.system) && (!q || a.name.toLowerCase().includes(q) || a.pkg.toLowerCase().includes(q)));
  const by = { size: (a, b) => b.size - a.size, name: (a, b) => a.name.localeCompare(b.name),
    installed: (a, b) => b.installed - a.installed, oldest: (a, b) => a.installed - b.installed };
  list.sort(by[sort] || by.size);
  $("appCount").textContent = list.length + " apps";
  const w = $("appList"); w.innerHTML = "";
  list.slice(0, 400).forEach((a) => {
    const d = el("div", "app");
    d.appendChild(el("div", "av", (a.name[0] || "?").toUpperCase()));
    const i = el("div", "info"); i.appendChild(el("div", "nm", a.name));
    i.appendChild(el("div", "pk", a.pkg + (a.version ? " · v" + a.version : "")));
    i.appendChild(el("div", "sz", fmt(a.size) + (a.installed ? " · " + new Date(a.installed).toLocaleDateString() : "")));
    d.appendChild(i);
    if (a.system) { d.appendChild(el("span", "keep", "SYSTEM")); }
    else {
      const b = el("button", "btn danger", "Uninstall");
      b.addEventListener("click", () => uninstall(a));
      d.appendChild(b);
    }
    w.appendChild(d);
  });
}
function uninstall(a) {
  if (!confirm("Uninstall " + a.name + "?\nAndroid will ask you to confirm.")) return;
  try {
    pendingUninstall = a;
    window.MangoDiskBridge.uninstallApp(a.pkg);
    clearInterval(pendingTimer);
    let n = 0;
    pendingTimer = setInterval(() => { checkPending(); if (++n > 60 || !pendingUninstall) { clearInterval(pendingTimer); } }, 2000);
  } catch (e) { pendingUninstall = null; banner("Could not start uninstall: " + e, false); }
}
function checkPending() {
  if (!pendingUninstall) return;
  let still = true;
  try { still = !!window.MangoDiskBridge.isAppInstalled(pendingUninstall.pkg); } catch (e) { return; }
  if (!still) {
    const a = pendingUninstall; pendingUninstall = null;
    logHistory("uninstall", "Uninstalled " + a.name, a.pkg, a.size);
    apps = apps.filter((x) => x.pkg !== a.pkg); renderApps();
    banner(a.name + " uninstalled · ~" + fmt(a.size) + " freed", true); refreshOverview();
  }
}
document.addEventListener("visibilitychange", () => { if (!document.hidden) setTimeout(() => { checkPending(); if (pendingUninstall) { /* user cancelled or still pending */ } }, 600); });
$("appSearch").addEventListener("input", renderApps);
$("appSort").addEventListener("change", renderApps);
$("appSys").addEventListener("change", renderApps);

refreshPerm();
refreshOverview();
