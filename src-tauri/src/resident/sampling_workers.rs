//! Bounded native workers: a slow disk cannot delay CPU, network, or window callbacks.
use mangodisk_core::{
    system_resources::{
        metrics::MetricId,
        models::{ProcessCpuSummary, SystemResourceSnapshot},
        process_cpu::ProcessCpuService,
        service::SystemResourceService,
    },
    CoreResult,
};
use mangodisk_platform::system_resources::{
    cpu::{CpuReader, CpuSample},
    disk::{ResourceVolume, VolumeCapacity},
    network::{InterfaceSample, NetworkReader},
};
use std::{
    sync::mpsc::{self, SyncSender},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use super::sampling_schedule::Demand;

pub enum Observation {
    Unsupported,
    Cpu {
        counters: CpuSample,
        processes: Option<CoreResult<Option<ProcessCpuSummary>>>,
    },
    Memory(SystemResourceSnapshot),
    Network(Vec<InterfaceSample>),
    Disk {
        volumes: Vec<ResourceVolume>,
        selected: Option<(ResourceVolume, VolumeCapacity)>,
    },
}

pub enum Request {
    Sample { generation: u64, demand: Demand },
    ReleaseCpu,
}
pub struct Completion {
    pub metric: MetricId,
    pub generation: u64,
    pub monotonic_ms: u64,
    pub timestamp_ms: u64,
    pub duration_ms: u64,
    pub result: CoreResult<Observation>,
}
pub enum SamplingEvent {
    Wake,
    DiskIo(super::disk_activity::Completion),
    Completed(Box<Completion>),
}

pub fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn start(
    metric: MetricId,
    origin: Instant,
    events: SyncSender<SamplingEvent>,
) -> SyncSender<Request> {
    let (sender, requests) = mpsc::sync_channel::<Request>(1);
    std::thread::spawn(move || {
        let mut cpu = (metric == MetricId::Cpu).then(|| CpuWorker::new(origin));
        let mut generation = None;
        let mut memory = (metric == MetricId::Memory).then(SystemResourceService::default);
        let mut network = (metric == MetricId::Network).then(NetworkReader::default);
        while let Ok(request) = requests.recv() {
            let Request::Sample {
                generation: requested_generation,
                demand,
            } = request
            else {
                if let Some(reader) = cpu.as_mut() {
                    reader.reset();
                }
                generation = None;
                continue;
            };
            if generation.replace(requested_generation) != Some(requested_generation) {
                if let Some(reader) = cpu.as_mut() {
                    // Re-enabling monitoring must prime a fresh interval even
                    // when the pause was shorter than the normal expiry limit.
                    reader.reset();
                }
            }
            let started = Instant::now();
            let timestamp_ms = timestamp_ms();
            let result = sample(
                metric,
                &demand,
                cpu.as_mut(),
                memory.as_mut(),
                network.as_mut(),
                timestamp_ms,
            );
            let completion = Completion {
                metric,
                generation: requested_generation,
                monotonic_ms: origin.elapsed().as_millis() as u64,
                timestamp_ms,
                duration_ms: started.elapsed().as_millis() as u64,
                result,
            };
            if events
                .send(SamplingEvent::Completed(Box::new(completion)))
                .is_err()
            {
                break;
            }
        }
    });
    sender
}

fn sample(
    metric: MetricId,
    demand: &Demand,
    cpu: Option<&mut CpuWorker>,
    memory: Option<&mut SystemResourceService>,
    network: Option<&mut NetworkReader>,
    timestamp_ms: u64,
) -> CoreResult<Observation> {
    use mangodisk_platform::system_resources::disk;
    Ok(match metric {
        MetricId::Cpu => cpu
            .expect("CPU worker owns its sampler")
            .sample(demand.detailed)?,
        MetricId::Memory => Observation::Memory(
            memory
                .expect("memory worker owns its sampler")
                .sample(demand.detailed, timestamp_ms)?,
        ),
        MetricId::Network => {
            Observation::Network(network.expect("network worker owns its sampler").read()?)
        }
        MetricId::Disk => {
            let volumes = disk::list()?;
            let selected = mangodisk_core::system_resources::disk::select(
                &volumes,
                demand.selection.as_deref(),
            )
            .map(|volume| disk::capacity(volume).map(|capacity| (volume.clone(), capacity)))
            .transpose()?;
            Observation::Disk { volumes, selected }
        }
    })
}

struct CpuWorker {
    overview: CpuReader,
    processes: Option<ProcessCpuService>,
    origin: Instant,
}
impl CpuWorker {
    fn new(origin: Instant) -> Self {
        Self {
            overview: Default::default(),
            processes: None,
            origin,
        }
    }
    fn reset(&mut self) {
        self.overview.reset();
        // Drop native maps and handles, when resident monitoring is disabled.
        self.processes = None;
    }
    fn sample(&mut self, detailed: bool) -> CoreResult<Observation> {
        let counters = match self.overview.read() {
            Ok(counters) => counters,
            Err(error) if error.code() == mangodisk_platform::PlatformErrorCode::Unsupported => {
                return Ok(Observation::Unsupported)
            }
            Err(error) => return Err(error.into()),
        };
        let processes = if detailed {
            Some(
                self.processes
                    .get_or_insert_with(ProcessCpuService::default)
                    .sample(self.origin.elapsed().as_millis() as u64),
            )
        } else {
            self.processes = None;
            None
        };
        Ok(Observation::Cpu {
            counters,
            processes,
        })
    }
}
