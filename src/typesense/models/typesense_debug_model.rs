use serde::Deserialize;

/// Response of Typesense's `/debug` endpoint, e.g. `{"state": 1, "version": "30.1"}`.
/// `state` is the raft role: 1 = leader, 4 = follower.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TypesenseDebug {
    pub state: i32,
    pub version: Option<String>,
}
