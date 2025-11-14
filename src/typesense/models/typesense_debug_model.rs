use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypesenseDebug {
    pub version: String,
    pub state: i32,
}

impl Default for TypesenseDebug {
    fn default() -> TypesenseDebug {
        TypesenseDebug {
            version: String::new(),
            state: 0,
        }
    }
}

