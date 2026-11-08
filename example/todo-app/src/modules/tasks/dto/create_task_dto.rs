//! Cuerpo de `POST /tasks`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateTaskDto {
    pub title: String,
    pub done: Option<bool>,
}
