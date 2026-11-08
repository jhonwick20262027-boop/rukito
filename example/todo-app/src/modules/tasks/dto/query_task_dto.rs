//! Query string de `GET /tasks` (`Query<T>`, RF-054).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct QueryTaskDto {
    /// Substring del título.
    pub q: Option<String>,
    /// Filtra por estado.
    pub done: Option<bool>,
}
