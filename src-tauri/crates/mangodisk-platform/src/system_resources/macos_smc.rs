//! Read-only Apple SMC transport shared by identified CPU and GPU sensors.
use crate::{PlatformError, PlatformErrorCode, PlatformResult};
use core_foundation::dictionary::CFMutableDictionaryRef;
use std::{ffi::c_void, mem::size_of};
fn unsupported() -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::Unsupported,
        "Apple SMC sensors unavailable",
    )
}
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOServiceMatching(name: *const std::ffi::c_char) -> CFMutableDictionaryRef;
    fn IOServiceGetMatchingServices(
        port: u32,
        matching: CFMutableDictionaryRef,
        iterator: *mut u32,
    ) -> i32;
    fn IOIteratorNext(iterator: u32) -> u32;
    fn IORegistryEntryGetName(entry: u32, name: *mut std::ffi::c_char) -> i32;
    fn IOObjectRelease(object: u32) -> i32;
    fn IOServiceOpen(service: u32, task: u32, kind: u32, connection: *mut u32) -> i32;
    fn IOServiceClose(connection: u32) -> i32;
    fn IOConnectCallStructMethod(
        connection: u32,
        selector: u32,
        input: *const c_void,
        input_size: usize,
        output: *mut c_void,
        output_size: *mut usize,
    ) -> i32;
}
unsafe extern "C" {
    fn mach_task_self() -> u32;
}

#[repr(C)]
#[derive(Default)]
struct Version {
    major: u8,
    minor: u8,
    build: u8,
    reserved: u8,
    release: u16,
}
#[repr(C)]
#[derive(Default)]
struct Limits {
    version: u16,
    length: u16,
    cpu: u32,
    gpu: u32,
    memory: u32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct KeyInfo {
    size: u32,
    kind: u32,
    attributes: u8,
}
#[repr(C)]
#[derive(Default)]
struct Packet {
    key: u32,
    version: Version,
    limits: Limits,
    info: KeyInfo,
    result: u8,
    status: u8,
    command: u8,
    index: u32,
    bytes: [u8; 32],
}
struct Object(u32);
impl Drop for Object {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe {
                IOObjectRelease(self.0);
            }
        }
    }
}
struct Connection(u32);
impl Drop for Connection {
    fn drop(&mut self) {
        unsafe {
            IOServiceClose(self.0);
        }
    }
}
impl Connection {
    fn open() -> PlatformResult<Self> {
        let mut iterator = 0;
        // Matching consumes the dictionary; the iterator and each service are owned.
        let code = unsafe {
            IOServiceGetMatchingServices(0, IOServiceMatching(c"AppleSMC".as_ptr()), &mut iterator)
        };
        let iterator = Object(iterator);
        if code != 0 {
            return Err(native_error("enumerate", code));
        }
        loop {
            let service = Object(unsafe { IOIteratorNext(iterator.0) });
            if service.0 == 0 {
                return Err(unsupported());
            }
            let mut name = [0i8; 128];
            if unsafe { IORegistryEntryGetName(service.0, name.as_mut_ptr()) } != 0 {
                continue;
            }
            let name = name
                .into_iter()
                .take_while(|byte| *byte != 0)
                .map(|byte| byte as u8)
                .collect::<Vec<_>>();
            let expected = if cfg!(target_arch = "aarch64") {
                b"AppleSMCKeysEndpoint".as_slice()
            } else {
                b"AppleSMC".as_slice()
            };
            if name != expected {
                continue;
            }
            let mut connection = 0;
            let code = unsafe { IOServiceOpen(service.0, mach_task_self(), 0, &mut connection) };
            if code != 0 {
                return Err(native_error("open", code));
            }
            if connection == 0 {
                return Err(unsupported());
            }
            return Ok(Self(connection));
        }
    }
    fn query(&self, input: Packet) -> PlatformResult<Option<Packet>> {
        let mut output = Packet::default();
        let mut size = size_of::<Packet>();
        // Packet is the fixed C ABI and both buffers stay alive throughout the call.
        let code = unsafe {
            IOConnectCallStructMethod(
                self.0,
                2,
                (&input as *const Packet).cast(),
                size_of::<Packet>(),
                (&mut output as *mut Packet).cast(),
                &mut size,
            )
        };
        if code != 0 {
            return Err(native_error("read", code));
        }
        if size != size_of::<Packet>() {
            return Err(PlatformError::new(
                PlatformErrorCode::InvalidData,
                format!("SMC response size={size}"),
            ));
        }
        if output.result == 132 {
            return Ok(None);
        }
        if output.result != 0 {
            return Err(PlatformError::new(
                PlatformErrorCode::OperationFailed,
                format!(
                    "SMC read key={:08x} result={} status={}",
                    input.key, output.result, output.status
                ),
            ));
        }
        Ok(Some(output))
    }
}
fn native_error(stage: &str, code: i32) -> PlatformError {
    PlatformError::new(
        PlatformErrorCode::OperationFailed,
        format!("SMC {stage} native_code=0x{:08x}", code as u32),
    )
}

