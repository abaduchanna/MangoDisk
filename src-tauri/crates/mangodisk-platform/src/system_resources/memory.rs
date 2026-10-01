use std::path::PathBuf;

use sysinfo::{MemoryRefreshKind, System};
#[cfg(not(windows))]
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, UpdateKind};

use crate::{PlatformError, PlatformErrorCode, PlatformResult};

#[derive(Debug, Clone)]
pub struct ProcessMemory {
    pub pid: u32,
    pub name: String,
    pub executable: Option<PathBuf>,
    pub used_bytes: Option<u64>,
    pub is_application: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessMemoryKind {
    PhysicalFootprint,
    PrivateWorkingSet,
    ResidentSet,
}
impl ProcessMemoryKind {
    pub fn native() -> Self {
        if cfg!(target_os = "macos") {
            Self::PhysicalFootprint
        } else if cfg!(windows) {
            Self::PrivateWorkingSet
        } else {
            Self::ResidentSet
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeMemorySnapshot {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub swap_used_bytes: u64,
    pub process_memory_kind: ProcessMemoryKind,
    /// None means no process enumeration was requested, not an empty process list.
    pub processes: Option<Vec<ProcessMemory>>,
}

/// Allows deterministic Core tests without replacing native APIs in production.
pub trait MemorySource: Send {
    fn sample(&mut self, include_processes: bool) -> PlatformResult<NativeMemorySnapshot>;
}

pub struct MemorySampler {
    system: System,
    #[cfg(windows)]
    processes: super::process_snapshot_windows::ProcessSnapshotReader,
}

impl Default for MemorySampler {
    fn default() -> Self {
        // Do not load CPU topology, disks, users, or process command lines at startup.
        Self {
            system: System::new(),
            #[cfg(windows)]
            processes: Default::default(),
        }
    }
}

impl MemorySource for MemorySampler {
    fn sample(&mut self, include_processes: bool) -> PlatformResult<NativeMemorySnapshot> {
        self.system
            .refresh_memory_specifics(MemoryRefreshKind::everything());
        if self.system.total_memory() == 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                "system memory counters are unavailable",
            ));
        }
        let processes = if include_processes {
            #[cfg(windows)]
            let processes = self
                .processes
                .read()
                .ok_or_else(|| {
                    PlatformError::new(
                        PlatformErrorCode::OperationFailed,
                        "private working set counters unavailable",
                    )
                })?
                .into_iter()
                .map(|row| ProcessMemory {
                    pid: row.counter.pid,
                    name: row.counter.name,
                    executable: row.counter.executable,
                    used_bytes: Some(row.private_working_set_bytes),
                    is_application: false,
                })
                .collect();
            #[cfg(not(windows))]
            let processes = {
                // Refresh only identity and the counters needed on this platform. Native macOS
                // footprint fills memory separately; RSS platforms need just one process pass.
                let refresh = ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet);
                #[cfg(not(target_os = "macos"))]
                let refresh = refresh.with_memory();
                self.system
                    .refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);
                self.system
                    .processes()
                    .iter()
                    .map(|(pid, process)| ProcessMemory {
                        pid: pid.as_u32(),
                        name: process.name().to_string_lossy().into_owned(),
                        executable: process.exe().map(PathBuf::from),
                        used_bytes: if cfg!(target_os = "macos") {
                            None
                        } else {
                            Some(process.memory())
                        },
                        is_application: false,
                    })
                    .collect::<Vec<_>>()
            };
            #[cfg(target_os = "macos")]
            let processes = {
                let mut rows = processes;
                super::memory_macos::fill(&mut rows);
                rows
            };
            Some(processes)
        } else {
            None
        };
        #[cfg(target_os = "macos")]
        let (used_bytes, free_bytes) = super::memory_macos::overview(self.system.total_memory())?;
        #[cfg(not(target_os = "macos"))]
        let (used_bytes, free_bytes) = (self.system.used_memory(), self.system.free_memory());
        Ok(NativeMemorySnapshot {
            total_bytes: self.system.total_memory(),
            used_bytes,
            free_bytes,
            swap_used_bytes: self.system.used_swap(),
            process_memory_kind: ProcessMemoryKind::native(),
            processes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sampling_can_skip_processes_and_observe_this_process() {
        let mut sampler = MemorySampler::default();
        let overview = sampler
            .sample(false)
            .expect("memory counters should be readable");
        assert!(overview.total_bytes > 0);
        assert!(overview.processes.is_none());
        let detailed = sampler
            .sample(true)
            .expect("process sampling should complete");
        assert!(detailed.processes.unwrap().iter().any(|process| {
            process.pid == std::process::id() && process.used_bytes.is_some_and(|bytes| bytes > 0)
        }));
    }
}
