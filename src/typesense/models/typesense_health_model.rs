use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypesenseHealth {
    pub ok: bool,
    #[serde(default)]
    pub resource_error: Option<String>,
}

impl Default for TypesenseHealth {
    fn default() -> TypesenseHealth {
        TypesenseHealth {
            ok: false,
            resource_error: None,
        }
    }
}

