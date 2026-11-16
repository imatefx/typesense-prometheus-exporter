use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// Response of Typesense's `/metrics.json` endpoint. Typesense reports every
/// value as a string; keys missing from the response are left empty and not
/// exported.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypesenseMetrics {
    pub system_cpu_active_percentage: String,
    pub system_disk_total_bytes: String,
    pub system_disk_used_bytes: String,
    pub system_memory_total_bytes: String,
    pub system_memory_used_bytes: String,
    pub system_memory_total_swap_bytes: String,
    pub system_memory_used_swap_bytes: String,
    pub system_network_received_bytes: String,
    pub system_network_sent_bytes: String,
    pub typesense_memory_active_bytes: String,
    pub typesense_memory_allocated_bytes: String,
    pub typesense_memory_fragmentation_ratio: String,
    pub typesense_memory_mapped_bytes: String,
    pub typesense_memory_metadata_bytes: String,
    pub typesense_memory_resident_bytes: String,
    pub typesense_memory_retained_bytes: String,
    /// Every other key. Per-core `system_cpuN_active_percentage` values live
    /// here, since the number of cores varies by host.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

impl TypesenseMetrics {
    /// `(metric key, raw value)` pairs for every metric Typesense reported:
    /// the fixed metrics followed by one entry per CPU core.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        let per_core = self.other.iter().filter_map(|(key, value)| {
            core_number(key)?;
            Some((key.as_str(), value.as_str()?))
        });

        self.fixed_entries().chain(per_core)
    }

    /// `(metric key, raw value)` pairs for the fixed fields Typesense
    /// reported, i.e. everything except the per-core CPU values and unknown
    /// fields, which stay in [`TypesenseMetrics::other`].
    pub fn fixed_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            (
                "system_cpu_active_percentage",
                &self.system_cpu_active_percentage,
            ),
            ("system_disk_total_bytes", &self.system_disk_total_bytes),
            ("system_disk_used_bytes", &self.system_disk_used_bytes),
            ("system_memory_total_bytes", &self.system_memory_total_bytes),
            ("system_memory_used_bytes", &self.system_memory_used_bytes),
            (
                "system_memory_total_swap_bytes",
                &self.system_memory_total_swap_bytes,
            ),
            (
                "system_memory_used_swap_bytes",
                &self.system_memory_used_swap_bytes,
            ),
            (
                "system_network_received_bytes",
                &self.system_network_received_bytes,
            ),
            ("system_network_sent_bytes", &self.system_network_sent_bytes),
            (
                "typesense_memory_active_bytes",
                &self.typesense_memory_active_bytes,
            ),
            (
                "typesense_memory_allocated_bytes",
                &self.typesense_memory_allocated_bytes,
            ),
            (
                "typesense_memory_fragmentation_ratio",
                &self.typesense_memory_fragmentation_ratio,
            ),
            (
                "typesense_memory_mapped_bytes",
                &self.typesense_memory_mapped_bytes,
            ),
            (
                "typesense_memory_metadata_bytes",
                &self.typesense_memory_metadata_bytes,
            ),
            (
                "typesense_memory_resident_bytes",
                &self.typesense_memory_resident_bytes,
            ),
            (
                "typesense_memory_retained_bytes",
                &self.typesense_memory_retained_bytes,
            ),
        ]
        .map(|(key, value)| (key, value.as_str()))
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
    }
}

/// The core number in a per-core CPU key, e.g. `"3"` for
/// `system_cpu3_active_percentage`; `None` for any other key.
pub fn core_number(key: &str) -> Option<&str> {
    let core = key
        .strip_prefix("system_cpu")?
        .strip_suffix("_active_percentage")?;
    (!core.is_empty() && core.bytes().all(|b| b.is_ascii_digit())).then_some(core)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(m: &TypesenseMetrics) -> Vec<&str> {
        m.entries().map(|(k, _)| k).collect()
    }

    #[test]
    fn exports_every_numbered_core() {
        let json = (1..=32)
            .map(|n| format!(r#""system_cpu{n}_active_percentage": "{n}""#))
            .collect::<Vec<_>>()
            .join(",");
        let m: TypesenseMetrics = serde_json::from_str(&format!("{{{json}}}")).unwrap();
        let cores: Vec<_> = m
            .entries()
            .filter(|(k, _)| k.starts_with("system_cpu") && *k != "system_cpu_active_percentage")
            .collect();

        assert_eq!(cores.len(), 32);
        assert!(cores.contains(&("system_cpu32_active_percentage", "32")));
    }

    #[test]
    fn ignores_non_core_and_non_string_values() {
        let m: TypesenseMetrics = serde_json::from_str(
            r#"{"system_cpuX_active_percentage": "1", "system_cpu_active_percentage_extra": "1",
                "system_cpu7_active_percentage": 7, "system_cpu_active_percentage": "9",
                "unrelated": "1"}"#,
        )
        .unwrap();
        let keys = keys(&m);

        assert!(!keys
            .iter()
            .any(|k| k.contains("cpuX") || k.contains("extra") || *k == "unrelated"));
        assert!(
            !keys.contains(&"system_cpu7_active_percentage"),
            "numbers are not strings"
        );
        assert!(keys.contains(&"system_cpu_active_percentage"));
    }

    #[test]
    fn recognises_core_numbers() {
        assert_eq!(core_number("system_cpu12_active_percentage"), Some("12"));
        for key in [
            "system_cpu_active_percentage",
            "system_cpuX_active_percentage",
            "system_cpu1_active_percentage_extra",
            "system_memory_used_bytes",
        ] {
            assert_eq!(core_number(key), None, "{key}");
        }
    }

    #[test]
    fn fixed_entries_leave_out_cores_and_unknown_fields() {
        let m: TypesenseMetrics = serde_json::from_str(
            r#"{"system_cpu1_active_percentage": "1", "system_cpu_active_percentage": "9",
                "unrelated": "1"}"#,
        )
        .unwrap();

        assert_eq!(
            m.fixed_entries().collect::<Vec<_>>(),
            [("system_cpu_active_percentage", "9")]
        );
    }

    #[test]
    fn unreported_metrics_are_skipped() {
        let m: TypesenseMetrics =
            serde_json::from_str(r#"{"system_memory_total_bytes": "1024"}"#).unwrap();

        assert_eq!(keys(&m), ["system_memory_total_bytes"]);
        assert_eq!(
            serde_json::from_str::<TypesenseMetrics>("{}")
                .unwrap()
                .entries()
                .count(),
            0
        );
    }
}
