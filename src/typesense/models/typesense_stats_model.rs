use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

/// Response of Typesense's `/stats.json` endpoint.
///
/// Every top-level numeric field is exported as a `typesense_stats{key=...}`
/// gauge: averages, percentiles (`search_95Percentile_latency_ms`), min/max,
/// request rates, cache counters and whatever later Typesense versions add.
/// Fields that are missing, `null` or not numbers are left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypesenseStats {
    /// Per-endpoint latency, keyed by e.g. `"GET /collections/books/documents/search"`.
    pub latency_ms: Map<String, Value>,
    /// Per-endpoint request rate, same keys as `latency_ms`.
    pub requests_per_second: Map<String, Value>,
    /// Every other top-level field.
    #[serde(flatten)]
    pub scalars: BTreeMap<String, Value>,
}

impl TypesenseStats {
    /// `(metric key, value)` pairs for every numeric scalar Typesense reported,
    /// sorted by key.
    pub fn entries(&self) -> impl Iterator<Item = (&str, f64)> {
        self.scalars
            .iter()
            .filter_map(|(key, value)| Some((key.as_str(), value.as_f64()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(json: &str) -> Vec<(String, f64)> {
        serde_json::from_str::<TypesenseStats>(json)
            .unwrap()
            .entries()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    #[test]
    fn exports_every_numeric_field() {
        let got = entries(
            r#"{"search_latency_ms": 1.5, "search_95Percentile_latency_ms": 4,
                "search_max_latency_ms": 9, "cache_hit_count": 12, "some_future_stat": 3}"#,
        );

        assert_eq!(
            got,
            [
                ("cache_hit_count".to_string(), 12.0),
                ("search_95Percentile_latency_ms".to_string(), 4.0),
                ("search_latency_ms".to_string(), 1.5),
                ("search_max_latency_ms".to_string(), 9.0),
                ("some_future_stat".to_string(), 3.0),
            ]
        );
    }

    #[test]
    fn unreported_null_and_non_numeric_fields_are_omitted() {
        let got = entries(
            r#"{"cache_hit_ratio": null, "search_latency_ms": "fast", "write_latency_ms": 2}"#,
        );

        assert_eq!(got, [("write_latency_ms".to_string(), 2.0)]);
        assert!(entries("{}").is_empty());
    }

    #[test]
    fn per_endpoint_maps_are_not_scalars() {
        let stats: TypesenseStats = serde_json::from_str(
            r#"{"latency_ms": {"GET /health": 0.1}, "requests_per_second": {"GET /health": 2},
                "total_requests_per_second": 2}"#,
        )
        .unwrap();

        assert_eq!(stats.latency_ms.len(), 1);
        assert_eq!(stats.requests_per_second.len(), 1);
        assert_eq!(
            stats.entries().collect::<Vec<_>>(),
            [("total_requests_per_second", 2.0)]
        );
    }
}
