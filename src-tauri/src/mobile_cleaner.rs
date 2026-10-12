// MangoDisk Android Edition - REAL working junk scanner & cleaner.
// Scans public shared storage (/storage/emulated/0) plus the app's own cache,
// classifies junk (temp/log files, cache contents, empty dirs, huge files),
// and deletes ONLY validated paths under the scan root. No root required:
// uses the MANAGE_EXTERNAL_STORAGE ("All files access") grant.
#![cfg(target_os = "android")]

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::command;
use tauri::Manager;

const SCAN_ROOT: &str = "/storage/emulated/0";
const MAX_ENTRIES_PER_CAT: usize = 400;
const MAX_WALK_FILES: u64 = 150_000;
const LARGE_FILE_MIN: u64 = 200 * 1024 * 1024;

// Path segments that mark a file as cache content (safe to delete)
const CACHE_DIR_NAMES: &[&str] = &[".cache", ".thumbnails", ".recycle", "caches"];
// Top-level segments we never descend into (kernel-locked on API 30+ anyway)
const SKIP_SEGMENTS: &[&str] = &["Android", "proc", "sys", "dev"];

fn is_temp_ext(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".tmp", ".temp", ".log", ".bak", ".old", ".crdownload", ".part", ".partial", ".download"]
        .iter()
        .any(|e| lower.ends_with(e))
        || lower == "thumbs.db"
        || lower == ".ds_store"
}

fn is_junk_meta(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "thumbs.db" | ".ds_store" | "desktop.ini")
}

#[derive(Serialize, Clone)]
pub struct JunkEntry {
    path: String,
    size: u64,
}

#[derive(Serialize, Clone)]
pub struct ScanResult {
    done: bool,
    scanned_files: u64,
    total_reclaimable: u64,
    counts: HashMap<String, u64>,
    bytes: HashMap<String, u64>,
    categories: HashMap<String, Vec<JunkEntry>>,
    truncated: bool,
    stopped_early: bool,
}

struct ScanState {
    running: AtomicBool,
    scanned: AtomicU64,
    result: Mutex<Option<ScanResult>>,
}

static SCAN: LazyLock<ScanState> = LazyLock::new(|| ScanState {
    running: AtomicBool::new(false),
    scanned: AtomicU64::new(0),
    result: Mutex::new(None),
});

struct Walker {
    temp_logs: Vec<JunkEntry>,
    caches: Vec<JunkEntry>,
    junk_meta: Vec<JunkEntry>,
    empty_dirs: Vec<JunkEntry>,
    large_files: Vec<JunkEntry>,
    counts: HashMap<String, u64>,
    bytes: HashMap<String, u64>,
    files_seen: u64,
    truncated: bool,
    stopped_early: bool,
}

impl Walker {
    fn new() -> Self {
        Walker {
            temp_logs: Vec::new(),
            caches: Vec::new(),
            junk_meta: Vec::new(),
            empty_dirs: Vec::new(),
            large_files: Vec::new(),
            counts: HashMap::new(),
            bytes: HashMap::new(),
            files_seen: 0,
            truncated: false,
            stopped_early: false,
        }
    }

    fn push(&mut self, cat: &str, e: JunkEntry) {
        *self.counts.entry(cat.to_string()).or_insert(0) += 1;
        *self.bytes.entry(cat.to_string()).or_insert(0) += e.size;
        let list = match cat {
            "temp_logs" => &mut self.temp_logs,
            "caches" => &mut self.caches,
            "junk_meta" => &mut self.junk_meta,
            "empty_dirs" => &mut self.empty_dirs,
            "large_files" => &mut self.large_files,
            _ => return,
        };
        if list.len() < MAX_ENTRIES_PER_CAT {
            list.push(e);
        } else {
            // keep only the biggest for large files; others just count
            if cat == "large_files" {
                if let Some(max_i) =
                    list.iter().enumerate().min_by_key(|(_, x)| x.size).map(|(i, _)| i)
                {
                    if list[max_i].size < e.size {
                        list[max_i] = e;
                    }
                }
            } else {
                self.truncated = true;
            }
        }
    }

