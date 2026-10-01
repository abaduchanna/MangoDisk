//! Native footprint and physical-memory counters; unavailable values are never RSS substitutes.
use super::memory::ProcessMemory;
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use std::{collections::HashMap, mem::MaybeUninit, path::PathBuf};

pub(super) fn overview(total: u64) -> PlatformResult<(u64, u64)> {
    static HOST: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    unsafe extern "C" {
        fn mach_host_self() -> libc::mach_port_t;
    }
    let host = *HOST.get_or_init(|| unsafe { mach_host_self() });
    let mut counters = MaybeUninit::<libc::vm_statistics64>::zeroed();
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let status = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            counters.as_mut_ptr().cast(),
            &mut count,
        )
    };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let required = (std::mem::offset_of!(libc::vm_statistics64, internal_page_count)
        + std::mem::size_of::<libc::natural_t>())
        / std::mem::size_of::<libc::integer_t>();
    if status != libc::KERN_SUCCESS || count < required as u32 || page_size <= 0 {
        return Err(PlatformError::new(PlatformErrorCode::OperationFailed,
            format!("macOS memory counters unavailable status={status} count={count} page_size={page_size}")));
    }
    let counters = unsafe { counters.assume_init() };
    Ok(physical_usage(
        total,
        page_size as u64,
        counters.free_count.into(),
        counters.speculative_count.into(),
        counters.external_page_count.into(),
    ))
}

fn physical_usage(
    total: u64,
    page_size: u64,
    free: u64,
    speculative: u64,
    file_backed: u64,
) -> (u64, u64) {
    // Inactive anonymous pages still belong to apps. Subtract native free and file-backed
    // pages from installed RAM instead of counting only active pages. Speculative pages
    // are already included in free_count; subtract them only for the free-page display.
    (
        total.saturating_sub(free.saturating_add(file_backed).saturating_mul(page_size)),
        free.saturating_sub(speculative)
            .saturating_mul(page_size)
            .min(total),
    )
}

pub(super) fn fill(processes: &mut [ProcessMemory]) {
    let apps = objc2::rc::autoreleasepool(|_| {
        let apps = objc2_app_kit::NSWorkspace::sharedWorkspace().runningApplications();
        (0..apps.count())
            .map(|i| {
                let app = apps.objectAtIndex(i);
                let name = app.localizedName().map(|value| value.to_string());
                let path = app
                    .executableURL()
                    .and_then(|url| url.path())
                    .map(|value| PathBuf::from(value.to_string()));
                (app.processIdentifier() as u32, (name, path))
            })
            .collect::<HashMap<_, _>>()
    });
    for process in processes {
        let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
        let status = unsafe {
            libc::proc_pid_rusage(
                process.pid as i32,
                libc::RUSAGE_INFO_V2,
                usage.as_mut_ptr().cast(),
            )
        };
        process.used_bytes =
            (status == 0).then(|| unsafe { usage.assume_init() }.ri_phys_footprint);
        if let Some((name, path)) = apps.get(&process.pid) {
            process.is_application = true;
            if let Some(name) = name {
                process.name.clone_from(name);
            }
            if process.executable.is_none() {
                process.executable.clone_from(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usage_includes_inactive_app_pages_and_counts_speculative_pages_once() {
        assert_eq!(physical_usage(1000, 10, 5, 2, 20), (750, 30));
        assert_eq!(physical_usage(10, u64::MAX, 5, 20, 3), (0, 0));
    }
    #[test]
    fn own_footprint_is_readable_and_invalid_pid_remains_unknown() {
        let mut processes = vec![ProcessMemory {
            pid: std::process::id(),
            name: "test".into(),
            executable: None,
            used_bytes: None,
            is_application: false,
        }];
        fill(&mut processes);
        assert!(processes[0].used_bytes.is_some_and(|value| value > 0));
        processes[0].pid = u32::MAX;
        fill(&mut processes);
        assert_eq!(processes[0].used_bytes, None);
    }
}
