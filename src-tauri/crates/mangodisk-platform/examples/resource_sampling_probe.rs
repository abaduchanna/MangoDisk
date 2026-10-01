//! Reproducible sensor-cost probe; emits aggregate measurements without process identities.
use mangodisk_platform::system_resources::{
    cpu::CpuReader,
    memory::{MemorySampler, MemorySource},
    process_cpu::{ProcessCpuSampler, ProcessCpuSource},
};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("overview");
    let seconds: u64 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(60);
    assert!(matches!(
        mode,
        "overview" | "memory" | "cpu" | "cpu-background"
    ));
    assert!((10..=3600).contains(&seconds));
    let mut cpu: CpuReader = Default::default();
    let mut memory = MemorySampler::default();
    let mut processes = (mode.starts_with("cpu")).then(ProcessCpuSampler::default);
    let mut observer = System::new();
    let pid = Pid::from_u32(std::process::id());
    let refresh = |observer: &mut System| {
        observer.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let process = observer.process(pid).expect("probe process is readable");
        (process.accumulated_cpu_time(), process.memory())
    };
    let mut sample = |cpu: &mut CpuReader, memory: &mut MemorySampler, tick: u64| {
        cpu.read().expect("CPU overview is readable");
        if let Some(sampler) = processes.as_mut() {
            if mode != "cpu-background" || tick.is_multiple_of(2) {
                sampler.sample().expect("process CPU is readable");
            }
        }
        if tick.is_multiple_of(3) {
            memory.sample(mode == "memory").expect("memory is readable");
        }
    };
    for tick in 0..5 {
        sample(&mut cpu, &mut memory, tick);
        std::thread::sleep(Duration::from_secs(1));
    }
    let (before_cpu, before_rss) = refresh(&mut observer);
    let before_helpers = helper_cpu_ms();
    let started = Instant::now();
    let mut durations = Vec::new();
    let mut rss_min = before_rss;
    let mut rss_max = before_rss;
    for tick in 0..seconds {
        let at = Instant::now();
        sample(&mut cpu, &mut memory, tick);
        durations.push(at.elapsed().as_micros() as u64);
        let (_, rss) = refresh(&mut observer);
        rss_min = rss_min.min(rss);
        rss_max = rss_max.max(rss);
        std::thread::sleep(Duration::from_secs(1).saturating_sub(at.elapsed()));
    }
    let (after_cpu, after_rss) = refresh(&mut observer);
    let helper_cpu = helper_cpu_ms().saturating_sub(before_helpers);
    durations.sort_unstable();
    println!(
        "{}",
        serde_json::json!({
            "schemaVersion": 1, "mode": mode, "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH, "samples": durations.len(),
            "elapsedSeconds": started.elapsed().as_secs_f64(),
            "cpuSingleCorePercent": (after_cpu - before_cpu) as f64 / started.elapsed().as_secs_f64() / 10.0,
            "helperCpuSingleCorePercent": helper_cpu as f64 / started.elapsed().as_secs_f64() / 10.0,
            "cpuIncludingHelpersSingleCorePercent": (after_cpu - before_cpu + helper_cpu) as f64 / started.elapsed().as_secs_f64() / 10.0,
            "rssStartBytes": before_rss, "rssEndBytes": after_rss,
            "rssMinBytes": rss_min, "rssMaxBytes": rss_max,
            "sampleP50Micros": durations[durations.len() / 2],
            "sampleP95Micros": durations[(durations.len() - 1) * 95 / 100],
        })
    );
}

#[cfg(target_os = "macos")]
fn helper_cpu_ms() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    if unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, usage.as_mut_ptr()) } != 0 {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    let millis = |time: libc::timeval| time.tv_sec as u64 * 1000 + time.tv_usec as u64 / 1000;
    millis(usage.ru_utime) + millis(usage.ru_stime)
}
#[cfg(not(target_os = "macos"))]
fn helper_cpu_ms() -> u64 {
    0
}
