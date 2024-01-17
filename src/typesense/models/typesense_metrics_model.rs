use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypesenseMetrics {
    pub system_cpu1_active_percentage: String,
    #[serde(default = "system_cpu2_active_percentage_default")]
    pub system_cpu2_active_percentage: String,
     #[serde(default = "system_cpu3_active_percentage_default")]
    pub system_cpu3_active_percentage: String,
     #[serde(default = "system_cpu4_active_percentage_default")]
    pub system_cpu4_active_percentage: String,
     #[serde(default = "system_cpu5_active_percentage_default")]
    pub system_cpu5_active_percentage: String,
     #[serde(default = "system_cpu6_active_percentage_default")]
    pub system_cpu6_active_percentage: String,
     #[serde(default = "system_cpu7_active_percentage_default")]
    pub system_cpu7_active_percentage: String,
     #[serde(default = "system_cpu8_active_percentage_default")]
    pub system_cpu8_active_percentage: String,
    pub system_cpu_active_percentage: String,
    pub system_disk_total_bytes: String,
    pub system_disk_used_bytes: String,
    pub system_memory_total_bytes: String,
    pub system_memory_used_bytes: String,
    pub system_network_received_bytes: String,
    pub system_network_sent_bytes: String,
    pub typesense_memory_active_bytes: String,
    pub typesense_memory_allocated_bytes: String,
    pub typesense_memory_fragmentation_ratio: String,
    pub typesense_memory_mapped_bytes: String,
    pub typesense_memory_metadata_bytes: String,
    pub typesense_memory_resident_bytes: String,
    pub typesense_memory_retained_bytes: String,
}

impl Default for TypesenseMetrics {
    fn default() -> TypesenseMetrics {
        TypesenseMetrics {
            system_cpu1_active_percentage: "0".to_string(),
            system_cpu2_active_percentage: "0".to_string(),
            system_cpu3_active_percentage: "0".to_string(),
            system_cpu4_active_percentage: "0".to_string(),
            system_cpu5_active_percentage: "0".to_string(),
            system_cpu6_active_percentage: "0".to_string(),
            system_cpu7_active_percentage: "0".to_string(),
            system_cpu8_active_percentage: "0".to_string(),
            system_cpu_active_percentage: "0".to_string(),
            system_disk_total_bytes: "0".to_string(),
            system_disk_used_bytes: "0".to_string(),
            system_memory_total_bytes: "0".to_string(),
            system_memory_used_bytes: "0".to_string(),
            system_network_received_bytes: "0".to_string(),
            system_network_sent_bytes: "0".to_string(),
            typesense_memory_active_bytes: "0".to_string(),
            typesense_memory_allocated_bytes: "0".to_string(),
            typesense_memory_fragmentation_ratio: "0".to_string(),
            typesense_memory_mapped_bytes: "0".to_string(),
            typesense_memory_metadata_bytes: "0".to_string(),
            typesense_memory_resident_bytes: "0".to_string(),
            typesense_memory_retained_bytes: "0".to_string(),
        }
    }
}

fn system_cpu2_active_percentage_default() -> String{
  "0".to_string()
}

fn system_cpu3_active_percentage_default() -> String{
  "0".to_string()
}

fn system_cpu4_active_percentage_default() -> String{
  "0".to_string()
}


fn system_cpu5_active_percentage_default() -> String{
  "0".to_string()
}


fn system_cpu6_active_percentage_default() -> String{
  "0".to_string()
}


fn system_cpu7_active_percentage_default() -> String{
  "0".to_string()
}


fn system_cpu8_active_percentage_default() -> String{
  "0".to_string()
}