pub(crate) struct TemperatureSensors {
    connection: Connection,
    sensors: Vec<(u32, KeyInfo)>,
}
impl TemperatureSensors {
    pub(crate) fn new(keys: &[[u8; 4]]) -> PlatformResult<Self> {
        let connection = Connection::open()?;
        let sensors = discover(&connection, keys)?;
        Ok(Self {
            connection,
            sensors,
        })
    }
    #[cfg(target_arch = "x86_64")]
    pub(crate) fn select(&mut self, keys: &[[u8; 4]]) -> PlatformResult<()> {
        self.sensors = discover(&self.connection, keys)?;
        Ok(())
    }
    pub(crate) fn len(&self) -> usize {
        self.sensors.len()
    }
    pub(crate) fn read(&self) -> PlatformResult<Vec<f64>> {
        // Every selected sensor must remain readable; changing the denominator
        // during a sample would silently change an average's meaning.
        self.sensors
            .iter()
            .map(|&(key, info)| {
                let packet = self
                    .connection
                    .query(Packet {
                        key,
                        info,
                        command: 5,
                        ..Default::default()
                    })?
                    .ok_or_else(|| {
                        PlatformError::new(
                            PlatformErrorCode::OperationFailed,
                            format!("SMC sensor disappeared key={key:08x}"),
                        )
                    })?;
                decode(info, &packet.bytes)
                    .filter(|value| value.is_finite() && *value > 0.0 && *value <= 150.0)
                    .ok_or_else(|| {
                        PlatformError::new(
                            PlatformErrorCode::InvalidData,
                            format!("SMC temperature invalid key={key:08x}"),
                        )
                    })
            })
            .collect()
    }
}
fn discover(connection: &Connection, keys: &[[u8; 4]]) -> PlatformResult<Vec<(u32, KeyInfo)>> {
    let mut sensors = Vec::new();
    for key in keys {
        let key = u32::from_be_bytes(*key);
        let Some(packet) = connection.query(Packet {
            key,
            command: 9,
            ..Default::default()
        })?
        else {
            continue;
        };
        if supported_encoding(packet.info) {
            sensors.push((key, packet.info));
        }
    }
    Ok(sensors)
}

fn supported_encoding(info: KeyInfo) -> bool {
    (info.kind == u32::from_be_bytes(*b"flt ") && info.size == 4)
        || (info.kind == u32::from_be_bytes(*b"sp78") && info.size == 2)
}
fn decode(info: KeyInfo, bytes: &[u8; 32]) -> Option<f64> {
    if !supported_encoding(info) {
        return None;
    }
    if info.size == 4 {
        Some(f64::from(f32::from_le_bytes(bytes[..4].try_into().ok()?)))
    } else {
        Some(f64::from(i16::from_be_bytes(bytes[..2].try_into().ok()?)) / 256.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smc_abi_and_signed_decoding_are_checked() {
        assert_eq!(size_of::<Packet>(), 80);
        let mut bytes = [0; 32];
        bytes[..2].copy_from_slice(&(-128i16).to_be_bytes());
        assert_eq!(
            decode(
                KeyInfo {
                    kind: u32::from_be_bytes(*b"sp78"),
                    size: 2,
                    ..Default::default()
                },
                &bytes
            ),
            Some(-0.5)
        );
        bytes[..4].copy_from_slice(&48.5f32.to_le_bytes());
        assert_eq!(
            decode(
                KeyInfo {
                    kind: u32::from_be_bytes(*b"flt "),
                    size: 4,
                    ..Default::default()
                },
                &bytes
            ),
            Some(48.5)
        );
        assert!(decode(
            KeyInfo {
                kind: u32::from_be_bytes(*b"flt "),
                size: 32,
                ..Default::default()
            },
            &bytes
        )
        .is_none());
    }
}
