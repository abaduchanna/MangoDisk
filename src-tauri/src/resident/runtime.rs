use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use mangodisk_core::system_resources::{
    models::SystemResourceSnapshot, service::SystemResourceService,
};
use serde::Serialize;
use tauri::Emitter;

use super::{preferences::ResidentPreferences, PANEL_LABEL, TRAY_ID};

pub const READING_EVENT: &str = "resident-reading";

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ReadingStatus {
    Loading,
    Ready,
    Unavailable,
    Paused,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidentReading {
    pub revision: u64,
    pub status: ReadingStatus,
    pub snapshot: Option<SystemResourceSnapshot>,
}

pub struct ResidentState {
    pub preferences: Mutex<ResidentPreferences>,
    pub panel_open: AtomicBool,
    pub panel_ready: AtomicBool,
    pub panel_creation: Mutex<()>,
    pub panel_requested_at: Mutex<Option<std::time::Instant>>,
    pub reading: Mutex<ResidentReading>,
    wake: SyncSender<()>,
}

impl ResidentState {
    pub fn enabled(&self) -> bool {
        self.preferences
            .lock()
            .map(|value| value.enabled)
            .unwrap_or(false)
    }

    fn publish(
        &self,
        result: mangodisk_core::CoreResult<SystemResourceSnapshot>,
    ) -> Option<ResidentReading> {
        // Preference changes and publication share this lock order. Disabling while an OS
        // query is in flight must leave a paused snapshot, even if that query later succeeds.
        let preferences = self.preferences.lock().ok()?;
        if !preferences.enabled {
            return None;
        }
        let mut reading = self.reading.lock().ok()?;
        reading.revision += 1;
        match result {
            Ok(snapshot) => {
                if reading.status == ReadingStatus::Unavailable {
                    log::info!("resident_sampling_recovered");
                }
                reading.status = ReadingStatus::Ready;
                reading.snapshot = Some(snapshot);
            }
            Err(error) => {
                if reading.status != ReadingStatus::Unavailable {
                    log::warn!("resident_sampling_failed code={:?}", error.code());
                }
                reading.status = ReadingStatus::Unavailable;
            }
        }
        Some(reading.clone())
    }

    pub fn wake(&self) {
        // One pending wake is sufficient. Repeated clicks never create parallel scans
        // or an unbounded queue while native process enumeration is in flight.
        let _ = self.wake.try_send(());
    }
}

pub fn start(app: &tauri::AppHandle, preferences: ResidentPreferences) -> Arc<ResidentState> {
    let (wake, receiver) = mpsc::sync_channel(1);
    let state = Arc::new(ResidentState {
        preferences: Mutex::new(preferences),
        panel_open: AtomicBool::new(false),
        panel_ready: AtomicBool::new(false),
        panel_creation: Mutex::new(()),
        panel_requested_at: Mutex::new(None),
        reading: Mutex::new(ResidentReading {
            revision: 0,
            status: if preferences.enabled {
                ReadingStatus::Loading
            } else {
                ReadingStatus::Paused
            },
            snapshot: None,
        }),
        wake,
    });
    let worker = state.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let mut service = SystemResourceService::default();
        let mut icons_warmed = false;
        loop {
            let enabled = worker.enabled();
            let detailed = worker.panel_open.load(Ordering::Relaxed);
            if !enabled {
                icons_warmed = false;
            }
            if enabled {
                let started = std::time::Instant::now();
                let timestamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                // One detailed background sample prepares app icons without creating
                // a WebView or blocking launch. Hidden panels then return to cheap samples.
                let result = service.sample(detailed || !icons_warmed, timestamp);
                if started.elapsed() > Duration::from_secs(2) {
                    log::debug!(
                        "resident_sample_slow elapsed_ms={}",
                        started.elapsed().as_millis()
                    );
                }
                let Some(reading) = worker.publish(result) else {
                    continue;
                };
                // Never publish a completed in-flight sample as active after the user disabled monitoring.
                if worker.enabled() {
                    update_tray(&app, &worker, &reading);
                    if detailed {
                        let _ = app.emit_to(PANEL_LABEL, READING_EVENT, &reading);
                    }
                }
                // Publish measurements first: icon I/O must never hold back the
                // first IPC reading or an already-open panel's sample event.
                if !icons_warmed && worker.enabled() {
                    if let Some(summary) = reading
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.processes.as_ref())
                    {
                        super::application_icons::warm(&app, summary);
                        icons_warmed = true;
                    }
                }
            }
            match sampling_interval(worker.enabled(), worker.panel_open.load(Ordering::Relaxed)) {
                Some(interval) => {
                    if receiver.recv_timeout(interval) == Err(mpsc::RecvTimeoutError::Disconnected)
                    {
                        break;
                    }
                }
                None => {
                    if receiver.recv().is_err() {
                        break;
                    }
                }
            }
        }
    });
    state
}

