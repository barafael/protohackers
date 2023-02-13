use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Camera {
    pub road: u16,
    pub mile: u16,
    pub limit: u16,
}
