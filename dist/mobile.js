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
    banner(r.ok ? "App cache cleared · freed " + fmt(r.freed_bytes) : "Could not clear cache.", r.ok);
    refreshOverview();
  } catch (e) { banner("App cache clean failed: " + e, false); }
}

/* ---------- wiring ---------- */
$("grantBtn").addEventListener("click", () => {
  try { window.MangoDiskBridge.requestAllFilesAccess(); }
  catch (e) { banner("Bridge error: " + e, false); }
});
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) { refreshPerm(); refreshOverview(); }
});
$("refreshBtn").addEventListener("click", refreshOverview);
$("scanBtn").addEventListener("click", startScan);
$("cleanBtn").addEventListener("click", cleanSelected);
$("cacheBtn").addEventListener("click", cleanAppCache);

refreshPerm();
refreshOverview();
