//! Cuerpo de `PUT /tasks/:id`: todo opcional, actualización parcial.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct UpdateTaskDto {
    pub title: Option<String>,
    pub done: Option<bool>,
}
