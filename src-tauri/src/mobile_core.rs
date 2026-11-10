// MangoDisk Android Edition - storage analysis + duplicate finder core.
// Pure std + serde (no tauri / no android deps) so it is unit-testable on any host.
#![allow(dead_code)]

use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::fs::{self, File};
use std::hash::Hasher;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub const MAX_ANALYZE_FILES: u64 = 400_000;
pub const MAX_DUPE_FILES: u64 = 300_000;
pub const MAX_GROUPS_RETURNED: usize = 300;
const SAMPLE: u64 = 32 * 1024;

/// Folders never descended into at the root (kernel-locked on API 30+ anyway).
const SKIP_ROOT: &[&str] = &["android", "proc", "sys", "dev"];

#[derive(Serialize, Clone, Default)]
pub struct CatStat {
    pub key: String,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Serialize, Clone)]
pub struct FolderStat {
    pub path: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Serialize, Clone)]
pub struct FileStat {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize, Clone)]
pub struct AnalysisResult {
    pub done: bool,
    pub scanned_files: u64,
    pub total_bytes: u64,
    pub categories: Vec<CatStat>,
    pub top_folders: Vec<FolderStat>,
    pub sub_folders: Vec<FolderStat>,
    pub largest: Vec<FileStat>,
    pub stopped_early: bool,
}

pub fn category_of(name: &str) -> &'static str {
    let lower = name.to_lowercase();
    let ext = match lower.rsplit_once('.') {
        Some((_, e)) => e,
        None => return "other",
    };
    match ext {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "heif" | "dng" | "raw" | "svg" => "images",
        "mp4" | "mkv" | "avi" | "mov" | "3gp" | "webm" | "flv" | "wmv" | "m4v" | "ts" => "videos",
        "mp3" | "m4a" | "wav" | "flac" | "aac" | "ogg" | "opus" | "amr" | "wma" => "audio",
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "csv" | "epub" | "odt" | "rtf" => "documents",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "archives",
        "apk" | "xapk" | "apks" | "aab" => "apks",
        _ => "other",
    }
}

fn skip_dir(rel_is_root: bool, name: &str) -> bool {
    rel_is_root && SKIP_ROOT.contains(&name.to_lowercase().as_str())
}

pub fn analyze(root: &Path, progress: &AtomicU64, cancel: &AtomicBool) -> AnalysisResult {
    let mut cats: HashMap<&'static str, (u64, u64)> = HashMap::new();
    let mut l1: HashMap<String, (u64, u64)> = HashMap::new();
    let mut l2: HashMap<String, (u64, u64)> = HashMap::new();
    let mut largest: Vec<FileStat> = Vec::new();
    let mut seen = 0u64;
    let mut total = 0u64;
    let mut stopped = false;

    let mut queue: VecDeque<(PathBuf, Vec<String>)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), Vec::new()));

    'outer: while let Some((abs, rel)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) || seen >= MAX_ANALYZE_FILES {
            stopped = true;
            break;
        }
        let rd = match fs::read_dir(&abs) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = match entry.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            if ft.is_dir() {
                if skip_dir(rel.is_empty(), &name) {
                    continue;
                }
                let mut r = rel.clone();
                r.push(name.clone());
                queue.push_back((abs.join(&name), r));
            } else if ft.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                seen += 1;
                total += size;
                if seen % 256 == 0 {
                    progress.store(seen, Ordering::Relaxed);
                    if cancel.load(Ordering::Relaxed) {
                        stopped = true;
                        break 'outer;
                    }
                }
                let c = cats.entry(category_of(&name)).or_insert((0, 0));
                c.0 += 1;
                c.1 += size;

                let k1 = rel.first().cloned().unwrap_or_else(|| "(files in root)".to_string());
                let e1 = l1.entry(k1).or_insert((0, 0));
                e1.0 += 1;
                e1.1 += size;
                if rel.len() >= 2 {
                    let e2 = l2.entry(format!("{}/{}", rel[0], rel[1])).or_insert((0, 0));
                    e2.0 += 1;
                    e2.1 += size;
                }

                if largest.len() < 30 {
                    largest.push(FileStat { path: abs.join(&name).to_string_lossy().to_string(), size });
                } else if let Some((i, m)) = largest.iter().enumerate().min_by_key(|(_, x)| x.size) {
                    if m.size < size {
                        largest[i] = FileStat { path: abs.join(&name).to_string_lossy().to_string(), size };
                    }
                }
            }
        }
    }
    progress.store(seen, Ordering::Relaxed);

    let mut categories: Vec<CatStat> = ["images", "videos", "audio", "documents", "archives", "apks", "other"]
        .iter()
        .map(|k| {
            let (count, bytes) = cats.get(k).copied().unwrap_or((0, 0));
            CatStat { key: k.to_string(), count, bytes }
        })
        .collect();
    categories.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    let to_vec = |m: HashMap<String, (u64, u64)>, n: usize| -> Vec<FolderStat> {
        let mut v: Vec<FolderStat> =
            m.into_iter().map(|(path, (files, bytes))| FolderStat { path, files, bytes }).collect();
        v.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        v.truncate(n);
        v
    };
    largest.sort_by(|a, b| b.size.cmp(&a.size));

    AnalysisResult {
        done: true,
        scanned_files: seen,
        total_bytes: total,
        categories,
        top_folders: to_vec(l1, 15),
        sub_folders: to_vec(l2, 25),
        largest,
        stopped_early: stopped,
    }
}

