use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlateRecord {
    pub plate: String,
    pub timestamp: u32,
}
