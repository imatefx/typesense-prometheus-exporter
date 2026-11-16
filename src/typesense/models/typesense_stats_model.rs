use serde::Deserialize;
use serde_json::{Map, Value};

/// Response of Typesense's `/stats.json` endpoint. Fields missing from the
/// response (e.g. `cache_hit_ratio` on older Typesense versions) default to 0.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypesenseStats {
    pub cache_hit_ratio: f64,
    pub delete_latency_ms: f64,
    pub delete_requests_per_second: f64,
    pub import_latency_ms: f64,
    pub import_requests_per_second: f64,
    /// Per-endpoint latency, keyed by e.g. `"GET /collections/books/documents/search"`.
    pub latency_ms: Map<String, Value>,
    pub overloaded_requests_per_second: f64,
    pub pending_write_batches: f64,
    /// Per-endpoint request rate, same keys as `latency_ms`.
    pub requests_per_second: Map<String, Value>,
    pub search_latency_ms: f64,
    pub search_requests_per_second: f64,
    pub total_requests_per_second: f64,
    pub write_latency_ms: f64,
    pub write_requests_per_second: f64,
}

impl TypesenseStats {
    /// `(metric key, value)` pairs for the scalar stats, in export order.
    pub fn entries(&self) -> [(&'static str, f64); 12] {
        [
            ("cache_hit_ratio", self.cache_hit_ratio),
            ("delete_latency_ms", self.delete_latency_ms),
            (
                "delete_requests_per_second",
                self.delete_requests_per_second,
            ),
            ("import_latency_ms", self.import_latency_ms),
            (
                "import_requests_per_second",
                self.import_requests_per_second,
            ),
            (
                "overloaded_requests_per_second",
                self.overloaded_requests_per_second,
            ),
            ("pending_write_batches", self.pending_write_batches),
            ("search_latency_ms", self.search_latency_ms),
            (
                "search_requests_per_second",
                self.search_requests_per_second,
            ),
            ("total_requests_per_second", self.total_requests_per_second),
            ("write_latency_ms", self.write_latency_ms),
            ("write_requests_per_second", self.write_requests_per_second),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_default_to_zero() {
        let stats: TypesenseStats = serde_json::from_str(r#"{"search_latency_ms": 2.5}"#).unwrap();

        assert_eq!(stats.search_latency_ms, 2.5);
        assert_eq!(stats.cache_hit_ratio, 0.0);
        assert!(stats.latency_ms.is_empty());
    }

    #[test]
    fn entries_cover_every_scalar_stat() {
        let stats: TypesenseStats =
            serde_json::from_str(r#"{"cache_hit_ratio": 0.5, "write_requests_per_second": 7}"#)
                .unwrap();
        let entries = stats.entries();

        assert_eq!(entries.len(), 12);
        assert!(entries.contains(&("cache_hit_ratio", 0.5)));
        assert!(entries.contains(&("write_requests_per_second", 7.0)));
    }

    #[test]
    fn wrong_type_is_an_error() {
        assert!(
            serde_json::from_str::<TypesenseStats>(r#"{"search_latency_ms": "fast"}"#).is_err()
        );
    }
}
