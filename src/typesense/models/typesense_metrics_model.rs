
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypesenseMetrics {
    pub system_cpu1_active_percentage: Option<String>,
    pub system_cpu2_active_percentage: Option<String>,
    pub system_cpu3_active_percentage: Option<String>,
    pub system_cpu4_active_percentage: Option<String>,
    pub system_cpu_active_percentage: Option<String>,
    pub system_disk_total_bytes: Option<String>,
    pub system_disk_used_bytes: Option<String>,
    pub system_memory_total_bytes: Option<String>,
    pub system_memory_used_bytes: Option<String>,
    pub system_network_received_bytes: Option<String>,
    pub system_network_sent_bytes: Option<String>,
    pub typesense_memory_active_bytes: Option<String>,
    pub typesense_memory_allocated_bytes: Option<String>,
    pub typesense_memory_fragmentation_ratio: Option<String>,
    pub typesense_memory_mapped_bytes: Option<String>,
    pub typesense_memory_metadata_bytes: Option<String>,
    pub typesense_memory_resident_bytes: Option<String>,
    pub typesense_memory_retained_bytes: Option<String>,
}