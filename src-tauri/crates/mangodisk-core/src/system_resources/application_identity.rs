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
    // System WebKit XPC services share one image across unrelated host applications.
    // Their OS display names belong to individual PIDs, not to that executable.
    let shared_webkit = path.as_deref().is_some_and(|path| {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("com.apple.WebKit."))
            && path
                .ancestors()
                .any(|parent| parent.extension().is_some_and(|ext| ext == "xpc"))
    });
    ApplicationIdentity {
        id: running_identity::id(path.as_deref().filter(|_| !shared_webkit), pid),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_webkit_services_preserve_pid_names_without_merging_hosts() {
        let image = Path::new("/System/Library/Frameworks/WebKit.framework/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent");
        let first = identify(1, "DingTalk Web Content".into(), Some(image), None);
        let second = identify(2, "Safari Web Content".into(), Some(image), None);
        assert_ne!(first.id, second.id);
        assert_eq!(first.name, "DingTalk Web Content");
        assert_eq!(second.name, "Safari Web Content");
        assert_eq!(first.icon_path, second.icon_path);
        assert!(!first.can_quit);
    }
}
