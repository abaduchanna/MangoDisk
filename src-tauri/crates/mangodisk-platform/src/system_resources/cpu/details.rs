//! Optional CPU identity and frequency observations, independent of utilization.
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use serde::Serialize;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuIdentity {
    pub model: Option<String>,
    pub nominal_frequency_mhz: Option<f64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CpuFrequencySource {
    WindowsPerformance,
    ApplePerformanceStates,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuFrequency {
    pub average_mhz: Option<f64>,
    pub efficiency_mhz: Option<f64>,
    pub performance_mhz: Option<f64>,
    pub source: CpuFrequencySource,
}

pub struct CpuDetails {
    pub identity: CpuIdentity,
    // None means no demand, Ok(None) means a baseline or no active observation.
    pub frequency: Option<PlatformResult<Option<CpuFrequency>>>,
}

#[derive(Default)]
pub struct CpuDetailsReader {
    identity: Option<CpuIdentity>,
    #[cfg(windows)]
    frequency: Option<windows::FrequencyReader>,
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    frequency: Option<macos::FrequencyReader>,
}
impl CpuDetailsReader {
    pub fn read(&mut self, detailed: bool) -> CpuDetails {
        let identity = self.identity.get_or_insert_with(identity).clone();
        #[cfg(any(windows, all(target_os = "macos", target_arch = "aarch64")))]
        let frequency = if detailed {
            let reader = self.frequency.get_or_insert_with(Default::default);
            Some(reader.read())
        } else {
            if let Some(reader) = &mut self.frequency {
                reader.pause();
            }
            None
        };
        #[cfg(not(any(windows, all(target_os = "macos", target_arch = "aarch64"))))]
        let frequency = detailed.then(|| Err(unsupported()));
        CpuDetails {
            identity,
            frequency,
        }
    }
    pub fn reset(&mut self) {
        #[cfg(any(windows, all(target_os = "macos", target_arch = "aarch64")))]
        {
            if let Some(reader) = &mut self.frequency {
                reader.pause();
            }
        }
    }
}
fn unsupported() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Unsupported,
        "CPU frequency source unavailable",
    )
}
#[cfg(any(test, windows, target_os = "macos"))]
fn valid_mhz(value: f64) -> Option<f64> {
    (value.is_finite() && (1.0..=20_000.0).contains(&value)).then_some(value)
}
#[cfg(windows)]
fn identity() -> CpuIdentity {
    windows::identity()
}
#[cfg(target_os = "macos")]
fn identity() -> CpuIdentity {
    fn sysctl_bytes(name: &std::ffi::CStr) -> Option<Vec<u8>> {
        let mut size = 0;
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
            || size == 0
            || size > 1024
        {
            return None;
        }
        let mut bytes = vec![0; size];
        if unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                bytes.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        } != 0
        {
            return None;
        }
        bytes.truncate(size);
        Some(bytes)
    }
    let model = sysctl_bytes(c"machdep.cpu.brand_string")
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|text| text.trim_end_matches('\0').trim().to_owned())
        .filter(|text| !text.is_empty());
    // Apple Silicon's maximum DVFS state is not an advertised base frequency.
    #[cfg(target_arch = "aarch64")]
    let nominal_frequency_mhz = None;
    #[cfg(not(target_arch = "aarch64"))]
    let nominal_frequency_mhz = sysctl_bytes(c"hw.cpufrequency")
        .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
        .and_then(|bytes| valid_mhz(u64::from_ne_bytes(bytes) as f64 / 1_000_000.0));
    CpuIdentity {
        model,
        nominal_frequency_mhz,
    }
}
#[cfg(not(any(windows, target_os = "macos")))]
fn identity() -> CpuIdentity {
    CpuIdentity::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frequency_rejects_zero_nonfinite_and_implausible_values() {
        for value in [0.0, -1.0, f64::NAN, f64::INFINITY, 20_001.0] {
            assert_eq!(valid_mhz(value), None);
        }
        assert_eq!(valid_mhz(4774.29), Some(4774.29));
    }
}
