// Tauri commands for Storage Analysis + Duplicate Finder (Android only).
#![cfg(target_os = "android")]

use crate::mobile_core::{self, AnalysisResult, DupeResult};
use serde::Serialize;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use tauri::command;

const ROOT: &str = "/storage/emulated/0";
const DUPE_MIN_SIZE: u64 = 100 * 1024;

struct Job<T> {
    running: AtomicBool,
    cancel: AtomicBool,
    progress: AtomicU64,
    phase: AtomicU64,
    result: Mutex<Option<T>>,
}

impl<T> Job<T> {
    fn new() -> Self {
        Job {
            running: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            progress: AtomicU64::new(0),
            phase: AtomicU64::new(0),
            result: Mutex::new(None),
        }
    }
    fn begin(&self) -> bool {
        if self.running.swap(true, Ordering::SeqCst) {
            return false;
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.progress.store(0, Ordering::Relaxed);
        self.phase.store(0, Ordering::Relaxed);
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = None;
        true
    }
}

static ANALYZE: LazyLock<Job<AnalysisResult>> = LazyLock::new(Job::new);
static DUPES: LazyLock<Job<DupeResult>> = LazyLock::new(Job::new);

#[derive(Serialize)]
pub struct AnalyzeStatus {
    running: bool,
    scanned: u64,
    result: Option<AnalysisResult>,
}

#[derive(Serialize)]
pub struct DupeStatus {
    running: bool,
    phase: u64,
    progress: u64,
    result: Option<DupeResult>,
}

#[command]
pub fn mobile_analyze_start() -> bool {
    if !ANALYZE.begin() {
        return false;
    }
    std::thread::spawn(|| {
        let r = catch_unwind(AssertUnwindSafe(|| {
            mobile_core::analyze(Path::new(ROOT), &ANALYZE.progress, &ANALYZE.cancel)
        }));
        if let Ok(res) = r {
            *ANALYZE.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(res);
        }
        ANALYZE.running.store(false, Ordering::SeqCst);
    });
    true
}

#[command]
pub fn mobile_analyze_status() -> AnalyzeStatus {
    AnalyzeStatus {
        running: ANALYZE.running.load(Ordering::Relaxed),
        scanned: ANALYZE.progress.load(Ordering::Relaxed),
        result: ANALYZE.result.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

#[command]
pub fn mobile_dupes_start() -> bool {
    if !DUPES.begin() {
        return false;
    }
    std::thread::spawn(|| {
        let r = catch_unwind(AssertUnwindSafe(|| {
            mobile_core::find_dupes(
                Path::new(ROOT),
                DUPE_MIN_SIZE,
                &DUPES.progress,
                &DUPES.phase,
                &DUPES.cancel,
            )
        }));
        if let Ok(res) = r {
            *DUPES.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(res);
        }
        DUPES.running.store(false, Ordering::SeqCst);
    });
    true
}

#[command]
pub fn mobile_dupes_status() -> DupeStatus {
    DupeStatus {
        running: DUPES.running.load(Ordering::Relaxed),
        phase: DUPES.phase.load(Ordering::Relaxed),
        progress: DUPES.progress.load(Ordering::Relaxed),
        result: DUPES.result.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

#[command]
pub fn mobile_dupes_cancel() {
    DUPES.cancel.store(true, Ordering::Relaxed);
    ANALYZE.cancel.store(true, Ordering::Relaxed);
}
