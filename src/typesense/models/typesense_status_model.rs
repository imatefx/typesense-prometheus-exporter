use serde::Deserialize;

/// Response of Typesense's `/status` endpoint, e.g.
/// `{"committed_index": 18, "queued_writes": 0, "state": "LEADER"}`.
///
/// Comparing `committed_index` across nodes shows how far each follower lags
/// the leader. Fields a Typesense version doesn't report are left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypesenseStatus {
    pub committed_index: Option<f64>,
    pub queued_writes: Option<f64>,
}
