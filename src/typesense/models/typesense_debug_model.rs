use serde::Deserialize;

/// Response of Typesense's `/debug` endpoint, e.g. `{"state": 1, "version": "27.1"}`.
/// `state` is the raft role: 1 = leader, 4 = follower.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TypesenseDebug {
    pub state: i32,
}