    fn has_cache_parent(rel: &Path) -> bool {
        rel.components().any(|c| {
            let s = c.as_os_str().to_string_lossy().to_lowercase();
            CACHE_DIR_NAMES.contains(&s.as_str())
        })
    }
}

fn dir_size(path: &Path, budget: u64) -> (u64, bool) {
    let mut total = 0u64;
    let mut n = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(d) = stack.pop() {
        if n > budget {
            return (total, true);
        }
        let rd = match fs::read_dir(&d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for e in rd.flatten() {
            n += 1;
            let p = e.path();
            match e.file_type() {
                Ok(ft) if ft.is_dir() => stack.push(p),
                Ok(_) => {
                    if let Ok(md) = e.metadata() {
                        total += md.len();
                    }
                }
                Err(_) => {}
            }
        }
    }
    (total, false)
}

fn scan_worker() {
    let mut w = Walker::new();
    let mut stack: Vec<(PathBuf, PathBuf)> = vec![(PathBuf::from(SCAN_ROOT), PathBuf::new())];

    while let Some((abs, rel)) = stack.pop() {
        if w.files_seen > MAX_WALK_FILES {
            w.stopped_early = true;
            break;
        }
        let rd = match fs::read_dir(&abs) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut child_count = 0u64;
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = match entry.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            let child_abs = abs.join(&name);
            let child_rel = rel.join(&name);

            if ft.is_dir() {
                child_count += 1;
                let lower = name.to_lowercase();
                if rel.as_os_str().is_empty() && SKIP_SEGMENTS.contains(&lower.as_str()) {
                    continue;
                }
                stack.push((child_abs, child_rel));
            } else if ft.is_file() {
                child_count += 1;
                w.files_seen += 1;
                SCAN.scanned.store(w.files_seen, Ordering::Relaxed);
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let je = JunkEntry { path: child_abs.to_string_lossy().to_string(), size };
                if size >= LARGE_FILE_MIN {
                    w.push("large_files", je);
                    continue;
                }
                if Walker::has_cache_parent(&child_rel) {
                    w.push("caches", je);
                } else if is_temp_ext(&name) {
                    w.push("temp_logs", je);
                } else if is_junk_meta(&name) {
                    w.push("junk_meta", je);
                }
            }
        }
        if child_count == 0 && !rel.as_os_str().is_empty() {
            w.push(
                "empty_dirs",
                JunkEntry { path: abs.to_string_lossy().to_string(), size: 0 },
            );
        }
    }

    // sort large files desc for display
    w.large_files.sort_by(|a, b| b.size.cmp(&a.size));

    let total = w.bytes.values().sum::<u64>();
    let result = ScanResult {
        done: true,
        scanned_files: w.files_seen,
        total_reclaimable: total,
        counts: w.counts,
        bytes: w.bytes,
        categories: [
            ("temp_logs".to_string(), w.temp_logs),
            ("caches".to_string(), w.caches),
            ("junk_meta".to_string(), w.junk_meta),
            ("empty_dirs".to_string(), w.empty_dirs),
            ("large_files".to_string(), w.large_files),
        ]
        .into_iter()
        .collect(),
        truncated: w.truncated,
        stopped_early: w.stopped_early,
    };

    let mut st = SCAN.result.lock().unwrap();
    *st = Some(result);
    SCAN.running.store(false, Ordering::Relaxed);
}

#[derive(Serialize)]
pub struct Overview {
    total_bytes: u64,
    free_bytes: u64,
    used_bytes: u64,
    scan_root: String,
    permission_ok: bool,
    app_cache_bytes: u64,
    app_cache_path: String,
}

