//! Native surfaces reuse the same locale resources as Vue.
use mangodisk_core::system_resources::metrics::{MetricId, MetricStatus};
use serde_json::Value;
use std::sync::OnceLock;
use tauri_plugin_store::StoreExt;

pub struct Labels {
    pub locale: String,
    messages: &'static Value,
}
impl Labels {
    pub fn load(app: &tauri::AppHandle) -> Self {
        let locale = app
            .store_builder("settings.json")
            .disable_auto_save()
            .build()
            .ok()
            .and_then(|store| store.get("settings"))
            .and_then(|value| {
                value
                    .get("language")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(tauri_plugin_os::locale)
            .unwrap_or_else(|| "en-US".into());
        Self::for_locale(&locale)
    }
    pub fn for_locale(locale: &str) -> Self {
        let locale = supported_locale(locale);
        static MESSAGES: OnceLock<[Value; 5]> = OnceLock::new();
        let values = MESSAGES.get_or_init(|| {
            [
                include_str!("../../../../src/locales/en-US.json"),
                include_str!("../../../../src/locales/zh-CN.json"),
                include_str!("../../../../src/locales/zh-TW.json"),
                include_str!("../../../../src/locales/ja-JP.json"),
                include_str!("../../../../src/locales/ko-KR.json"),
            ]
            .map(|text| serde_json::from_str(text).expect("validated locale resource"))
        });
        let index = match locale {
            "zh-CN" => 1,
            "zh-TW" => 2,
            "ja-JP" => 3,
            "ko-KR" => 4,
            _ => 0,
        };
        Self {
            locale: locale.into(),
            messages: &values[index],
        }
    }
    pub fn text(&self, key: &str) -> &str {
        self.messages
            .pointer(&format!("/systemStatus/{key}"))
            .and_then(Value::as_str)
            .unwrap_or("—")
    }
    pub fn metric(&self, metric: MetricId) -> &str {
        self.text(match metric {
            MetricId::Cpu => "cpu",
            MetricId::Memory => "memory",
            MetricId::Network => "network",
            MetricId::Disk => "disk",
        })
    }
    pub fn status(&self, status: MetricStatus) -> &str {
        self.text(match status {
            MetricStatus::Loading => "loading",
            MetricStatus::Ready => "ready",
            MetricStatus::Stale => "stale",
            MetricStatus::Disconnected => "disconnected",
            MetricStatus::Unsupported => "unsupported",
            MetricStatus::Failed => "failed",
        })
    }
}

/// Match the frontend language-prefix policy for installations that have not
/// persisted a language yet. Traditional Chinese must precede generic Chinese.
fn supported_locale(locale: &str) -> &'static str {
    let locale = locale.trim().to_ascii_lowercase();
    for (prefix, supported) in [
        ("zh-tw", "zh-TW"),
        ("zh-hk", "zh-TW"),
        ("zh-mo", "zh-TW"),
        ("zh-hant", "zh-TW"),
        ("zh", "zh-CN"),
        ("ja", "ja-JP"),
        ("ko", "ko-KR"),
        ("en", "en-US"),
    ] {
        if locale == prefix || locale.starts_with(&format!("{prefix}-")) {
            return supported;
        }
    }
    "en-US"
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_defaults_match_supported_browser_language_prefixes() {
        for (input, expected) in [
            ("zh-CN", "zh-CN"),
            ("zh-Hant-HK", "zh-TW"),
            ("zh-SG", "zh-CN"),
            ("ja", "ja-JP"),
            ("ko-KR", "ko-KR"),
            ("en-GB", "en-US"),
            ("de-DE", "en-US"),
        ] {
            assert_eq!(Labels::for_locale(input).locale, expected);
        }
    }
}
