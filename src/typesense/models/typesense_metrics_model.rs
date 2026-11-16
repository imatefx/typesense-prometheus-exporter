use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

/// Response of Typesense's `/metrics.json` endpoint. Typesense reports every
/// value as a string; missing keys default to empty and are exported as 0.
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
    /// `(metric key, raw value)` pairs: the fixed metrics followed by one entry
    /// per CPU core that Typesense reported.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        let fixed = [
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
        .map(|(key, value)| (key, value.as_str()));

        let per_core = self.other.iter().filter_map(|(key, value)| {
            let core = key
                .strip_prefix("system_cpu")?
                .strip_suffix("_active_percentage")?;
            let is_core_number = !core.is_empty() && core.bytes().all(|b| b.is_ascii_digit());
            Some((key.as_str(), value.as_str()?)).filter(|_| is_core_number)
        });

        fixed.into_iter().chain(per_core)
    }
}
