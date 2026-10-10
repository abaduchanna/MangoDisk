use super::{unsupported, CpuTemperature, CpuTemperatureKind, CpuTemperatureSource};
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use mangodisk_privileged::windows::{client, deployment};
use mangodisk_privileged_protocol::{CapabilityStatus, Operation, Reply};

pub(super) struct Reader;
impl Reader {
    pub(super) fn new() -> PlatformResult<Self> {
        if !deployment::hardware_supported() {
            return Err(unsupported());
        }
        Ok(Self)
    }
    pub(super) fn read(&mut self) -> PlatformResult<CpuTemperature> {
        let response = client::request(Operation::CpuTemperature).map_err(|error| {
            PlatformError::new(
                PlatformErrorCode::Io,
                format!("privileged CPU temperature unavailable: {error}"),
            )
        })?;
        match response.reply {
            Reply::CpuTemperature {
                status: CapabilityStatus::Ready,
                reading: Some(reading),
            } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                reading.validate(now).map_err(|error| {
                    PlatformError::new(PlatformErrorCode::InvalidData, error.to_string())
                })?;
                Ok(CpuTemperature {
                    celsius: reading.celsius,
                    kind: CpuTemperatureKind::Package,
                    source: CpuTemperatureSource::IntelDtsPawnIo,
                    sensor_count: 1,
                    sampled_at_ms: Some(reading.sampled_at_ms),
                })
            }
            Reply::CpuTemperature {
                status: CapabilityStatus::UnsupportedHardware,
                ..
            } => Err(unsupported()),
            _ => Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                "privileged CPU sensor unavailable",
            )),
        }
    }
}
