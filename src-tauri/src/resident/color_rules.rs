//! Independent, non-overlapping color conditions with metric-specific units.
use super::preference_schema::DisplayMetricId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayColor {
    #[default]
    Default,
    Green,
    Yellow,
    Orange,
    Red,
    Blue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "camelCase", deny_unknown_fields)]
pub enum ColorRule {
    Below {
        value: u16,
        color: DisplayColor,
    },
    Range {
        from: u16,
        to: u16,
        color: DisplayColor,
    },
    AtLeast {
        value: u16,
        color: DisplayColor,
    },
}

impl ColorRule {
    pub fn bounds(&self) -> (u16, Option<u16>) {
        match *self {
            Self::Below { value, .. } => (0, Some(value)),
            Self::Range { from, to, .. } => (from, Some(to)),
            Self::AtLeast { value, .. } => (value, None),
        }
    }

    pub fn color(&self) -> DisplayColor {
        match *self {
            Self::Below { color, .. } | Self::Range { color, .. } | Self::AtLeast { color, .. } => {
                color
            }
        }
    }

    pub fn matches(&self, value: u16) -> bool {
        let (from, to) = self.bounds();
        value >= from && to.is_none_or(|to| value < to)
    }

    fn valid(&self, limit: u16) -> bool {
        let (from, to) = self.bounds();
        from <= limit && to.is_none_or(|to| to <= limit && from < to)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MetricColorRule {
    pub id: DisplayMetricId,
    pub enabled: bool,
    pub rules: Vec<ColorRule>,
}

pub const MAX_COLOR_RULES: usize = 16;
pub const COLOR_METRICS: [DisplayMetricId; 6] = [
    DisplayMetricId::Cpu,
    DisplayMetricId::CpuTemperature,
    DisplayMetricId::Gpu,
    DisplayMetricId::GpuTemperature,
    DisplayMetricId::Memory,
    DisplayMetricId::Disk,
];

pub fn temperature(id: DisplayMetricId) -> bool {
    matches!(
        id,
        DisplayMetricId::CpuTemperature | DisplayMetricId::GpuTemperature
    )
}

impl MetricColorRule {
    pub fn defaults(id: DisplayMetricId) -> Self {
        let (middle, high) = if temperature(id) { (60, 80) } else { (70, 90) };
        Self {
            id,
            enabled: true,
            rules: vec![
                ColorRule::Below {
                    value: middle,
                    color: DisplayColor::Green,
                },
                ColorRule::Range {
                    from: middle,
                    to: high,
                    color: DisplayColor::Orange,
                },
                ColorRule::AtLeast {
                    value: high,
                    color: DisplayColor::Red,
                },
            ],
        }
    }

    pub fn valid(&self) -> bool {
        let limit = if temperature(self.id) { 150 } else { 100 };
        COLOR_METRICS.contains(&self.id)
            && self.rules.len() <= MAX_COLOR_RULES
            && self.rules.iter().all(|rule| rule.valid(limit))
            && self.rules.iter().enumerate().all(|(index, rule)| {
                let (from, to) = rule.bounds();
                self.rules[index + 1..].iter().all(|other| {
                    let (other_from, other_to) = other.bounds();
                    to.is_some_and(|to| to <= other_from) || other_to.is_some_and(|to| to <= from)
                })
            })
    }
}

pub fn defaults() -> Vec<MetricColorRule> {
    COLOR_METRICS
        .into_iter()
        .map(MetricColorRule::defaults)
        .collect()
}

pub fn valid(rules: &[MetricColorRule]) -> bool {
    rules.len() == COLOR_METRICS.len()
        && COLOR_METRICS
            .iter()
            .all(|id| rules.iter().filter(|rule| rule.id == *id).count() == 1)
        && rules.iter().all(MetricColorRule::valid)
}

/// Preserve the former normal color and keep previously uncolored temperatures off.
pub fn legacy(warning: u16, critical: u16) -> Vec<MetricColorRule> {
    defaults()
        .into_iter()
        .map(|mut rule| {
            if temperature(rule.id) {
                rule.enabled = false;
            } else {
                rule.rules = vec![
                    ColorRule::Below {
                        value: warning,
                        color: DisplayColor::Default,
                    },
                    ColorRule::Range {
                        from: warning,
                        to: critical,
                        color: DisplayColor::Orange,
                    },
                    ColorRule::AtLeast {
                        value: critical,
                        color: DisplayColor::Red,
                    },
                ];
            }
            rule
        })
        .collect()
}

/// Decode version 15 strictly before converting bands to equivalent conditions.
pub fn migrate_bands(value: serde_json::Value) -> Result<Vec<MetricColorRule>, &'static str> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Band {
        from: u16,
        color: DisplayColor,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Previous {
        id: DisplayMetricId,
        enabled: bool,
        bands: Vec<Band>,
    }
    let previous: Vec<Previous> =
        serde_json::from_value(value).map_err(|_| "preferences_invalid")?;
    let mut migrated = Vec::with_capacity(previous.len());
    for rule in previous {
        let limit = if temperature(rule.id) { 150 } else { 100 };
        if !(2..=5).contains(&rule.bands.len())
            || rule.bands[0].from != 0
            || rule.bands.iter().any(|band| band.from > limit)
            || rule
                .bands
                .windows(2)
                .any(|pair| pair[0].from >= pair[1].from)
        {
            return Err("preferences_invalid");
        }
        let rules = rule
            .bands
            .iter()
            .enumerate()
            .map(|(index, band)| match rule.bands.get(index + 1) {
                Some(next) if index == 0 => ColorRule::Below {
                    value: next.from,
                    color: band.color,
                },
                Some(next) => ColorRule::Range {
                    from: band.from,
                    to: next.from,
                    color: band.color,
                },
                None => ColorRule::AtLeast {
                    value: band.from,
                    color: band.color,
                },
            })
            .collect();
        migrated.push(MetricColorRule {
            id: rule.id,
            enabled: rule.enabled,
            rules,
        });
    }
    if valid(&migrated) {
        Ok(migrated)
    } else {
        Err("preferences_invalid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_accept_empty_single_gapped_and_reordered_rules() {
        let mut rule = MetricColorRule::defaults(DisplayMetricId::Cpu);
        assert!(rule.valid());
        rule.rules.reverse();
        assert!(rule.valid());
        rule.rules = vec![
            ColorRule::Below {
                value: 20,
                color: DisplayColor::Green,
            },
            ColorRule::AtLeast {
                value: 90,
                color: DisplayColor::Red,
            },
        ];
        assert!(rule.valid());
        assert!(!rule.rules.iter().any(|rule| rule.matches(70)));
        rule.rules.remove(0);
        assert!(rule.valid());
        rule.rules.clear();
        assert!(rule.valid());
    }

    #[test]
    fn conditions_reject_overlap_empty_ranges_duplicate_metrics_and_wrong_units() {
        let mut rules = defaults();
        rules[0].rules[0] = ColorRule::Below {
            value: 71,
            color: DisplayColor::Green,
        };
        assert!(!valid(&rules));
        rules = defaults();
        rules[0].rules = vec![ColorRule::Range {
            from: 70,
            to: 70,
            color: DisplayColor::Green,
        }];
        assert!(!valid(&rules));
        rules[0].rules = vec![ColorRule::AtLeast {
            value: 120,
            color: DisplayColor::Red,
        }];
        assert!(!valid(&rules));
        rules[1].rules = rules[0].rules.clone();
        assert!(rules[1].valid());
        rules = defaults();
        rules[0].id = DisplayMetricId::Disk;
        assert!(!valid(&rules));
    }
}