fn statvfs_totals(path: &str) -> Option<(u64, u64)> {
    use std::ffi::CString;
    let c = CString::new(path).ok()?;
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) != 0 {
            return None;
        }
        let bs = st.f_frsize as u64;
        Some((st.f_blocks as u64 * bs, st.f_bavail as u64 * bs))
    }
}

fn app_cache_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_cache_dir().ok()
}

fn quick_cache_size(p: &Path) -> u64 {
    let (size, _) = dir_size(p, 60_000);
    size
}

#[command]
pub fn mobile_storage_overview(app: tauri::AppHandle) -> Overview {
    let (total, free) = statvfs_totals(SCAN_ROOT).unwrap_or((0, 0));
    let used = total.saturating_sub(free);
    let cache = app_cache_dir(&app);
    Overview {
        total_bytes: total,
        free_bytes: free,
        used_bytes: used,
        scan_root: SCAN_ROOT.to_string(),
        permission_ok: Path::new(SCAN_ROOT).is_dir() && fs::read_dir(SCAN_ROOT).is_ok(),
        app_cache_bytes: cache.as_deref().map(quick_cache_size).unwrap_or(0),
        app_cache_path: cache
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
    }
}

#[command]
pub fn mobile_scan_start() -> bool {
    if SCAN.running.swap(true, Ordering::SeqCst) {
        return false; // already running
    }
    SCAN.scanned.store(0, Ordering::Relaxed);
    {
        let mut st = SCAN.result.lock().unwrap();
        *st = None;
    }
    std::thread::spawn(scan_worker);
    true
}

#[derive(Serialize)]
pub struct ScanStatus {
    running: bool,
    scanned: u64,
    result: Option<ScanResult>,
}

#[command]
pub fn mobile_scan_status() -> ScanStatus {
    ScanStatus {
        running: SCAN.running.load(Ordering::Relaxed),
        scanned: SCAN.scanned.load(Ordering::Relaxed),
        result: SCAN.result.lock().unwrap().clone(),
    }
}

#[derive(Serialize)]
pub struct CleanReport {
    deleted: u64,
    freed_bytes: u64,
    failed: Vec<String>,
}

#[command]
pub fn mobile_clean_files(paths: Vec<String>) -> CleanReport {
    let mut report = CleanReport { deleted: 0, freed_bytes: 0, failed: Vec::new() };
    let root_prefix = PathBuf::from(SCAN_ROOT);

    for raw in paths.into_iter().take(2000) {
        let p = PathBuf::from(&raw);
        // SAFETY: only delete inside the scan root, never the root itself
        let canonical = p.canonicalize().unwrap_or(p.clone());
        if canonical != root_prefix
            && canonical.starts_with(&root_prefix)
            && canonical.to_string_lossy().starts_with(SCAN_ROOT)
        {
            let size = if canonical.is_file() {
                fs::metadata(&canonical).map(|m| m.len()).unwrap_or(0)
            } else {
                let (s, _) = dir_size(&canonical, 80_000);
                s
            };
            let ok = if canonical.is_dir() {
                fs::remove_dir_all(&canonical).is_ok()
            } else {
                fs::remove_file(&canonical).is_ok()
            };
            if ok {
                report.deleted += 1;
                report.freed_bytes += size;
            } else {
                report.failed.push(raw);
            }
        } else {
            report.failed.push(raw);
        }
    }
    report
}

#[derive(Serialize)]
pub struct CacheClean {
    freed_bytes: u64,
    ok: bool,
    path: String,
}

#[command]
pub fn mobile_clean_app_cache(app: tauri::AppHandle) -> CacheClean {
    let before = app_cache_dir(&app).map(|p| quick_cache_size(&p)).unwrap_or(0);
    let path = app_cache_dir(&app);
    let mut ok = false;
    if let Some(dir) = &path {
        if let Ok(rd) = fs::read_dir(dir) {
            ok = true;
            for e in rd.flatten() {
                let p = e.path();
                let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
            }
        }
    }
    CacheClean {
        freed_bytes: if ok { before } else { 0 },
        ok,
        path: path.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
    }
}
