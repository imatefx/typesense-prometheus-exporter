use serde::Deserialize;

/// Response of Typesense's `/health` endpoint, e.g. `{"ok": true}`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TypesenseHealth {
    pub ok: bool,
}