fn update_tray(app: &tauri::AppHandle, state: &ResidentState, reading: &ResidentReading) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let show = state
        .preferences
        .lock()
        .map(|p| p.show_memory)
        .unwrap_or(false);
    let title = if show {
        match (&reading.snapshot, reading.status) {
            (Some(snapshot), ReadingStatus::Ready) => format!("{}%", snapshot.memory.used_percent),
            _ => "—".into(),
        }
    } else {
        String::new()
    };
    // Windows does not support tray titles; retain the same summary in its tooltip.
    #[cfg(target_os = "macos")]
    if let Err(error) = tray.set_title(Some(&title)) {
        log::debug!("resident_title_update_failed error={error}");
    }
    let _ = tray.set_tooltip(Some(if title.is_empty() {
        "MangoDisk".into()
    } else {
        format!("MangoDisk · {title}")
    }));
}

pub fn sampling_interval(enabled: bool, panel_open: bool) -> Option<Duration> {
    enabled.then(|| Duration::from_secs(if panel_open { 3 } else { 15 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> (ResidentState, mpsc::Receiver<()>) {
        let (wake, receiver) = mpsc::sync_channel(1);
        (
            ResidentState {
                preferences: Mutex::new(ResidentPreferences::default()),
                panel_open: AtomicBool::new(false),
                panel_ready: AtomicBool::new(false),
                panel_creation: Mutex::new(()),
                panel_requested_at: Mutex::new(None),
                reading: Mutex::new(ResidentReading {
                    revision: 0,
                    status: ReadingStatus::Loading,
                    snapshot: None,
                }),
                wake,
            },
            receiver,
        )
    }

    fn snapshot(time: u64) -> SystemResourceSnapshot {
        use mangodisk_core::system_resources::models::MemoryOverview;
        SystemResourceSnapshot {
            schema_version: 1,
            sampled_at_ms: time,
            memory: MemoryOverview {
                total_bytes: 100,
                used_bytes: 50,
                free_bytes: 50,
                swap_used_bytes: 0,
                used_percent: 50,
            },
            processes: None,
        }
    }

    #[test]
    fn failed_samples_preserve_last_success_and_recovery_advances_revision() {
        let (state, _receiver) = state();
        assert_eq!(state.publish(Ok(snapshot(1))).unwrap().revision, 1);
        let invalid = mangodisk_core::CoreError::operation_failed("memory unavailable");
        let failed = state.publish(Err(invalid)).unwrap();
        assert_eq!(failed.status, ReadingStatus::Unavailable);
        assert_eq!(failed.snapshot.unwrap().sampled_at_ms, 1);
        let recovered = state.publish(Ok(snapshot(3))).unwrap();
        assert_eq!(recovered.status, ReadingStatus::Ready);
        assert_eq!(recovered.revision, 3);
        assert_eq!(recovered.snapshot.unwrap().sampled_at_ms, 3);
    }

    #[test]
    fn disabling_rejects_in_flight_samples_and_repeated_wakes_are_coalesced() {
        let (state, receiver) = state();
        state.preferences.lock().unwrap().enabled = false;
        assert!(state.publish(Ok(snapshot(1))).is_none());
        assert_eq!(state.reading.lock().unwrap().revision, 0);
        for _ in 0..100 {
            state.wake();
        }
        assert!(receiver.try_recv().is_ok());
        assert_eq!(receiver.try_recv(), Err(mpsc::TryRecvError::Empty));
        state.preferences.lock().unwrap().enabled = true;
        assert_eq!(
            state.publish(Ok(snapshot(2))).unwrap().status,
            ReadingStatus::Ready
        );
    }

    #[test]
    fn disabled_monitoring_sleeps_until_woken_and_hidden_panels_sample_slowly() {
        assert_eq!(sampling_interval(false, true), None);
        assert_eq!(sampling_interval(false, false), None);
        assert_eq!(
            sampling_interval(true, false),
            Some(Duration::from_secs(15))
        );
        assert_eq!(sampling_interval(true, true), Some(Duration::from_secs(3)));
    }
}
