pub mod client;
mod cpu;
pub mod deployment;
mod diagnostics;
mod pawnio_removal;
mod security;
mod server;

use std::{ffi::OsString, io};

pub fn run(installer: &[u8], module: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let result = execute(installer, module);
    if let Err(error) = &result {
        diagnostics::error(format!("privileged_operation_failed error={error}"));
    }
    result
}

fn execute(installer: &[u8], module: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    match arguments.as_slice() {
        [] => server::run(),
        [action] if action == "--install-cpu-temperature" => deployment::install(installer, module),
        [action] if action == "--uninstall" => deployment::uninstall(false),
        [action] if action == "--uninstall-with-pawnio" => deployment::uninstall(true),
        [action] if action == "--health" => {
            println!(
                "{}",
                serde_json::to_string(&client::request(
                    mangodisk_privileged_protocol::Operation::Health
                )?)?
            );
            Ok(())
        }
        [action] if action == "--temperature" => {
            println!(
                "{}",
                serde_json::to_string(&client::request(
                    mangodisk_privileged_protocol::Operation::CpuTemperature
                )?)?
            );
            Ok(())
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unknown privileged service command",
        )
        .into()),
    }
}
