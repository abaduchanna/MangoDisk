//! Shared presentation identity for CPU and memory, independent of measurement and ranking.
use super::models::ApplicationIdentity;
use crate::applications::running_identity;
use std::path::Path;

pub(super) fn identify(
    pid: u32,
    name: String,
    path: Option<&Path>,
    own_path: Option<&Path>,
) -> ApplicationIdentity {
    let path = path.map(running_identity::application_path);
    let is_bundle = path.as_deref().is_some_and(running_identity::is_bundle);
    ApplicationIdentity {
        id: running_identity::id(path.as_deref(), pid),
        name: path
            .as_deref()
            .filter(|_| is_bundle)
            .and_then(Path::file_stem)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or(name),
        icon_path: path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
        is_bundle,
        can_quit: path
            .as_deref()
            .is_some_and(|path| running_identity::can_quit(path, own_path)),
        process_count: 0,
    }
}