// ---------------------------------------------------------------- duplicates

#[derive(Serialize, Clone)]
pub struct DupeGroup {
    pub size: u64,
    pub files: Vec<String>, // oldest first: index 0 is the one to KEEP
}

#[derive(Serialize, Clone)]
pub struct DupeResult {
    pub done: bool,
    pub scanned_files: u64,
    pub groups: Vec<DupeGroup>,
    pub total_groups: u64,
    pub wasted_bytes: u64,
    pub stopped_early: bool,
}

/// Two independent 64-bit hashes (SipHash + FNV-1a) in one pass. Files are only
/// hashed after they already match on exact size + sampled content, so a false
/// positive is practically impossible.
struct Digest {
    sip: DefaultHasher,
    fnv: u64,
}

impl Digest {
    fn new() -> Self {
        Digest { sip: DefaultHasher::new(), fnv: 0xcbf29ce484222325 }
    }
    fn update(&mut self, buf: &[u8]) {
        self.sip.write(buf);
        let mut h = self.fnv;
        for b in buf {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        self.fnv = h;
    }
    fn finish(&self) -> (u64, u64) {
        (self.sip.finish(), self.fnv)
    }
}

fn sample_digest(path: &Path, size: u64) -> Option<(u64, u64)> {
    let mut f = File::open(path).ok()?;
    let mut d = Digest::new();
    let mut buf = vec![0u8; SAMPLE as usize];
    let head = SAMPLE.min(size) as usize;
    f.read_exact(&mut buf[..head]).ok()?;
    d.update(&buf[..head]);
    if size > SAMPLE * 2 {
        f.seek(SeekFrom::Start(size / 2 - SAMPLE / 2)).ok()?; // middle
        f.read_exact(&mut buf[..SAMPLE as usize]).ok()?;
        d.update(&buf[..SAMPLE as usize]);
    }
    if size > SAMPLE {
        let tail = SAMPLE.min(size - SAMPLE) as usize;
        f.seek(SeekFrom::Start(size - tail as u64)).ok()?;
        f.read_exact(&mut buf[..tail]).ok()?;
        d.update(&buf[..tail]);
    }
    Some(d.finish())
}

fn full_digest(path: &Path, size: u64, cancel: &AtomicBool) -> Option<(u64, u64)> {
    let mut f = File::open(path).ok()?;
    let mut d = Digest::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut read = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        d.update(&buf[..n]);
        read += n as u64;
    }
    // file changed while hashing -> do not trust it
    if read != size {
        return None;
    }
    Some(d.finish())
}

