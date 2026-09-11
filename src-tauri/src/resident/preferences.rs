use super::{
    diagnostics::Failure,
    runtime::{ReadingStatus, ResidentState},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const FILE: &str = "resident.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResidentPreferences {
    pub schema_version: u32,
    pub enabled: bool,
    pub show_memory: bool,
}

impl Default for ResidentPreferences {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: true,
            show_memory: true,
        }
    }
}

pub fn load(app: &tauri::AppHandle) -> ResidentPreferences {
    let value = match app.store_builder(FILE).disable_auto_save().build() {
        Ok(store) => store.get("preferences"),
        Err(error) => {
            Failure::record("preferences_load", &error);
            return ResidentPreferences::default();
        }
    };
    match value {
        None => ResidentPreferences::default(),
        Some(value) => match serde_json::from_value::<ResidentPreferences>(value) {
            Ok(preferences) if preferences.schema_version == 1 => preferences,
            _ => {
                log::warn!("resident_preferences_invalid");
                ResidentPreferences::default()
            }
        },
    }
}

fn save(app: &tauri::AppHandle, preferences: ResidentPreferences) -> Result<(), Failure> {
    if preferences.schema_version != 1 {
        return Err(Failure::state("preferences_version"));
    }
    let store = app
        .store_builder(FILE)
        .disable_auto_save()
        .build()
        .map_err(|error| Failure::record("preferences_open", &error))?;
    let previous = store.get("preferences");
    store.set(
        "preferences",
        serde_json::to_value(preferences)
            .map_err(|error| Failure::record("preferences_encode", &error))?,
    );
    if let Err(error) = store.save() {
        // Restore the plugin cache too: otherwise a later save could persist a
        // preference that the UI correctly reported as rejected.
        if let Some(previous) = previous {
            store.set("preferences", previous);
        } else {
            store.delete("preferences");
        }
        return Err(Failure::record("preferences_save", &error));
    }
    Ok(())
}

/// Desktop policy stays here; IPC only transports the requested preference value.
pub fn apply(app: &tauri::AppHandle, preferences: ResidentPreferences) -> Result<(), Failure> {
    let state = app.state::<Arc<ResidentState>>();
    let mut current = state
        .preferences
        .lock()
        .map_err(|_| Failure::state("preferences_lock"))?;
    if preferences.schema_version != 1 {
        return Err(Failure::state("preferences_version"));
    }
    if !preferences.enabled {
        super::main_window::open(
            app,
            super::main_window::Destination::Main,
            "disable_resident",
        )
        .map_err(|error| Failure::record("preferences_foreground", &error))?;
    }
    let tray = app
        .tray_by_id(super::TRAY_ID)
        .ok_or_else(|| Failure::state("preferences_tray_missing"))?;
    // A rejected native update must not be reported as a persisted success.
    tray.set_visible(preferences.enabled)
        .map_err(|error| Failure::record("preferences_visibility", &error))?;
    if let Err(error) = save(app, preferences) {
        if let Err(rollback) = tray.set_visible(current.enabled) {
            Failure::record("preferences_visibility_rollback", &rollback);
        }
        return Err(error);
    }
    *current = preferences;
    if !preferences.enabled {
        let mut reading = state
            .reading
            .lock()
            .map_err(|_| Failure::state("preferences_reading_lock"))?;
        reading.revision += 1;
        reading.status = ReadingStatus::Paused;
        reading.snapshot = None;
    }
    drop(current);
    if preferences.enabled {
        super::panel::prewarm(app);
    } else {
        super::panel::hide(app);
    }
    state.wake();
    log::info!(
        "resident_preferences_saved enabled={} show_memory={}",
        preferences.enabled,
        preferences.show_memory
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_have_an_explicit_version_and_require_boolean_fields() {
        let defaults = serde_json::to_value(ResidentPreferences::default()).unwrap();
        assert_eq!(
            defaults,
            serde_json::json!({"schemaVersion":1,"enabled":true,"showMemory":true})
        );
        assert!(serde_json::from_value::<ResidentPreferences>(
            serde_json::json!({"schemaVersion":1,"enabled":"true","showMemory":false})
        )
        .is_err());
    }
}
