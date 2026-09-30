//! Basic operating-system identity without collecting host or user identifiers.

use sysinfo::System;

pub struct SystemIdentity {
    pub operating_system: &'static str,
    pub version: Option<String>,
    pub architecture: String,
}

pub fn current() -> SystemIdentity {
    SystemIdentity {
        operating_system: match std::env::consts::OS {
            "macos" => "macOS",
            "windows" => "Windows",
            "linux" => "Linux",
            other => other,
        },
        version: System::os_version(),
        architecture: System::cpu_arch(),
    }
}