/// `phase`: 1 = listing, 2 = comparing samples, 3 = full hashing. `progress` = files done in phase.
pub fn find_dupes(
    root: &Path,
    min_size: u64,
    progress: &AtomicU64,
    phase: &AtomicU64,
    cancel: &AtomicBool,
) -> DupeResult {
    phase.store(1, Ordering::Relaxed);
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();
    let mut seen = 0u64;
    let mut stopped = false;
    let mut queue: VecDeque<(PathBuf, bool)> = VecDeque::new();
    queue.push_back((root.to_path_buf(), true));

    'walk: while let Some((abs, is_root)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) || seen >= MAX_DUPE_FILES {
            stopped = true;
            break;
        }
        let rd = match fs::read_dir(&abs) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let ft = match entry.file_type() {
                Ok(f) => f,
                Err(_) => continue,
            };
            if ft.is_dir() {
                if skip_dir(is_root, &name) {
                    continue;
                }
                queue.push_back((abs.join(&name), false));
            } else if ft.is_file() {
                seen += 1;
                if seen % 256 == 0 {
                    progress.store(seen, Ordering::Relaxed);
                    if cancel.load(Ordering::Relaxed) {
                        stopped = true;
                        break 'walk;
                    }
                }
                if let Ok(md) = entry.metadata() {
                    if md.len() >= min_size {
                        by_size.entry(md.len()).or_default().push(abs.join(&name));
                    }
                }
            }
        }
    }
    progress.store(seen, Ordering::Relaxed);

    // phase 2: sample digest within same-size buckets
    phase.store(2, Ordering::Relaxed);
    progress.store(0, Ordering::Relaxed);
    let mut done = 0u64;
    let mut candidates: HashMap<(u64, u64, u64), Vec<PathBuf>> = HashMap::new();
    'p2: for (size, paths) in by_size.into_iter().filter(|(_, v)| v.len() > 1) {
        for p in paths {
            if cancel.load(Ordering::Relaxed) {
                stopped = true;
                break 'p2;
            }
            done += 1;
            progress.store(done, Ordering::Relaxed);
            if let Some((a, b)) = sample_digest(&p, size) {
                candidates.entry((size, a, b)).or_default().push(p);
            }
        }
    }

    // phase 3: full digest, only for files that still collide
    phase.store(3, Ordering::Relaxed);
    progress.store(0, Ordering::Relaxed);
    done = 0;
    let mut groups: Vec<DupeGroup> = Vec::new();
    let mut wasted = 0u64;
    let mut total_groups = 0u64;
    'p3: for ((size, _, _), paths) in candidates.into_iter().filter(|(_, v)| v.len() > 1) {
        let mut full: HashMap<(u64, u64), Vec<PathBuf>> = HashMap::new();
        for p in paths {
            if cancel.load(Ordering::Relaxed) {
                stopped = true;
                break 'p3;
            }
            done += 1;
            progress.store(done, Ordering::Relaxed);
            if let Some(h) = full_digest(&p, size, cancel) {
                full.entry(h).or_default().push(p);
            }
        }
        for (_, mut files) in full.into_iter().filter(|(_, v)| v.len() > 1) {
            files.sort_by(|a, b| {
                let ma = fs::metadata(a).and_then(|m| m.modified()).ok();
                let mb = fs::metadata(b).and_then(|m| m.modified()).ok();
                ma.cmp(&mb).then_with(|| a.cmp(b))
            });
            total_groups += 1;
            wasted += size * (files.len() as u64 - 1);
            groups.push(DupeGroup {
                size,
                files: files.into_iter().map(|p| p.to_string_lossy().to_string()).collect(),
            });
        }
    }
    groups.sort_by(|a, b| {
        (b.size * (b.files.len() as u64 - 1)).cmp(&(a.size * (a.files.len() as u64 - 1)))
    });
    groups.truncate(MAX_GROUPS_RETURNED);

    DupeResult {
        done: true,
        scanned_files: seen,
        groups,
        total_groups,
        wasted_bytes: wasted,
        stopped_early: stopped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn mk(dir: &Path, rel: &str, data: &[u8]) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        File::create(p).unwrap().write_all(data).unwrap();
    }

    #[test]
    fn finds_dupes_and_analyzes() {
        let d = std::env::temp_dir().join(format!("mdtest_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let big: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let mut big2 = big.clone();
        big2[100_000] ^= 1; // differs in an unsampled region -> only full hash can tell
        mk(&d, "DCIM/a.jpg", &big);
        mk(&d, "Download/a_copy.jpg", &big);
        mk(&d, "Pictures/a_copy2.jpg", &big);
        mk(&d, "Pictures/near.jpg", &big2);
        mk(&d, "Music/x.mp3", &[1u8; 150_000]);
        mk(&d, "Android/data/hidden.bin", &big);
        let p = AtomicU64::new(0);
        let ph = AtomicU64::new(0);
        let c = AtomicBool::new(false);
        let r = find_dupes(&d, 100_000, &p, &ph, &c);
        assert_eq!(r.total_groups, 1, "one dup group expected");
        assert_eq!(r.groups[0].files.len(), 3);
        assert_eq!(r.wasted_bytes, 600_000);
        assert!(r.groups[0].files.iter().all(|f| !f.contains("Android")));
        assert!(!r.groups[0].files.iter().any(|f| f.contains("near")));

        let a = analyze(&d, &p, &c);
        assert_eq!(a.scanned_files, 5);
        assert_eq!(a.categories[0].key, "images");
        assert_eq!(a.categories[0].count, 4);
        assert!(a.largest[0].size >= 300_000);
        let _ = fs::remove_dir_all(&d);
    }
}
